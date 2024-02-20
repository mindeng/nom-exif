use std::{
    cmp,
    io::{Read, Seek},
    ops::Range,
};

use chrono::DateTime;
use nom::{bytes::streaming, IResult};
use thiserror::Error;

use crate::{
    bbox::{
        find_box, get_ftyp, parse_video_tkhd_in_moov, travel_header, travel_while, IlstBox,
        KeysBox, MvhdBox, ParseBox,
    },
    file::FileType,
    EntryValue,
};

/// Analyze the byte stream in the `reader` as a MOV/MP4 file, attempting to
/// extract any possible metadata it may contain, and return it in the form of
/// key-value pairs.
///
/// Please note that the parsing routine itself provides a buffer, so the
/// `reader` may not need to be wrapped with `BufRead`.
///
/// # Usage
///
/// ```rust
/// use nom_exif::*;
///
/// use std::fs::File;
/// use std::path::Path;
///
/// let f = File::open(Path::new("./testdata/meta.mov")).unwrap();
/// let entries = parse_mov_metadata(f).unwrap();
///
/// assert_eq!(
///     entries
///         .iter()
///         .map(|x| format!("{x:?}"))
///         .collect::<Vec<_>>()
///         .join("\n"),
///     r#"("com.apple.quicktime.make", Text("Apple"))
/// ("com.apple.quicktime.model", Text("iPhone X"))
/// ("com.apple.quicktime.software", Text("12.1.2"))
/// ("com.apple.quicktime.location.ISO6709", Text("+27.1281+100.2508+000.000/"))
/// ("com.apple.quicktime.creationdate", Time(2019-02-12T15:27:12+08:00))
/// ("duration", U32(500))
/// ("width", U32(720))
/// ("height", U32(1280))"#,
/// );
/// ```
pub fn parse_metadata<R: Read + Seek>(reader: R) -> crate::Result<Vec<(String, EntryValue)>> {
    let (ft, moov_body) = extract_moov_body(reader)?;

    let (_, mut entries) = match parse_moov_body(&moov_body) {
        Ok(entries) => entries,
        Err(_) => {
            if ft == FileType::QuickTime {
                return Err("parse moov body failed".into());
            }
            (&b""[..], Vec::new())
        }
    };

    if ft == FileType::MP4 {
        const LOCATION_KEY: &str = "com.apple.quicktime.location.ISO6709";

        if entries.iter().find(|x| x.0 == LOCATION_KEY).is_none() {
            // Try to parse GPS location for MP4 files. For mp4 files, Android
            // phones store GPS info in the `moov/udta/©xyz` atom.
            let (_, bbox) = find_box(&moov_body, "udta/©xyz").map_err(|_| "udta/©xyz not found")?;
            if let Some(bbox) = bbox {
                let location = &bbox.body_data()[4..]
                    .iter()
                    .map(|b| *b as char)
                    .collect::<String>();
                entries.push((LOCATION_KEY.to_owned(), location.into()))
            }
        }
    }

    const CREATIONDATE_KEY: &str = "com.apple.quicktime.creationdate";
    if let Some(pos) = entries.iter().position(|x| x.0 == CREATIONDATE_KEY) {
        if let EntryValue::Text(ref s) = entries[pos].1 {
            if let Ok(t) = DateTime::parse_from_str(s, "%+") {
                let _ = std::mem::replace(
                    &mut entries[pos],
                    (CREATIONDATE_KEY.to_string(), EntryValue::Time(t)),
                );
            }
        }
    }

    let (_, bbox) = find_box(&moov_body, "mvhd")?;
    if let Some(bbox) = bbox {
        let (_, mvhd) = MvhdBox::parse_box(bbox.data)?;

        entries.push(("duration".to_owned(), mvhd.duration_ms().into()));

        if entries.iter().find(|x| x.0 == CREATIONDATE_KEY).is_none() {
            entries.push((
                "com.apple.quicktime.creationdate".to_owned(),
                EntryValue::Time(mvhd.creation_time()),
            ));
        }
    }

    if let Ok(tkhd) = parse_video_tkhd_in_moov(&moov_body) {
        entries.push(("width".to_owned(), tkhd.width.into()));
        entries.push(("height".to_owned(), tkhd.height.into()));
    }

    Ok(entries)
}

/// Analyze the byte stream in the `reader` as a MOV file, attempting to extract
/// any possible metadata it may contain, and return it in the form of key-value
/// pairs.
///
/// Please note that the parsing routine itself provides a buffer, so the
/// `reader` may not need to be wrapped with `BufRead`.
///
/// # Usage
///
/// ```rust
/// use nom_exif::*;
///
/// use std::fs::File;
/// use std::path::Path;
///
/// let f = File::open(Path::new("./testdata/meta.mov")).unwrap();
/// let entries = parse_mov_metadata(f).unwrap();
///
/// assert_eq!(
///     entries
///         .iter()
///         .map(|x| format!("{x:?}"))
///         .collect::<Vec<_>>()
///         .join("\n"),
///     r#"("com.apple.quicktime.make", Text("Apple"))
/// ("com.apple.quicktime.model", Text("iPhone X"))
/// ("com.apple.quicktime.software", Text("12.1.2"))
/// ("com.apple.quicktime.location.ISO6709", Text("+27.1281+100.2508+000.000/"))
/// ("com.apple.quicktime.creationdate", Time(2019-02-12T15:27:12+08:00))
/// ("duration", U32(500))
/// ("width", U32(720))
/// ("height", U32(1280))"#,
/// );
/// ```
pub fn parse_mov_metadata<R: Read + Seek>(reader: R) -> crate::Result<Vec<(String, EntryValue)>> {
    parse_metadata(reader)
}

fn extract_moov_body<R: Read + Seek>(mut reader: R) -> Result<(FileType, Vec<u8>), crate::Error> {
    const INIT_BUF_SIZE: usize = 4096;
    const GROW_BUF_SIZE: usize = 4096;
    let mut buf = Vec::with_capacity(INIT_BUF_SIZE);

    buf.reserve(INIT_BUF_SIZE);
    let n = reader
        .by_ref()
        .take(INIT_BUF_SIZE as u64)
        .read_to_end(buf.as_mut())?;
    if n == 0 {
        Err("file is empty")?;
    }

    let ft = check_ftyp(&buf)?;

    let mut offset = 0;
    let moov_body_range = loop {
        let input = if offset > 0 { &buf[offset..] } else { &buf[..] };

        let to_read = match extract_moov_body_from_buf(input) {
            Ok(range) => break range.start + offset..range.end + offset,
            Err(Error::Need(n)) => n,
            Err(Error::Skip(n)) => {
                // println!("skip: {n}");
                reader.seek(std::io::SeekFrom::Current(n as i64))?;
                offset = buf.len();
                GROW_BUF_SIZE
            }
            Err(Error::ParseFailed(e)) => return Err(e),
        };

        // println!("to_read: {to_read}");
        assert!(to_read > 0);

        let to_read = cmp::max(GROW_BUF_SIZE, to_read);
        buf.reserve(to_read);

        let n = reader
            .by_ref()
            .take(to_read as u64)
            .read_to_end(buf.as_mut())?;
        if n == 0 {
            Err("metadata not found")?;
        }
    };
    Ok((ft, buf.drain(moov_body_range).collect()))
}

/// Due to the fact that metadata in MOV files is typically located at the end
/// of the file, conventional parsing methods would require reading a
/// significant amount of unnecessary data during the parsing process. This
/// would impact the performance of the parsing program and consume more memory.
///
/// To address this issue, we have defined an `Error::Skip` enumeration type to
/// inform the caller that certain bytes in the parsing process are not required
/// and can be skipped directly. The specific method of skipping can be
/// determined by the caller based on the situation. For example:
///
/// - For files, you can quickly skip using a `Seek` operation.
///
/// - For network byte streams, you may need to skip these bytes through read
/// operations, or preferably, by designing an appropriate network protocol for
/// skipping.
///
/// # `Error::Skip`
///
/// Please note that when the caller receives an `Error::Skip(n)` error, it
/// should be understood as follows:
///
/// - The parsing program has already consumed all available data and needs to
/// skip n bytes further.
///
/// - After skipping n bytes, it should continue to read subsequent data to fill
/// the buffer and use it as input for the parsing function.
///
/// - The next time the parsing function is called (usually within a loop), the
/// previously consumed data (including the skipped bytes) should be ignored,
/// and only the newly read data should be passed in.
///
/// # `Error::Need`
///
/// Additionally, to simplify error handling, we have integrated
/// `nom::Err::Incomplete` error into `Error::Need`. This allows us to use the
/// same error type to notify the caller that we require more bytes to continue
/// parsing.
#[derive(Debug, Error)]
pub enum Error {
    #[error("skip {0} bytes")]
    Skip(u64),

    #[error("need {0} more bytes")]
    Need(usize),

    #[error("{0}")]
    ParseFailed(crate::Error),
}

/// Parse the byte data of an ISOBMFF file and return the potential body data of
/// moov atom it may contain.
///
/// Regarding error handling, please refer to [Error] for more information.
fn extract_moov_body_from_buf<'a>(input: &'a [u8]) -> Result<Range<usize>, Error> {
    // parse metadata from moov/meta/keys & moov/meta/ilst
    let remain = input;

    let convert_error = |e: nom::Err<_>, msg: &str| match e {
        nom::Err::Incomplete(needed) => match needed {
            nom::Needed::Unknown => Error::Need(4096),
            nom::Needed::Size(n) => Error::Need(n.get()),
        },
        nom::Err::Error(_) => Error::ParseFailed(msg.into()),
        nom::Err::Failure(_) => Error::ParseFailed(msg.into()),
    };

    let mut to_skip = 0;
    let mut skipped = 0;
    let (remain, header) = travel_header(remain, |h, remain| {
        // println!("got: {} {}", h.box_type, h.box_size);
        if h.box_type == "moov" {
            // stop travelling
            skipped += h.header_size;
            false
        } else {
            if (remain.len() as u64) < h.body_size() {
                // stop travelling & skip unused box data
                to_skip = h.body_size() - remain.len() as u64;
                false
            } else {
                skipped += h.box_size as usize;
                true
            }
        }
    })
    .map_err(|e| convert_error(e, "search atom moov failed"))?;

    if to_skip > 0 {
        return Err(Error::Skip(to_skip));
    }

    let (_, body) = streaming::take(header.body_size())(remain)
        .map_err(|e| convert_error(e, "moov is too small"))?;

    Ok(skipped..skipped + body.len())
}

fn check_ftyp(input: &[u8]) -> crate::Result<FileType> {
    let Some(ftyp) = get_ftyp(input)? else {
        // ftyp is None, assume it's a MOV file extracted from HEIC
        return Ok(FileType::QuickTime);
    };

    match ftyp {
        b"qt  " => Ok(FileType::QuickTime),
        b"mp41" => Ok(FileType::MP4),
        b"mp42" => Ok(FileType::MP4),

        o => Err(format!("unsupported MOV file; ftyp: {o:?}").into()),
    }
}

fn parse_moov_body(remain: &[u8]) -> IResult<&[u8], Vec<(String, EntryValue)>> {
    let (_, meta) = travel_while(remain, |b| b.header.box_type != "meta")?;
    let (_, keys) = travel_while(&meta.data[meta.header_size()..], |b| {
        b.header.box_type != "keys"
    })?;
    let (_, ilst) = travel_while(&meta.data[meta.header_size()..], |b| {
        b.header.box_type != "ilst"
    })?;

    let (_, keys) = KeysBox::parse_box(keys.data)?;
    let (_, ilst) = IlstBox::parse_box(ilst.data)?;

    let entries = keys
        .entries
        .into_iter()
        .map(|k| k.key)
        .zip(ilst.items.into_iter().map(|v| v.value))
        .collect::<Vec<_>>();
    // .collect::<HashMap<_, _>>();

    Ok((remain, entries))
}

/// Change timezone format from iso 8601 to rfc3339, e.g.:
///
/// - `2023-11-02T19:58:34+08` -> `2023-11-02T19:58:34+08:00`
/// - `2023-11-02T19:58:34+0800` -> `2023-11-02T19:58:34+08:00`
#[allow(dead_code)]
fn tz_iso_8601_to_rfc3339(s: String) -> String {
    use regex::Regex;

    let ss = s.trim();
    let re = Regex::new(r"([+-][0-9][0-9])([0-9][0-9])?$").unwrap();

    if let Some((offset, tz)) = re.captures(ss).map(|caps| {
        (
            caps.get(1).unwrap().start(),
            format!(
                "{}:{}",
                caps.get(1).map_or("00", |m| m.as_str()),
                caps.get(2).map_or("00", |m| m.as_str())
            ),
        )
    }) {
        let s1 = &ss.as_bytes()[..offset];
        let s2 = tz.as_bytes();
        s1.iter().chain(s2.iter()).map(|x| *x as char).collect()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use test_case::test_case;

    #[test_case("meta.mov")]
    fn mov_parse(path: &str) {
        let reader = open_sample(path).unwrap();
        let entries = parse_metadata(reader).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|x| format!("{x:?}"))
                .collect::<Vec<_>>()
                .join("\n"),
            "(\"com.apple.quicktime.make\", Text(\"Apple\"))
(\"com.apple.quicktime.model\", Text(\"iPhone X\"))
(\"com.apple.quicktime.software\", Text(\"12.1.2\"))
(\"com.apple.quicktime.location.ISO6709\", Text(\"+27.1281+100.2508+000.000/\"))
(\"com.apple.quicktime.creationdate\", Time(2019-02-12T15:27:12+08:00))
(\"duration\", U32(500))
(\"width\", U32(720))
(\"height\", U32(1280))"
        );
    }

    #[test_case("meta.mov")]
    fn mov_extract_mov(path: &str) {
        let buf = read_sample(path).unwrap();
        println!("file size: {}", buf.len());
        let range = extract_moov_body_from_buf(&buf).unwrap();
        let (_, entries) = parse_moov_body(&buf[range]).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|x| format!("{x:?}"))
                .collect::<Vec<_>>()
                .join("\n"),
            "(\"com.apple.quicktime.make\", Text(\"Apple\"))
(\"com.apple.quicktime.model\", Text(\"iPhone X\"))
(\"com.apple.quicktime.software\", Text(\"12.1.2\"))
(\"com.apple.quicktime.location.ISO6709\", Text(\"+27.1281+100.2508+000.000/\"))
(\"com.apple.quicktime.creationdate\", Text(\"2019-02-12T15:27:12+08:00\"))"
        );
    }

    #[test_case("meta.mp4")]
    fn parse_mp4(path: &str) {
        let entries = parse_metadata(open_sample(path).unwrap()).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|x| format!("{x:?}"))
                .collect::<Vec<_>>()
                .join("\n"),
            "(\"com.apple.quicktime.location.ISO6709\", Text(\"+27.2939+112.6932/\"))
(\"duration\", U32(1063))
(\"com.apple.quicktime.creationdate\", Time(2024-02-03T07:05:38+00:00))
(\"width\", U32(1920))
(\"height\", U32(1080))"
        );
    }

    #[test_case("embedded-in-heic.mov")]
    fn parse_embedded_mov(path: &str) {
        let entries = parse_mov_metadata(open_sample(path).unwrap()).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|x| format!("{x:?}"))
                .collect::<Vec<_>>()
                .join("\n"),
            "(\"com.apple.quicktime.location.accuracy.horizontal\", Text(\"14.235563\"))
(\"com.apple.quicktime.live-photo.auto\", U8(1))
(\"com.apple.quicktime.content.identifier\", Text(\"DA1A7EE8-0925-4C9F-9266-DDA3F0BB80F0\"))
(\"com.apple.quicktime.live-photo.vitality-score\", F32(0.93884003))
(\"com.apple.quicktime.live-photo.vitality-scoring-version\", I64(4))
(\"com.apple.quicktime.location.ISO6709\", Text(\"+22.5797+113.9380+028.396/\"))
(\"com.apple.quicktime.make\", Text(\"Apple\"))
(\"com.apple.quicktime.model\", Text(\"iPhone 15 Pro\"))
(\"com.apple.quicktime.software\", Text(\"17.1\"))
(\"com.apple.quicktime.creationdate\", Time(2023-11-02T19:58:34+08:00))
(\"duration\", U32(2795))
(\"width\", U32(1920))
(\"height\", U32(1440))"
        );
    }

    #[test]
    fn test_iso_8601_tz_to_rfc3339() {
        let s = "2023-11-02T19:58:34+08".to_string();
        assert_eq!(tz_iso_8601_to_rfc3339(s), "2023-11-02T19:58:34+08:00");

        let s = "2023-11-02T19:58:34+0800".to_string();
        assert_eq!(tz_iso_8601_to_rfc3339(s), "2023-11-02T19:58:34+08:00");

        let s = "2023-11-02T19:58:34+08:00".to_string();
        assert_eq!(tz_iso_8601_to_rfc3339(s), "2023-11-02T19:58:34+08:00");

        let s = "2023-11-02T19:58:34Z".to_string();
        assert_eq!(tz_iso_8601_to_rfc3339(s), "2023-11-02T19:58:34Z");

        let s = "2023-11-02T19:58:34".to_string();
        assert_eq!(tz_iso_8601_to_rfc3339(s), "2023-11-02T19:58:34");
    }
}

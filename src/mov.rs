use std::{
    cmp,
    io::{Read, Seek},
};

use nom::IResult;
use thiserror::Error;

use crate::bbox::{
    get_ftyp, travel_header, travel_while, BoxHolder, IlstBox, IlstItemValue, KeysBox, ParseBox,
};

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
/// ("com.apple.quicktime.creationdate", Text("2019-02-12T15:27:12+08:00"))"#
/// );
/// ```
pub fn parse_mov_metadata<R: Read + Seek>(
    mut reader: R,
) -> crate::Result<Vec<(String, IlstItemValue)>> {
    const INIT_BUF_SIZE: usize = 4096;
    const GROW_BUF_SIZE: usize = 4096;

    let mut buf = Vec::with_capacity(INIT_BUF_SIZE);

    let n = reader
        .by_ref()
        .take(INIT_BUF_SIZE as u64)
        .read_to_end(buf.as_mut())?;
    if n == 0 {
        Err("file is empty")?;
    }

    let (_, bbox) = BoxHolder::parse(&buf).map_err(|_| "Invalid ISOBMFF file; parse box failed")?;
    // MOV files that extracts from HEIC starts with `wide` & `mdat` atoms
    if bbox.box_type() != "wide" {
        check_mov(&buf)?;
    }

    let mut offset = 0;
    let metadata = loop {
        let input = if offset > 0 { &buf[offset..] } else { &buf[..] };

        let to_read = match extract_metadata(input) {
            Ok(metadata) => break metadata,
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

    Ok(metadata)
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

/// Parse the byte data of a MOV file and return the potential metadata it may
/// contain.
///
/// The metadata is returned in the form of a series of key-value pairs.
///
/// Regarding error handling, please refer to [Error] for more information.
pub fn extract_metadata<'a>(input: &'a [u8]) -> Result<Vec<(String, IlstItemValue)>, Error> {
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
    let (remain, _) = travel_header(remain, |h, remain| {
        // println!("got: {} {}", h.box_type, h.box_size);
        if h.box_type == "moov" {
            // stop travelling
            false
        } else {
            if (remain.len() as u64) < h.body_size() {
                // stop travelling & skip unused box data
                to_skip = h.body_size() - remain.len() as u64;
                false
            } else {
                true
            }
        }
    })
    .map_err(|e| convert_error(e, "search atom moov failed"))?;

    let remain = if to_skip > 0 {
        return Err(Error::Skip(to_skip));
    } else {
        remain
    };

    let (_, entries) =
        parse_moov_body(remain).map_err(|e| convert_error(e, "parse moov failed"))?;
    Ok(entries)
}

fn check_mov(input: &[u8]) -> crate::Result<()> {
    let ftyp = get_ftyp(input)?;
    if ftyp != b"qt  " {
        Err(format!("unsupported MOV file; ftyp: {ftyp:?}").into())
    } else {
        Ok(())
    }
}

fn parse_moov_body(remain: &[u8]) -> IResult<&[u8], Vec<(String, IlstItemValue)>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use test_case::test_case;

    #[test_case("meta.mov")]
    fn mov_parse(path: &str) {
        let reader = open_sample(path).unwrap();
        let entries = parse_mov_metadata(reader).unwrap();
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

    #[test_case("meta.mov")]
    fn mov_extract_mov(path: &str) {
        let buf = read_sample(path).unwrap();
        println!("file size: {}", buf.len());
        let entries = extract_metadata(&buf).unwrap();
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

    #[test_case("embedded-in-heic.mov")]
    fn mov_extract_embedded(path: &str) {
        let buf = read_sample(path).unwrap();
        println!("file size: {}", buf.len());
        let entries = extract_metadata(&buf).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|x| format!("{x:?}"))
                .collect::<Vec<_>>()
                .join("\n"),
            "(\"com.apple.quicktime.location.accuracy.horizontal\", Text(\"14.235563\"))
(\"com.apple.quicktime.live-photo.auto\", U64(1))
(\"com.apple.quicktime.content.identifier\", Text(\"DA1A7EE8-0925-4C9F-9266-DDA3F0BB80F0\"))
(\"com.apple.quicktime.live-photo.vitality-score\", F64(0.9388400316238403))
(\"com.apple.quicktime.live-photo.vitality-scoring-version\", I64(4))
(\"com.apple.quicktime.location.ISO6709\", Text(\"+22.5797+113.9380+028.396/\"))
(\"com.apple.quicktime.make\", Text(\"Apple\"))
(\"com.apple.quicktime.model\", Text(\"iPhone 15 Pro\"))
(\"com.apple.quicktime.software\", Text(\"17.1\"))
(\"com.apple.quicktime.creationdate\", Text(\"2023-11-02T19:58:34+0800\"))"
        );
    }
}

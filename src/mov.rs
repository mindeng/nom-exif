use std::{
    collections::BTreeMap,
    io::{Read, Seek},
    ops::Range,
};

use chrono::DateTime;
use nom::{bytes::streaming, IResult};

use crate::{bbox::to_boxes, values::filter_zero};
#[allow(deprecated)]
use crate::{
    bbox::{
        find_box, parse_video_tkhd_in_moov, travel_header, IlstBox, KeysBox, MvhdBox, ParseBox,
    },
    error::ParsingError,
    loader::{BufLoader, Load},
    partial_vec::PartialVec,
    skip::Seekable,
    video::TrackInfoTag,
    EntryValue, FileFormat,
};

/// *Deprecated*: Please use [`MediaParser`] instead.
///
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
/// let entries = parse_metadata(f).unwrap();
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
#[deprecated(since = "2.0.0")]
#[tracing::instrument(skip_all)]
#[allow(deprecated)]
pub fn parse_metadata<R: Read + Seek>(reader: R) -> crate::Result<Vec<(String, EntryValue)>> {
    let mut loader = BufLoader::<Seekable, _>::new(reader);
    let ff = FileFormat::try_from_load(&mut loader)?;
    match ff {
        FileFormat::Jpeg | FileFormat::Heif => {
            return Err(crate::error::Error::ParseFailed(
                "can not parse metadata from an image".into(),
            ));
        }
        FileFormat::QuickTime | FileFormat::MP4 => (),
        FileFormat::Ebml => {
            return Err(crate::error::Error::ParseFailed(
                "please use MediaParser to parse *.webm, *.mkv files".into(),
            ))
        }
    };

    let moov_body = extract_moov_body(loader)?;

    let (_, mut entries) = match parse_moov_body(&moov_body) {
        Ok((remain, Some(entries))) => (remain, entries),
        Ok((remain, None)) => (remain, Vec::new()),
        Err(_) => {
            return Err("invalid moov body".into());
        }
    };

    let map: BTreeMap<TrackInfoTag, EntryValue> = convert_video_tags(entries.clone());
    let mut extras = parse_mvhd_tkhd(&moov_body);

    const CREATIONDATE_KEY: &str = "com.apple.quicktime.creationdate";
    if map.contains_key(&TrackInfoTag::CreateDate) {
        extras.remove(&TrackInfoTag::CreateDate);
        let date = map.get(&TrackInfoTag::CreateDate);
        if let Some(pos) = entries.iter().position(|x| x.0 == CREATIONDATE_KEY) {
            if let Some(date) = date {
                entries[pos] = (CREATIONDATE_KEY.to_string(), date.clone());
            } else {
                entries.remove(pos);
            }
        }
    }

    entries.extend(extras.into_iter().map(|(k, v)| match k {
        TrackInfoTag::ImageWidth => ("width".to_string(), v),
        TrackInfoTag::ImageHeight => ("height".to_string(), v),
        TrackInfoTag::DurationMs => (
            "duration".to_string(),
            // For compatibility with older versions, convert to u32
            EntryValue::U32(v.as_u64().unwrap() as u32),
        ),
        TrackInfoTag::CreateDate => (CREATIONDATE_KEY.to_string(), v),
        _ => unreachable!(),
    }));

    if map.contains_key(&TrackInfoTag::GpsIso6709) {
        const LOCATION_KEY: &str = "com.apple.quicktime.location.ISO6709";
        if let Some(idx) = entries.iter().position(|(k, _)| k == "udta.©xyz") {
            entries.remove(idx);
            entries.push((
                LOCATION_KEY.to_string(),
                map.get(&TrackInfoTag::GpsIso6709).unwrap().to_owned(),
            ));
        }
    }

    Ok(entries)
}

#[tracing::instrument(skip_all)]
pub(crate) fn parse_qt(
    moov_body: &[u8],
) -> Result<BTreeMap<TrackInfoTag, EntryValue>, ParsingError> {
    let (_, entries) = match parse_moov_body(moov_body) {
        Ok((remain, Some(entries))) => (remain, entries),
        Ok((remain, None)) => (remain, Vec::new()),
        Err(_) => {
            return Err("invalid moov body".into());
        }
    };

    let mut entries: BTreeMap<TrackInfoTag, EntryValue> = convert_video_tags(entries);
    let extras = parse_mvhd_tkhd(moov_body);
    if entries.contains_key(&TrackInfoTag::CreateDate) {
        entries.remove(&TrackInfoTag::CreateDate);
    }
    entries.extend(extras);

    Ok(entries)
}

#[tracing::instrument(skip_all)]
pub(crate) fn parse_mp4(
    moov_body: &[u8],
) -> Result<BTreeMap<TrackInfoTag, EntryValue>, ParsingError> {
    let (_, entries) = match parse_moov_body(moov_body) {
        Ok((remain, Some(entries))) => (remain, entries),
        Ok((remain, None)) => (remain, Vec::new()),
        Err(_) => {
            return Err("invalid moov body".into());
        }
    };

    let mut entries: BTreeMap<TrackInfoTag, EntryValue> = convert_video_tags(entries);
    let extras = parse_mvhd_tkhd(moov_body);
    entries.extend(extras);

    Ok(entries)
}

fn parse_mvhd_tkhd(moov_body: &[u8]) -> BTreeMap<TrackInfoTag, EntryValue> {
    let mut entries = BTreeMap::new();
    if let Ok((_, Some(bbox))) = find_box(moov_body, "mvhd") {
        if let Ok((_, mvhd)) = MvhdBox::parse_box(bbox.data) {
            entries.insert(TrackInfoTag::DurationMs, mvhd.duration_ms().into());

            entries.insert(
                TrackInfoTag::CreateDate,
                EntryValue::Time(mvhd.creation_time()),
            );
        }
    }

    if let Ok(Some(tkhd)) = parse_video_tkhd_in_moov(moov_body) {
        entries.insert(TrackInfoTag::ImageWidth, tkhd.width.into());
        entries.insert(TrackInfoTag::ImageHeight, tkhd.height.into());
    }

    entries
}

fn convert_video_tags(entries: Vec<(String, EntryValue)>) -> BTreeMap<TrackInfoTag, EntryValue> {
    entries
        .into_iter()
        .filter_map(|(k, v)| {
            if k == "com.apple.quicktime.creationdate" {
                v.as_str()
                    .and_then(|s| DateTime::parse_from_str(s, "%+").ok())
                    .map(|t| (TrackInfoTag::CreateDate, EntryValue::Time(t)))
            } else if k == "com.apple.quicktime.make" {
                Some((TrackInfoTag::Make, v))
            } else if k == "com.apple.quicktime.model" {
                Some((TrackInfoTag::Model, v))
            } else if k == "com.apple.quicktime.software" {
                Some((TrackInfoTag::Software, v))
            } else if k == "com.apple.quicktime.author" {
                Some((TrackInfoTag::Author, v))
            } else if k == "com.apple.quicktime.location.ISO6709" {
                Some((TrackInfoTag::GpsIso6709, v))
            } else if k == "udta.©xyz" {
                // For mp4 files, Android phones store GPS info in that box.
                v.as_u8array()
                    .and_then(parse_udta_gps)
                    .map(|v| (TrackInfoTag::GpsIso6709, EntryValue::Text(v)))
            } else if k == "udta.auth" {
                v.as_u8array()
                    .and_then(parse_udta_auth)
                    .map(|v| (TrackInfoTag::Author, EntryValue::Text(v)))
            } else if k.starts_with("udta.") {
                let tag = TryInto::<TrackInfoTag>::try_into(k.as_str()).ok();
                tag.map(|t| (t, v))
            } else {
                None
            }
        })
        .collect()
}

/// Try to find GPS info from box `moov/udta/©xyz`. For mp4 files, Android
/// phones store GPS info in that box.
// fn parse_mp4_gps(moov_body: &[u8]) -> Option<String> {
//     let bbox = match find_box(moov_body, "udta/©xyz") {
//         Ok((_, b)) => b,
//         Err(_) => None,
//     };
//     if let Some(bbox) = bbox {
//         return parse_udta_gps(bbox.body_data());
//     }
//     None
// }
fn parse_udta_gps(data: &[u8]) -> Option<String> {
    if data.len() <= 4 {
        tracing::warn!("moov/udta/©xyz body is too small");
        None
    } else {
        // The first 4 bytes is zero, skip them
        let location = data[4..] // Safe-slice
            .iter()
            .map(|b| *b as char)
            .collect::<String>();
        Some(location)
    }
}

const ISO_639_2_UND: [u8; 2] = [0x55, 0xc4];

fn parse_udta_auth(data: &[u8]) -> Option<String> {
    // Skip leading zero bytes
    let data = filter_zero(data);

    // Skip leading language flags.
    // Refer to: https://exiftool.org/forum/index.php?topic=11498.0
    if data.starts_with(&ISO_639_2_UND) {
        String::from_utf8(data.into_iter().skip(2).collect()).ok()
    } else {
        String::from_utf8(data).ok()
    }
}

/// *Deprecated*: Please use [`crate::MediaParser`] instead.
///
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
#[deprecated(since = "2.0.0")]
pub fn parse_mov_metadata<R: Read + Seek>(reader: R) -> crate::Result<Vec<(String, EntryValue)>> {
    #[allow(deprecated)]
    parse_metadata(reader)
}

#[tracing::instrument(skip_all)]
fn extract_moov_body<L: Load>(mut loader: L) -> Result<PartialVec, crate::Error> {
    let moov_body_range = loader.load_and_parse(extract_moov_body_from_buf)?;

    tracing::debug!(?moov_body_range);
    Ok(PartialVec::from_vec_range(
        loader.into_vec(),
        moov_body_range,
    ))
}

/// Parse the byte data of an ISOBMFF file and return the potential body data of
/// moov atom it may contain.
///
/// Regarding error handling, please refer to [Error] for more information.
#[tracing::instrument(skip_all)]
pub(crate) fn extract_moov_body_from_buf(input: &[u8]) -> Result<Range<usize>, ParsingError> {
    // parse metadata from moov/meta/keys & moov/meta/ilst
    let remain = input;

    let convert_error = |e: nom::Err<_>, msg: &str| match e {
        nom::Err::Incomplete(needed) => match needed {
            nom::Needed::Unknown => ParsingError::Need(1),
            nom::Needed::Size(n) => ParsingError::Need(n.get()),
        },
        nom::Err::Failure(_) | nom::Err::Error(_) => ParsingError::Failed(msg.to_string()),
    };

    let mut to_skip = 0;
    let mut skipped = 0;
    let (remain, header) = travel_header(remain, |h, remain| {
        tracing::debug!(?h.box_type, ?h.box_size, "Got");
        if h.box_type == "moov" {
            // stop travelling
            skipped += h.header_size;
            false
        } else if (remain.len() as u64) < h.body_size() {
            // stop travelling & skip unused box data
            to_skip = h.body_size() as usize - remain.len();
            false
        } else {
            // body has been read, so just consume it
            skipped += h.box_size as usize;
            true
        }
    })
    .map_err(|e| convert_error(e, "search atom moov failed"))?;

    if to_skip > 0 {
        return Err(ParsingError::ClearAndSkip(
            to_skip
                .checked_add(input.len())
                .ok_or_else(|| ParsingError::Failed("to_skip is too big".into()))?,
        ));
    }

    let size: usize = header.body_size().try_into().expect("must fit");
    let (_, body) =
        streaming::take(size)(remain).map_err(|e| convert_error(e, "moov is too small"))?;

    Ok(skipped..skipped + body.len())
}

type EntriesResult<'a> = IResult<&'a [u8], Option<Vec<(String, EntryValue)>>>;

#[tracing::instrument(skip(input))]
fn parse_moov_body(input: &[u8]) -> EntriesResult {
    tracing::debug!("parse_moov_body");

    let mut entries = parse_meta(input).unwrap_or_default();

    if let Ok((_, Some(udta))) = find_box(input, "udta") {
        tracing::debug!("udta");
        if let Ok(boxes) = to_boxes(udta.body_data()) {
            for entry in boxes.iter() {
                tracing::debug!(?entry, "udta entry");
                entries.push((
                    format!("udta.{}", entry.box_type()),
                    EntryValue::U8Array(Vec::from(entry.body_data())),
                ));
            }
        }
    }

    Ok((input, Some(entries)))
}

fn parse_meta(input: &[u8]) -> Option<Vec<(String, EntryValue)>> {
    let (_, Some(meta)) = find_box(input, "meta").ok()? else {
        return None;
    };

    let (_, Some(keys)) = find_box(meta.body_data(), "keys").ok()? else {
        return None;
    };

    let (_, Some(ilst)) = find_box(meta.body_data(), "ilst").ok()? else {
        return None;
    };

    let (_, keys) = KeysBox::parse_box(keys.data).ok()?;
    let (_, ilst) = IlstBox::parse_box(ilst.data).ok()?;

    let entries = keys
        .entries
        .into_iter()
        .map(|k| k.key)
        .zip(ilst.items.into_iter().map(|v| v.value))
        .collect::<Vec<_>>();

    Some(entries)
}

/// Change timezone format from iso 8601 to rfc3339, e.g.:
///
/// - `2023-11-02T19:58:34+08` -> `2023-11-02T19:58:34+08:00`
/// - `2023-11-02T19:58:34+0800` -> `2023-11-02T19:58:34+08:00`
#[allow(dead_code)]
fn tz_iso_8601_to_rfc3339(s: String) -> String {
    use regex::Regex;

    let ss = s.trim();
    // Safe unwrap
    let re = Regex::new(r"([+-][0-9][0-9])([0-9][0-9])?$").unwrap();

    if let Some((offset, tz)) = re.captures(ss).map(|caps| {
        (
            // Safe unwrap
            caps.get(1).unwrap().start(),
            format!(
                "{}:{}",
                caps.get(1).map_or("00", |m| m.as_str()),
                caps.get(2).map_or("00", |m| m.as_str())
            ),
        )
    }) {
        let s1 = &ss.as_bytes()[..offset]; // Safe-slice
        let s2 = tz.as_bytes();
        s1.iter().chain(s2.iter()).map(|x| *x as char).collect()
    } else {
        s
    }
}

#[cfg(test)]
#[allow(deprecated)]
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
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        tracing::info!(bytes = buf.len(), "File size.");
        let range = extract_moov_body_from_buf(&buf).unwrap();
        let (_, entries) = parse_moov_body(&buf[range]).unwrap();
        assert_eq!(
            entries
                .unwrap()
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
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let entries = parse_metadata(open_sample(path).unwrap()).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|x| format!("{x:?}"))
                .collect::<Vec<_>>()
                .join("\n"),
            "(\"com.apple.quicktime.creationdate\", Time(2024-02-03T07:05:38+00:00))
(\"duration\", U32(1063))
(\"width\", U32(1920))
(\"height\", U32(1080))
(\"com.apple.quicktime.location.ISO6709\", Text(\"+27.2939+112.6932/\"))"
        );
    }

    #[test_case("embedded-in-heic.mov")]
    fn parse_embedded_mov(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

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
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

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

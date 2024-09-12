use std::{
    collections::{btree_map::IntoIter, BTreeMap},
    fmt::Display,
    io::Read,
};

use crate::{
    ebml::webm::parse_webm,
    error::ParsingError,
    file::{Mime, MimeVideo},
    loader::{BufLoader, Load},
    mov::{extract_moov_body_from_buf, parse_mp4, parse_qt},
    skip::Skip,
    EntryValue, FileFormat, GPSInfo,
};

/// Try to keep the tag name consistent with [`crate::ExifTag`], and add some
/// unique to video/audio, such as `DurationMs`.
///
/// Different variants of `TrackInfoTag` may have different value types, please
/// refer to the documentation of each variant.
#[derive(Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash)]
pub enum TrackInfoTag {
    /// [`EntryValue::Text`]
    Make,

    /// [`EntryValue::Text`]
    Model,

    /// [`EntryValue::Text`]
    Software,

    /// [`EntryValue::Time`]
    CreateDate,

    /// Duration in millisecond, it's an [`EntryValue::U64`]
    DurationMs,

    /// [`EntryValue::U32`]
    ImageWidth,

    /// [`EntryValue::U32`]
    ImageHeight,

    /// [`EntryValue::Text`], Location presented in ISO6709. If you need a
    /// parsed [`GPSInfo`] which provides more detailed GPS info, please use
    /// [`TrackInfo::get_gps_info`].
    GpsIso6709,
}

/// Parse video/audio info from `reader`. The file format will be detected
/// automatically by parser, if the format is not supported, an `Err` will be
/// returned.
///
/// Currently supported file formats are:
///
/// - ISO base media file format (ISOBMFF): *.mp4, *.mov, *.3gp, etc.
/// - Matroska based file format: *.webm, *.mkv, *.mka, etc.
///
/// ## Explanation of the generic parameters of this function:
///
/// - In order to improve parsing efficiency, the parser will internally skip
///   some useless bytes during parsing the byte stream, which is called
///   [`Skip`] internally.
///
/// - In order to support both `Read` and `Read` + `Seek` types, the interface
///   of input parameters is defined as `Read`.
///   
/// - Since Rust does not support specialization, the parser cannot internally
///   distinguish between `Read` and `Seek` and provide different `Skip`
///   implementations for them.
///   
/// Therefore, We chose to let the user specify how `Skip` works:
///
/// - `parse_track_info::<SkipSeek, _>(reader)` means the `reader` supports
///   `Seek`, so `Skip` will use the `Seek` trait to implement efficient skip
///   operations.
///   
/// - `parse_track_info::<SkipRead, _>(reader)` means the `reader` dosn't
///   support `Seek`, so `Skip` will fall back to using `Read` to implement the
///   skip operations.
///
/// ## Performance impact
///
/// If your `reader` only supports `Read`, it may cause performance loss when
/// processing certain large files. For example, *.mov files place metadata at
/// the end of the file, therefore, when parsing such files, locating metadata
/// will be slightly slower.
///
/// ## Examples
///
/// ```rust
/// use nom_exif::*;
/// use std::fs::File;
/// use chrono::DateTime;
///
/// let ms = MediaSource::file_path("./testdata/meta.mov").unwrap();
/// let mut parser = MediaParser::new();
/// let info: TrackInfo = parser.parse(ms).unwrap();
///
/// assert_eq!(info.get(TrackInfoTag::Make), Some(&"Apple".into()));
/// assert_eq!(info.get(TrackInfoTag::Model), Some(&"iPhone X".into()));
/// assert_eq!(info.get(TrackInfoTag::GpsIso6709), Some(&"+27.1281+100.2508+000.000/".into()));
/// assert_eq!(info.get_gps_info().unwrap().latitude_ref, 'N');
/// assert_eq!(
///     info.get_gps_info().unwrap().latitude,
///     [(27, 1), (7, 1), (68, 100)].into(),
/// );
/// ```
pub(crate) fn parse_track_info(
    input: &[u8],
    mime_video: MimeVideo,
) -> Result<TrackInfo, ParsingError> {
    let mut info: TrackInfo = match mime_video {
        crate::file::MimeVideo::QuickTime
        | crate::file::MimeVideo::_3gpp
        | crate::file::MimeVideo::Mp4 => {
            let range = extract_moov_body_from_buf(input)?;
            let moov_body = &input[range];

            match mime_video {
                MimeVideo::QuickTime => parse_qt(moov_body)?.into(),

                MimeVideo::Mp4 | MimeVideo::_3gpp => parse_mp4(moov_body)?.into(),
                _ => unreachable!(),
            }
        }
        crate::file::MimeVideo::Webm | crate::file::MimeVideo::Matroska => {
            parse_webm(input)?.into()
        }
    };

    if let Some(gps) = info.get(TrackInfoTag::GpsIso6709) {
        info.gps_info = gps.as_str().and_then(|s| s.parse().ok());
    }

    Ok(info)
}

#[derive(Debug, Clone, Default)]
pub struct TrackInfo {
    entries: BTreeMap<TrackInfoTag, EntryValue>,
    gps_info: Option<GPSInfo>,
}

impl TrackInfo {
    /// Get value for `tag`. Different variants of `TrackInfoTag` may have
    /// different value types, please refer to [`TrackInfoTag`].
    pub fn get(&self, tag: TrackInfoTag) -> Option<&EntryValue> {
        self.entries.get(&tag)
    }

    /// Get parsed `GPSInfo`.
    pub fn get_gps_info(&self) -> Option<&GPSInfo> {
        self.gps_info.as_ref()
    }

    /// Get an iterator for `(&TrackInfoTag, &EntryValue)`. The parsed
    /// `GPSInfo` is not included.
    pub fn iter(&self) -> impl Iterator<Item = (&TrackInfoTag, &EntryValue)> {
        self.entries.iter()
    }

    pub(crate) fn put(&mut self, tag: TrackInfoTag, value: EntryValue) {
        self.entries.insert(tag, value);
    }
}

impl IntoIterator for TrackInfo {
    type Item = (TrackInfoTag, EntryValue);
    type IntoIter = IntoIter<TrackInfoTag, EntryValue>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl From<BTreeMap<TrackInfoTag, EntryValue>> for TrackInfo {
    fn from(entries: BTreeMap<TrackInfoTag, EntryValue>) -> Self {
        Self {
            entries,
            gps_info: None,
        }
    }
}

impl Display for TrackInfoTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &str = (*self).into();
        s.fmt(f)
    }
}

impl From<TrackInfoTag> for &str {
    fn from(value: TrackInfoTag) -> Self {
        match value {
            TrackInfoTag::Make => "Make",
            TrackInfoTag::Model => "Model",
            TrackInfoTag::Software => "Software",
            TrackInfoTag::CreateDate => "CreateDate",
            TrackInfoTag::DurationMs => "Duration",
            TrackInfoTag::ImageWidth => "ImageWidth",
            TrackInfoTag::ImageHeight => "ImageHeight",
            TrackInfoTag::GpsIso6709 => "GpsIso6709",
        }
    }
}

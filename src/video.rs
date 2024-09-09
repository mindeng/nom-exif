use std::{
    collections::{btree_map::IntoIter, BTreeMap},
    fmt::Display,
    io::Read,
};

use crate::{
    ebml::webm::parse_webm,
    loader::{BufLoader, Load},
    mov::{parse_mp4, parse_qt},
    skip::Skip,
    EntryValue, FileFormat, GPSInfo,
};

/// Try to keep the tag name consistent with [`crate::ExifTag`], and add some
/// unique to video/audio, such as `Duration`
#[derive(Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord)]
pub enum TrackInfoTag {
    Make,
    Model,
    Software,
    CreateDate,
    Duration,
    ImageWidth,
    ImageHeight,
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
/// let f = File::open("./testdata/meta.mov").unwrap();
/// let info = parse_track_info::<SkipSeek, _>(f).unwrap();
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
pub fn parse_track_info<S: Skip<R>, R: Read>(reader: R) -> crate::Result<TrackInfo> {
    let mut loader = BufLoader::<S, _>::new(reader);
    let ff = FileFormat::try_from_load(&mut loader)?;
    let mut info: TrackInfo = match ff {
        FileFormat::Jpeg | FileFormat::Heif => {
            return Err(crate::error::Error::ParseFailed(
                "can not parse video info from an image".into(),
            ));
        }
        FileFormat::QuickTime => parse_qt(loader)?.into(),
        FileFormat::MP4 => parse_mp4(loader)?.into(),
        FileFormat::Ebml => parse_webm(loader)?.into(),
    };

    if let Some(gps) = info.get(TrackInfoTag::GpsIso6709) {
        info.gps_info = gps.as_str().and_then(|s| s.parse().ok());
    }

    Ok(info)
}

pub fn parse_track_from_loader<L: Load>(mut loader: L) -> crate::Result<TrackInfo> {
    let ff = FileFormat::try_from_load(&mut loader)?;
    let mut info: TrackInfo = match ff {
        FileFormat::Jpeg | FileFormat::Heif => {
            return Err(crate::error::Error::ParseFailed(
                "can not parse video info from an image".into(),
            ));
        }
        FileFormat::QuickTime => parse_qt(loader)?.into(),
        FileFormat::MP4 => parse_mp4(loader)?.into(),
        FileFormat::Ebml => parse_webm(loader)?.into(),
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
    pub fn get(&self, tag: TrackInfoTag) -> Option<&EntryValue> {
        self.entries.get(&tag)
    }

    pub fn get_gps_info(&self) -> Option<&GPSInfo> {
        self.gps_info.as_ref()
    }

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
            TrackInfoTag::Duration => "Duration",
            TrackInfoTag::ImageWidth => "ImageWidth",
            TrackInfoTag::ImageHeight => "ImageHeight",
            TrackInfoTag::GpsIso6709 => "GpsIso6709",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testkit::open_sample;
    use crate::values::Rational;
    use crate::{LatLng, Seekable, SkipRead};
    use chrono::DateTime;
    use test_case::test_case;

    use super::TrackInfoTag::*;
    use super::*;

    #[test_case("mkv_640x360.mkv", ImageWidth, 640_u32.into())]
    #[test_case("mkv_640x360.mkv", ImageHeight, 360_u32.into())]
    #[test_case("mkv_640x360.mkv", Duration, 13346_f64.into())]
    #[test_case("mkv_640x360.mkv", CreateDate, DateTime::parse_from_str("2008-08-08T08:08:08Z", "%+").unwrap().into())]
    fn test_skip_seek(path: &str, tag: TrackInfoTag, v: EntryValue) {
        let info = parse_track_info::<Seekable, _>(open_sample(path).unwrap()).unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);
    }

    #[test_case("meta.mov", Make, "Apple".into())]
    #[test_case("meta.mov", Model, "iPhone X".into())]
    #[test_case("meta.mov", GpsIso6709, "+27.1281+100.2508+000.000/".into())]
    fn test_skip_read(path: &str, tag: TrackInfoTag, v: EntryValue) {
        let info = parse_track_info::<SkipRead, _>(open_sample(path).unwrap()).unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);
        assert_eq!(
            info.get_gps_info().unwrap().latitude,
            LatLng(Rational(27, 1), Rational(7, 1), Rational(68, 100))
        );
    }
}

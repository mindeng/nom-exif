use std::{
    collections::BTreeMap,
    fmt::Display,
    io::{Read, Seek},
    str::FromStr,
};

use crate::{
    ebml::webm::parse_webm,
    loader::BufLoader,
    mov::{parse_mp4, parse_qt},
    skip::SkipSeek,
    EntryValue, FileFormat, GPSInfo,
};

/// Try to keep the tag name consistent with [`crate::ExifTag`], and add some
/// unique to video, such as `Duration`
#[derive(Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord)]
pub(crate) enum VideoInfoTag {
    Make,
    Model,
    Software,
    CreateDate,
    Duration,
    ImageWidth,
    ImageHeight,
    GpsIso6709,
}

#[derive(Debug, Clone, Default)]
pub struct VideoInfo {
    entries: BTreeMap<VideoInfoTag, EntryValue>,
    gps_info: Option<GPSInfo>,
}

impl VideoInfo {
    pub fn get_value_by_name(&self, name: &str) -> Option<&EntryValue> {
        let t: VideoInfoTag = name.parse().ok()?;
        self.entries.get(&t)
    }

    pub fn get_gps_info(&self) -> Option<&GPSInfo> {
        self.gps_info.as_ref()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &EntryValue)> {
        self.entries
            .iter()
            .map(|(k, v)| (Into::<&str>::into(*k), v))
    }

    pub fn into_iter(self) -> impl Iterator<Item = (&'static str, EntryValue)> {
        self.entries
            .into_iter()
            .map(|(k, v)| (Into::<&str>::into(k), v))
    }

    pub(crate) fn put(&mut self, tag: VideoInfoTag, value: EntryValue) {
        self.entries.insert(tag, value);
    }
}

pub fn parse_video_info<R: Read + Seek>(mut reader: R) -> crate::Result<VideoInfo> {
    reader.rewind()?;
    let mut loader = BufLoader::<SkipSeek, _>::new(reader);
    let ff = FileFormat::try_from_load(&mut loader)?;
    let mut info: VideoInfo = match ff {
        FileFormat::Jpeg | FileFormat::Heif => {
            return Err(crate::error::Error::ParseFailed(
                "can not parse video info from an image".into(),
            ));
        }
        FileFormat::QuickTime => parse_qt(loader)?.into(),
        FileFormat::MP4 => parse_mp4(loader)?.into(),
        FileFormat::Ebml => parse_webm(loader)?.into(),
    };

    if let Some(gps) = info.get_value_by_name(VideoInfoTag::GpsIso6709.into()) {
        info.gps_info = gps.as_str().and_then(|s| s.parse().ok());
    }

    Ok(info)
}

impl From<BTreeMap<VideoInfoTag, EntryValue>> for VideoInfo {
    fn from(entries: BTreeMap<VideoInfoTag, EntryValue>) -> Self {
        Self {
            entries,
            gps_info: None,
        }
    }
}

pub(crate) struct UnsupportedTagError;

impl FromStr for VideoInfoTag {
    type Err = UnsupportedTagError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = match s {
            "Make" => Self::Make,
            "Model" => Self::Model,
            "Software" => Self::Software,
            "CreateDate" => Self::CreateDate,
            "Duration" => Self::Duration,
            "ImageWidth" => Self::ImageWidth,
            "ImageHeight" => Self::ImageHeight,
            "GPSInfo" => Self::GpsIso6709,
            _ => return Err(UnsupportedTagError),
        };
        Ok(t)
    }
}

impl Display for VideoInfoTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &str = (*self).into();
        s.fmt(f)
    }
}

impl From<VideoInfoTag> for &str {
    fn from(value: VideoInfoTag) -> Self {
        match value {
            VideoInfoTag::Make => "Make",
            VideoInfoTag::Model => "Model",
            VideoInfoTag::Software => "Software",
            VideoInfoTag::CreateDate => "CreateDate",
            VideoInfoTag::Duration => "Duration",
            VideoInfoTag::ImageWidth => "ImageWidth",
            VideoInfoTag::ImageHeight => "ImageHeight",
            VideoInfoTag::GpsIso6709 => "GpsIso6709",
        }
    }
}

use std::{
    collections::BTreeMap,
    io::{Read, Seek},
};

use crate::{
    ebml::webm::parse_webm,
    loader::SeekBufLoader,
    mov::{parse_mp4, parse_qt},
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
    GPSInfo,
}

#[derive(Debug, Clone, Default)]
pub struct VideoInfo {
    entries: BTreeMap<VideoInfoTag, EntryValue>,
    gps_info: Option<GPSInfo>,
}

impl VideoInfo {
    pub(crate) fn put(&mut self, tag: VideoInfoTag, value: EntryValue) {
        self.entries.insert(tag, value);
    }
}

pub fn parse_video_info<R: Read + Seek>(reader: R) -> crate::Result<VideoInfo> {
    let mut loader = SeekBufLoader::new(reader);
    let ff = FileFormat::try_from_load(&mut loader)?;
    let info = match ff {
        FileFormat::Jpeg | FileFormat::Heif => {
            return Err(crate::error::Error::ParseFailed(
                "can not parse video info from an image".into(),
            ));
        }
        FileFormat::QuickTime => parse_qt(loader)?.into(),
        FileFormat::MP4 => parse_mp4(loader)?.into(),
        FileFormat::Ebml => parse_webm(loader)?.into(),
    };

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

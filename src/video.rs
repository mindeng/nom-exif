use std::{
    collections::HashMap,
    io::{Read, Seek},
};

use crate::{ebml::webm::parse_webm, loader::SeekBufLoader, EntryValue, GPSInfo};

/// Try to keep the tag name consistent with [`crate::ExifTag`], and add some
/// unique to video, such as `Duration`
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum VideoInfoTag {
    Make,
    Model,
    CreateDate,
    ImageWidth,
    ImageHeight,
    Duration,
}

#[derive(Debug, Clone, Default)]
pub struct VideoInfo {
    entries: HashMap<VideoInfoTag, EntryValue>,
    gps_info: Option<GPSInfo>,
}

pub fn parse_video_info<R: Read + Seek>(reader: R) -> crate::Result<VideoInfo> {
    let loader = SeekBufLoader::new(reader);
    parse_webm(loader)?;
    Ok(VideoInfo::default())
}

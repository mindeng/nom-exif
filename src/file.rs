use std::fmt::Display;

#[allow(unused)]
#[derive(Debug, PartialEq, Eq)]
pub enum FileType {
    JPEG,
    HEIF,
    QuickTime,
    MP4,
}

use FileType::*;

impl Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JPEG => "JPEG".fmt(f),
            HEIF => "HEIF/HEIC".fmt(f),
            QuickTime => "QuickTime".fmt(f),
            MP4 => "MP4".fmt(f),
        }
    }
}

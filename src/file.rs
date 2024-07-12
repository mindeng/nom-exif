use std::fmt::Display;

#[allow(unused)]
#[derive(Debug, PartialEq, Eq)]
pub enum FileType {
    Jpeg,
    Heif,
    QuickTime,
    MP4,
}

use FileType::*;

impl Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Jpeg => "JPEG".fmt(f),
            Heif => "HEIF/HEIC".fmt(f),
            QuickTime => "QuickTime".fmt(f),
            MP4 => "MP4".fmt(f),
        }
    }
}

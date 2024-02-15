use std::fmt::Display;

#[allow(unused)]
#[derive(Debug)]
pub enum FileType {
    QuickTime,
    HEIF,
    JPEG,
}

use FileType::*;

impl Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuickTime => "QuickTime".fmt(f),
            HEIF => "HEIF/HEIC".fmt(f),
            JPEG => "JPEG".fmt(f),
        }
    }
}

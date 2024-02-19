mod bbox;
mod error;
mod exif;
mod file;
mod heif;
mod jpeg;
mod mov;

pub use heif::parse_heif_exif;
pub use jpeg::parse_jpeg_exif;
pub use mov::{parse_metadata, parse_mov_metadata};

pub use exif::{ExifTag, IfdEntryValue};

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod testkit;

/// Parse Exif for JPEG/HEIC/HEIF files.
pub fn parse_exif(path: &str) -> crate::Result<Option<exif::Exif>> {
    use std::{ffi::OsStr, fs::File, path::Path};
    let Some(extension) = Path::new(path).extension().and_then(OsStr::to_str) else {
        return Err("unsupported filetype: filename extension is empty".into());
    };

    let extension = extension.to_lowercase();
    let reader = File::open(path)?;
    match extension.as_ref() {
        "jpg" | "jpeg" => parse_jpeg_exif(reader),
        "heic" | "heif" => parse_heif_exif(reader),
        o => Err(format!("unsupported filetype: {o}").into()),
    }
}

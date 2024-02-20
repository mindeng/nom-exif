mod bbox;
mod error;
mod exif;
mod file;
mod heif;
mod jpeg;
mod mov;
mod values;

pub use heif::parse_heif_exif;
pub use jpeg::parse_jpeg_exif;
pub use mov::{parse_metadata, parse_mov_metadata};

pub use exif::ExifTag;
pub use values::EntryValue;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod testkit;

/// Parse Exif for JPEG/HEIC/HEIF files.
///
/// # Usage
///
/// ```rust
/// use nom_exif::*;
/// use nom_exif::ExifTag::*;
///
/// let exif = parse_exif("./testdata/exif.heic").unwrap().unwrap();
///
/// assert_eq!(exif.get_value(&Make).unwrap().unwrap().to_string(), "Apple");
///
/// assert_eq!(
///     exif.get_values(&[DateTimeOriginal, CreateDate, ModifyDate])
///         .into_iter()
///         .map(|x| (x.0.to_string(), x.1.to_string()))
///         .collect::<Vec<_>>(),
///     [
///         ("DateTimeOriginal(0x9003)", "2022-07-22T21:26:32+08:00"),
///         ("CreateDate(0x9004)", "2022-07-22T21:26:32+08:00"),
///         ("ModifyDate(0x0132)", "2022-07-22T21:26:32+08:00")
///     ]
///     .into_iter()
///     .map(|x| (x.0.to_string(), x.1.to_string()))
///     .collect::<Vec<_>>()
/// );
/// ```
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

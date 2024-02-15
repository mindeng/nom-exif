mod gps;
use gps::{GPSInfo, LatLng};

mod tags;
use nom::bytes::complete;
pub use tags::ExifTag;

mod value;
pub use value::IfdEntryValue;

mod parser;
pub use parser::{parse_exif, Exif};
use parser::{DirectoryEntry, ImageFileDirectory};

pub(crate) const EXIF_IDENT: &str = "Exif\0\0";

pub fn check_exif_header(data: &[u8]) -> bool {
    if data.len() < 6 {
        return false;
    }

    complete::tag::<_, _, nom::error::Error<_>>(EXIF_IDENT)(data).is_ok()
}

mod gps;
pub use gps::{GPSInfo, LatLng};

mod tags;
use nom::bytes::complete;
pub use tags::ExifTag;

pub(crate) mod ifd;

mod parser;
pub use parser::{parse_exif, Exif};

mod exif_ext;

pub fn check_exif_header(data: &[u8]) -> bool {
    assert!(data.len() >= 6);

    const EXIF_IDENT: &str = "Exif\0\0";
    complete::tag::<_, _, nom::error::Error<_>>(EXIF_IDENT)(data).is_ok()
}

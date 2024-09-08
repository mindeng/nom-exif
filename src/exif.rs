pub use exif_iter::{ExifIter, ParsedExifEntry};
pub use gps::{GPSInfo, LatLng};
pub use parser::Exif;
pub use tags::ExifTag;

pub(crate) mod ifd;
pub(crate) use io::read_exif;
pub(crate) use parser::{check_exif_header, input_to_exif, input_to_iter};

mod exif_iter;
mod gps;
mod io;
mod parser;
mod tags;

use crate::file::FileFormat;
use std::io::Read;

/// Read exif data from `reader`, and build an [`ExifIter`] for it.
///
/// If `format` is None, then guess the file format based on the read content.
///
/// All entries are lazy-parsed. That is, only when you iterate over
/// [`ExifIter`] will the IFD entries be parsed one by one.
///
/// The one exception is the time zone entries. The parser will try to find and
/// parse the time zone data first, so we can correctly parse all time
/// information in subsequent iterates.
///
/// Please note that the parsing routine itself provides a buffer, so the
/// `reader` may not need to be wrapped with `BufRead`.
///
/// Returns:
///
/// - An `Ok<Some<ExifIter>>` if Exif data is found and parsed successfully.
/// - An `Ok<None>` if Exif data is not found.
/// - An `Err` if Exif data is found but parsing failed.
pub fn parse_exif<T: Read>(
    reader: T,
    format: Option<FileFormat>,
) -> crate::Result<Option<ExifIter<'static>>> {
    read_exif(reader, format)?.map(input_to_iter).transpose()
}

#[cfg(feature = "async")]
use tokio::io::AsyncRead;

/// `async` version of [`parse_exif`].
#[cfg(feature = "async")]
pub async fn parse_exif_async<T: AsyncRead + Unpin>(
    reader: T,
    format: Option<FileFormat>,
) -> crate::Result<Option<ExifIter<'static>>> {
    use io::read_exif_async;
    read_exif_async(reader, format)
        .await?
        .map(input_to_iter)
        .transpose()
}

#[cfg(test)]
mod tests {
    use crate::testkit::open_sample;
    use test_case::test_case;

    use super::*;

    #[test_case("exif.heic", "+43.29013+084.22713+1595.950CRSWGS_84/")]
    #[test_case("exif.jpg", "+22.53113+114.02148/")]
    fn gps(path: &str, gps_str: &str) {
        let f = open_sample(path).unwrap();
        let iter = parse_exif(f, None).unwrap().unwrap();
        let gps_info = iter.parse_gps_info().unwrap().unwrap();
        assert_eq!(gps_info.format_iso6709(), gps_str);
    }

    #[test_case("exif.heic")]
    fn tag_values(path: &str) {
        let f = open_sample(path).unwrap();
        let iter = parse_exif(f, None).unwrap().unwrap();
        let tags = [ExifTag::Make, ExifTag::Model];
        let res: Vec<String> = iter
            .clone()
            .filter(|e| e.tag().is_some_and(|t| tags.contains(&t)))
            .filter(|e| e.has_value())
            .map(|e| format!("{} => {}", e.tag().unwrap(), e.take_value().unwrap()))
            .collect();
        assert_eq!(
            res.join(", "),
            "Make(0x010f) => Apple, Model(0x0110) => iPhone 12 Pro"
        );
    }
}

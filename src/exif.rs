use crate::error::ParsingError;
use crate::loader::{BufLoad, BufLoader, Load};
use crate::skip::Unseekable;
use crate::slice::SubsliceRange;
use crate::{input::Input, FileFormat};
pub use exif_iter::{ExifIter, ParsedExifEntry};
pub use gps::{GPSInfo, LatLng};
pub use parser::Exif;
pub use tags::ExifTag;

use std::io::Read;

pub(crate) mod ifd;
pub(crate) use parser::{check_exif_header, input_to_exif, input_to_iter, ExifParser, TiffHeader};

mod exif_iter;
mod gps;
mod parser;
mod tags;

/// *Deprecated*: Please use [`crate::MediaParser`] instead.
///
/// Read exif data from `reader`, and build an [`ExifIter`] for it.
///
/// If `format` is None, the parser will detect the file format automatically.
///
/// Currently supported file formats are:
///
/// - *.heic, *.heif, etc.
/// - *.jpg, *.jpeg, etc.
/// - *.tiff
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
#[deprecated(since = "2.0.0")]
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
    read_exif_async(reader, format)
        .await?
        .map(input_to_iter)
        .transpose()
}

/// Read exif data from `reader`, if `format` is None, the parser will detect
/// the file format automatically.
#[tracing::instrument(skip(read))]
pub(crate) fn read_exif<R: Read>(
    read: R,
    format: Option<FileFormat>,
) -> crate::Result<Option<Input<'static>>> {
    let mut loader = BufLoader::<Unseekable, R>::new(read);
    let ff = match format {
        Some(ff) => ff,
        None => loader.load_and_parse(|x| {
            x.try_into()
                .map_err(|_| ParsingError::Failed("unrecognized file format".to_string()))
        })?,
    };

    let exif_data = loader.load_and_parse(|buf| match ff.extract_exif_data(buf) {
        Ok((_, data)) => Ok(data.and_then(|x| buf.subslice_range(x))),
        Err(e) => Err(e.into()),
    })?;

    Ok(exif_data.map(|x| Input::from_vec_range(loader.into_vec(), x)))
}

/// Read exif data from `reader`, if `format` is None, then guess the file
/// format based on the read content.
#[cfg(feature = "async")]
#[tracing::instrument(skip(read))]
pub(crate) async fn read_exif_async<T>(
    read: T,
    format: Option<FileFormat>,
) -> crate::Result<Option<Input<'static>>>
where
    T: AsyncRead + std::marker::Unpin,
{
    use crate::loader::{AsyncBufLoader, AsyncLoad};

    let mut loader = AsyncBufLoader::<Unseekable, _>::new(read);
    let ff = match format {
        Some(ff) => ff,
        None => {
            loader
                .load_and_parse(|x| {
                    x.try_into()
                        .map_err(|_| ParsingError::Failed("unrecognized file format".to_string()))
                })
                .await?
        }
    };

    let exif_data = loader
        .load_and_parse(|buf| match ff.extract_exif_data(buf) {
            Ok((_, data)) => Ok(data.and_then(|x| buf.subslice_range(x))),
            Err(e) => Err(e.into()),
        })
        .await?;

    Ok(exif_data.map(|x| Input::from_vec_range(loader.into_vec(), x)))
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
            .map(|e| format!("{} => {}", e.tag().unwrap(), e.get_value().unwrap()))
            .collect();
        assert_eq!(res.join(", "), "Make => Apple, Model => iPhone 12 Pro");
    }
}

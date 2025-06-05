use crate::error::{nom_error_to_parsing_error_with_state, ParsingError, ParsingErrorState};
use crate::file::MimeImage;
use crate::parser::{BufParser, ParsingState, ShareBuf};
use crate::raf::RafInfo;
use crate::skip::Skip;
use crate::slice::SubsliceRange;
use crate::{heif, jpeg, MediaParser, MediaSource};
#[allow(deprecated)]
use crate::{partial_vec::PartialVec, FileFormat};
use exif_exif::check_exif_header2;
pub use exif_exif::Exif;
use exif_iter::input_into_iter;
pub use exif_iter::{ExifIter, ParsedExifEntry};
pub use gps::{GPSInfo, LatLng};
pub use tags::ExifTag;

use std::io::Read;
use std::ops::Range;

pub(crate) mod ifd;
pub(crate) use exif_exif::{check_exif_header, TiffHeader};
pub(crate) use travel::IfdHeaderTravel;

mod exif_exif;
mod exif_iter;
mod gps;
mod tags;
mod travel;

/// *Deprecated*: Please use [`crate::MediaParser`] instead.
///
/// Read exif data from `reader`, and build an [`ExifIter`] for it.
///
/// ~~If `format` is None, the parser will detect the file format automatically.~~
/// *The `format` param will be ignored from v2.0.0.*
///
/// Currently supported file formats are:
///
/// - *.heic, *.heif, etc.
/// - *.jpg, *.jpeg, etc.
///
/// *.tiff/*.tif is not supported by this function, please use `MediaParser`
/// instead.
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
#[allow(deprecated)]
pub fn parse_exif<T: Read>(reader: T, _: Option<FileFormat>) -> crate::Result<Option<ExifIter>> {
    let mut parser = MediaParser::new();
    let iter: ExifIter = parser.parse(MediaSource::unseekable(reader)?)?;
    let iter = iter.to_owned();
    Ok(Some(iter))
}

#[tracing::instrument(skip(reader))]
pub(crate) fn parse_exif_iter<R: Read, S: Skip<R>>(
    parser: &mut MediaParser,
    mime_img: MimeImage,
    reader: &mut R,
) -> Result<ExifIter, crate::Error> {
    let out = parser.load_and_parse::<R, S, _, _>(reader, |buf, state| {
        extract_exif_range(mime_img, buf, state)
    })?;

    range_to_iter(parser, out)
}

type ExifRangeResult = Result<Option<(Range<usize>, Option<TiffHeader>)>, ParsingErrorState>;

fn extract_exif_range(img: MimeImage, buf: &[u8], state: Option<ParsingState>) -> ExifRangeResult {
    let (exif_data, state) = extract_exif_with_mime(img, buf, state)?;
    let header = state.and_then(|x| match x {
        ParsingState::TiffHeader(h) => Some(h),
        ParsingState::HeifExifSize(_) => None,
    });
    Ok(exif_data
        .and_then(|x| buf.subslice_in_range(x))
        .map(|x| (x, header)))
}

fn range_to_iter(
    parser: &mut impl ShareBuf,
    out: Option<(Range<usize>, Option<TiffHeader>)>,
) -> Result<ExifIter, crate::Error> {
    if let Some((range, header)) = out {
        tracing::debug!(?range, ?header, "Got Exif data");
        let input: PartialVec = parser.share_buf(range);
        let iter = input_into_iter(input, header)?;

        Ok(iter)
    } else {
        tracing::debug!("Exif not found");
        Err("Exif not found".into())
    }
}

#[cfg(feature = "async")]
#[tracing::instrument(skip(reader))]
pub(crate) async fn parse_exif_iter_async<
    R: AsyncRead + Unpin + Send,
    S: crate::skip::AsyncSkip<R>,
>(
    parser: &mut crate::AsyncMediaParser,
    mime_img: MimeImage,
    reader: &mut R,
) -> Result<ExifIter, crate::Error> {
    use crate::parser_async::AsyncBufParser;

    let out = parser
        .load_and_parse::<R, S, _, _>(reader, |buf, state| {
            extract_exif_range(mime_img, buf, state)
        })
        .await?;

    range_to_iter(parser, out)
}

#[tracing::instrument(skip(buf))]
pub(crate) fn extract_exif_with_mime(
    img_type: crate::file::MimeImage,
    buf: &[u8],
    state: Option<ParsingState>,
) -> Result<(Option<&[u8]>, Option<ParsingState>), ParsingErrorState> {
    let (exif_data, state) = match img_type {
        MimeImage::Jpeg => jpeg::extract_exif_data(buf)
            .map(|res| (res.1, state.clone()))
            .map_err(|e| nom_error_to_parsing_error_with_state(e, state))?,
        MimeImage::Heic | crate::file::MimeImage::Heif => heif_extract_exif(state, buf)?,
        MimeImage::Tiff => {
            let (header, data_start) = match state {
                Some(ParsingState::TiffHeader(ref h)) => (h.to_owned(), 0),
                None => {
                    let (_, header) = TiffHeader::parse(buf)
                        .map_err(|e| nom_error_to_parsing_error_with_state(e, None))?;
                    if header.ifd0_offset as usize > buf.len() {
                        let clear_and_skip =
                            ParsingError::ClearAndSkip(header.ifd0_offset as usize);
                        let state = Some(ParsingState::TiffHeader(header));
                        return Err(ParsingErrorState::new(clear_and_skip, state));
                    }
                    let start = header.ifd0_offset as usize;
                    (header, start)
                }
                _ => unreachable!(),
            };

            // full fill TIFF data
            tracing::debug!("full fill TIFF data");
            let mut iter = IfdHeaderTravel::new(
                &buf[data_start..],
                tags::ExifTagCode::Code(0x2a),
                header.ifd0_offset,
                header.endian,
            );
            iter.travel_ifd(0)
                .map_err(|e| ParsingErrorState::new(e, state.clone()))?;
            tracing::debug!("full fill TIFF data done");

            (Some(buf), state)
        }
        MimeImage::Raf => RafInfo::parse(buf)
            .map(|res| (res.1.exif_data, state.clone()))
            .map_err(|e| nom_error_to_parsing_error_with_state(e, state))?,
    };
    Ok((exif_data, state))
}

fn heif_extract_exif(
    state: Option<ParsingState>,
    buf: &[u8],
) -> Result<(Option<&[u8]>, Option<ParsingState>), ParsingErrorState> {
    let (data, state) = match state {
        Some(ParsingState::HeifExifSize(size)) => {
            let (_, data) = nom::bytes::streaming::take(size)(buf)
                .map_err(|e| nom_error_to_parsing_error_with_state(e, state.clone()))?;
            (Some(data), state)
        }
        None => {
            let (_, meta) = heif::parse_meta_box(buf)
                .map_err(|e| nom_error_to_parsing_error_with_state(e, state))?;

            if let Some(meta) = meta {
                if let Some(range) = meta.exif_data_offset() {
                    if range.end > buf.len() {
                        let state = ParsingState::HeifExifSize(range.len());
                        let clear_and_skip = ParsingError::ClearAndSkip(range.start);
                        return Err(ParsingErrorState::new(clear_and_skip, Some(state)));
                    } else {
                        (Some(&buf[range]), None)
                    }
                } else {
                    return Err(ParsingErrorState::new(
                        ParsingError::Failed("no exif offset in meta box".into()),
                        None,
                    ));
                }
            } else {
                (None, None)
            }
        }
        _ => unreachable!(),
    };

    let data = data.and_then(|x| check_exif_header2(x).map(|x| x.0).ok());

    Ok((data, state))
}

#[cfg(feature = "async")]
use tokio::io::AsyncRead;

/// *Deprecated*: Please use [`crate::MediaParser`] instead.
///
/// `async` version of [`parse_exif`].
#[allow(deprecated)]
#[cfg(feature = "async")]
#[deprecated(since = "2.0.0")]
pub async fn parse_exif_async<T: AsyncRead + Unpin + Send>(
    reader: T,
    _: Option<FileFormat>,
) -> crate::Result<Option<ExifIter>> {
    use crate::{AsyncMediaParser, AsyncMediaSource};

    let mut parser = AsyncMediaParser::new();
    let exif: ExifIter = parser
        .parse(AsyncMediaSource::unseekable(reader).await?)
        .await?;
    Ok(Some(exif))
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use std::{sync::mpsc, thread, time::Duration};

    use crate::{
        file::MimeImage,
        testkit::{open_sample, read_sample},
        values::URational,
    };
    use test_case::test_case;

    use super::*;

    #[test_case("exif.heic", "+43.29013+084.22713+1595.950CRSWGS_84/")]
    #[test_case("exif.jpg", "+22.53113+114.02148/")]
    #[test_case("invalid-gps", "-")]
    fn gps(path: &str, gps_str: &str) {
        let f = open_sample(path).unwrap();
        let iter = parse_exif(f, None)
            .expect("should be Ok")
            .expect("should not be None");

        if gps_str == "-" {
            assert!(iter.parse_gps_info().expect("should be ok").is_none());
        } else {
            let gps_info = iter
                .parse_gps_info()
                .expect("should be parsed Ok")
                .expect("should not be None");

            // let gps_info = iter
            //     .consume_parse_gps_info()
            //     .expect("should be parsed Ok")
            //     .expect("should not be None");
            assert_eq!(gps_info.format_iso6709(), gps_str);
        }
    }

    #[cfg(feature = "async")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    #[test_case("exif.heic", "+43.29013+084.22713+1595.950CRSWGS_84/")]
    #[test_case("exif.jpg", "+22.53113+114.02148/")]
    async fn gps_async(path: &str, gps_str: &str) {
        use std::path::Path;
        use tokio::fs::File;

        let f = File::open(Path::new("testdata").join(path)).await.unwrap();
        let iter = parse_exif_async(f, None)
            .await
            .expect("should be Ok")
            .expect("should not be None");

        let gps_str = gps_str.to_owned();
        let _ = tokio::spawn(async move {
            let exif: Exif = iter.into();
            let gps_info = exif.get_gps_info().expect("ok").expect("some");
            assert_eq!(gps_info.format_iso6709(), gps_str);
        })
        .await;
    }

    #[test_case(
        "exif.jpg",
        'N',
        [(22, 1), (31, 1), (5208, 100)].into(),
        'E',
        [(114, 1), (1, 1), (1733, 100)].into(),
        0u8,
        (0, 1).into(),
        None,
        None
    )]
    #[allow(clippy::too_many_arguments)]
    fn gps_info(
        path: &str,
        latitude_ref: char,
        latitude: LatLng,
        longitude_ref: char,
        longitude: LatLng,
        altitude_ref: u8,
        altitude: URational,
        speed_ref: Option<char>,
        speed: Option<URational>,
    ) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let (data, _) = extract_exif_with_mime(MimeImage::Jpeg, &buf, None).unwrap();
        let data = data.unwrap();

        let subslice_in_range = buf.subslice_in_range(data).unwrap();
        let iter = input_into_iter((buf, subslice_in_range), None).unwrap();
        let exif: Exif = iter.into();

        let gps = exif.get_gps_info().unwrap().unwrap();
        assert_eq!(
            gps,
            GPSInfo {
                latitude_ref,
                latitude,
                longitude_ref,
                longitude,
                altitude_ref,
                altitude,
                speed_ref,
                speed,
            }
        )
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

    #[test]
    fn endless_loop() {
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let name = "endless_loop.jpg";
            let f = open_sample(name).unwrap();
            let iter = parse_exif(f, None).unwrap().unwrap();
            let _: Exif = iter.into();
            sender.send(()).unwrap();
        });

        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("There is an infinite loop in the parsing process!");
    }
}

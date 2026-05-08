use crate::error::{nom_error_to_parsing_error_with_state, ParsingError, ParsingErrorState};
use crate::file::MediaMimeImage;
use crate::parser::{BufParser, ParsingState, ShareBuf};
use crate::raf::RafInfo;
use crate::slice::SubsliceRange;
use crate::{cr3, heif, jpeg, MediaParser};
use crate::partial_vec::PartialVec;
pub use exif_exif::Exif;
use exif_exif::TIFF_HEADER_LEN;
use exif_iter::input_into_iter;
pub use exif_iter::{ExifIter, ParsedExifEntry};
pub use gps::{GPSInfo, LatLng};
pub use tags::ExifTag;

use std::io::Read;
use std::ops::Range;

pub(crate) mod ifd;
pub(crate) use exif_exif::{check_exif_header, check_exif_header2, TiffHeader};
pub(crate) use travel::IfdHeaderTravel;

mod exif_exif;
mod exif_iter;
mod gps;
mod tags;
mod travel;

#[tracing::instrument(skip(reader, skip_by_seek))]
pub(crate) fn parse_exif_iter<R: Read>(
    parser: &mut MediaParser,
    mime_img: MediaMimeImage,
    reader: &mut R,
    skip_by_seek: crate::parser::SkipBySeekFn<R>,
) -> Result<ExifIter, crate::Error> {
    // For CR3 files, we need special handling to get all CMT blocks
    if mime_img == MediaMimeImage::Cr3 {
        return parse_cr3_exif_iter(parser, reader, skip_by_seek);
    }

    let out = parser.load_and_parse(reader, skip_by_seek, |buf, state| {
        extract_exif_range(mime_img, buf, state)
    })?;

    range_to_iter(parser, out)
}

/// Special parser for CR3 files that extracts all CMT blocks (CMT1, CMT2, CMT3)
/// and adds them as additional TIFF blocks to the ExifIter.
#[tracing::instrument(skip(reader, skip_by_seek))]
fn parse_cr3_exif_iter<R: Read>(
    parser: &mut MediaParser,
    reader: &mut R,
    skip_by_seek: crate::parser::SkipBySeekFn<R>,
) -> Result<ExifIter, crate::Error> {
    use crate::parser::Buf;

    // First, parse to get all CMT ranges
    let cmt_ranges = parser
        .load_and_parse(reader, skip_by_seek, |buf, _state| cr3::extract_all_cmt_ranges(buf))?;

    let Some(cmt_ranges) = cmt_ranges else {
        return Err(crate::Error::Malformed {
            kind: crate::error::MalformedKind::Cr3Container,
            message: "no CMT data found".into(),
        });
    };

    if cmt_ranges.ranges.is_empty() {
        return Err(crate::Error::Malformed {
            kind: crate::error::MalformedKind::Cr3Container,
            message: "no CMT ranges available".into(),
        });
    }

    tracing::debug!(
        cmt_count = cmt_ranges.ranges.len(),
        "Found CMT ranges in CR3 file"
    );

    // Get the parser position offset - share_buf will add this to ranges
    let position_offset = parser.position();

    // Get the first CMT range (CMT1) to create the primary ExifIter
    let (first_block_id, first_range) = &cmt_ranges.ranges[0];
    tracing::debug!(
        block_id = first_block_id,
        range = ?first_range,
        position_offset,
        "Creating primary ExifIter from first CMT block"
    );

    // Share the buffer and create the primary ExifIter
    // Note: share_buf adds position_offset to the range internally
    let input: PartialVec = parser.share_buf(first_range.clone());
    let mut iter = input_into_iter(input, None)?;

    // Add remaining CMT blocks as additional TIFF blocks
    // We need to adjust the ranges by position_offset since the PartialVec.data
    // contains the full buffer and ranges need to be absolute
    // Note: We skip CMT3 (MakerNotes) as it has a proprietary format that requires
    // special handling and would produce garbage data if parsed as standard EXIF
    for (block_id, range) in cmt_ranges.ranges.iter().skip(1) {
        // Skip CMT3 (MakerNotes) - it has a proprietary Canon format
        if *block_id == "CMT3" {
            tracing::debug!(block_id, "Skipping CMT3 (MakerNotes) - proprietary format");
            continue;
        }

        let adjusted_range = (range.start + position_offset)..(range.end + position_offset);
        tracing::debug!(
            block_id,
            original_range = ?range,
            adjusted_range = ?adjusted_range,
            "Adding additional CMT block"
        );
        iter.add_tiff_block(block_id.to_string(), adjusted_range, None);
    }

    Ok(iter)
}

type ExifRangeResult = Result<Option<(Range<usize>, Option<TiffHeader>)>, ParsingErrorState>;

fn extract_exif_range(img: MediaMimeImage, buf: &[u8], state: Option<ParsingState>) -> ExifRangeResult {
    let (exif_data, state) = extract_exif_with_mime(img, buf, state)?;
    let header = state.and_then(|x| match x {
        ParsingState::TiffHeader(h) => Some(h),
        ParsingState::HeifExifSize(_) => None,
        ParsingState::Cr3ExifSize(_) => None,
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
        Err(crate::Error::ExifNotFound)
    }
}

#[cfg(feature = "tokio")]
#[tracing::instrument(skip(reader))]
pub(crate) async fn parse_exif_iter_async<
    R: AsyncRead + Unpin + Send,
    S: crate::skip::AsyncSkip<R>,
>(
    parser: &mut crate::AsyncMediaParser,
    mime_img: MediaMimeImage,
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
    img_type: crate::file::MediaMimeImage,
    buf: &[u8],
    state: Option<ParsingState>,
) -> Result<(Option<&[u8]>, Option<ParsingState>), ParsingErrorState> {
    let (exif_data, state) = match img_type {
        MediaMimeImage::Jpeg => jpeg::extract_exif_data(buf)
            .map(|res| (res.1, state.clone()))
            .map_err(|e| nom_error_to_parsing_error_with_state(e, state))?,
        MediaMimeImage::Heic | crate::file::MediaMimeImage::Heif => heif_extract_exif(state, buf)?,
        MediaMimeImage::Tiff => {
            let header = match state {
                Some(ParsingState::TiffHeader(ref h)) => h.to_owned(),
                None => {
                    let (_, header) = TiffHeader::parse(buf)
                        .map_err(|e| nom_error_to_parsing_error_with_state(e, None))?;
                    if header.ifd0_offset as usize > buf.len() {
                        let clear_and_skip =
                            ParsingError::Need(header.ifd0_offset as usize - TIFF_HEADER_LEN + 2);
                        let state = Some(ParsingState::TiffHeader(header));
                        return Err(ParsingErrorState::new(clear_and_skip, state));
                    }
                    header
                }
                _ => {
                    return Err(ParsingErrorState::new(
                        ParsingError::Failed("unexpected parsing state for tiff".into()),
                        None,
                    ))
                }
            };

            // full fill TIFF data
            tracing::debug!("full fill TIFF data");
            let mut iter = IfdHeaderTravel::new(
                buf,
                header.ifd0_offset as usize,
                tags::ExifTagCode::Code(0x2a),
                header.endian,
            );
            iter.travel_ifd(0)
                .map_err(|e| ParsingErrorState::new(e, state.clone()))?;
            tracing::debug!("full fill TIFF data done");

            (Some(buf), state)
        }
        MediaMimeImage::Raf => RafInfo::parse(buf)
            .map(|res| (res.1.exif_data, state.clone()))
            .map_err(|e| nom_error_to_parsing_error_with_state(e, state))?,
        MediaMimeImage::Cr3 => cr3_extract_exif(state, buf)?,
    };
    Ok((exif_data, state))
}

fn heif_extract_exif(
    state: Option<ParsingState>,
    buf: &[u8],
) -> Result<(Option<&[u8]>, Option<ParsingState>), ParsingErrorState> {
    heif::extract_exif_data(state, buf)
}

fn cr3_extract_exif(
    state: Option<ParsingState>,
    buf: &[u8],
) -> Result<(Option<&[u8]>, Option<ParsingState>), ParsingErrorState> {
    cr3::extract_exif_data(state, buf)
}

#[cfg(feature = "tokio")]
use tokio::io::AsyncRead;

#[cfg(test)]
mod tests {
    use crate::{
        file::MediaMimeImage,
        testkit::read_sample,
        values::URational,
    };
    use test_case::test_case;

    use super::*;

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
        let (data, _) = extract_exif_with_mime(MediaMimeImage::Jpeg, &buf, None).unwrap();
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

}

use nom::combinator::fail;
use nom::IResult;

use crate::bbox::find_box;
use crate::{
    bbox::{BoxHolder, MetaBox, ParseBox},
    error::{nom_error_to_parsing_error_with_state, ParsingError, ParsingErrorState},
    exif::check_exif_header2,
    parser::ParsingState,
};

pub(crate) fn extract_exif_data(
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
            let (_, meta) =
                parse_meta_box(buf).map_err(|e| nom_error_to_parsing_error_with_state(e, state))?;

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
        _ => {
            return Err(ParsingErrorState::new(
                ParsingError::Failed("unexpected parsing state for heif".into()),
                None,
            ))
        }
    };

    let data = data.and_then(|x| check_exif_header2(x).map(|x| x.0).ok());

    Ok((data, state))
}

pub(crate) fn parse_meta_box(input: &[u8]) -> IResult<&[u8], Option<MetaBox>> {
    let remain = input;
    let (remain, bbox) = BoxHolder::parse(remain)?;
    if bbox.box_type() != "ftyp" {
        return fail(input);
    }

    let (remain, Some(bbox)) = find_box(remain, "meta")? else {
        tracing::debug!(?bbox, "meta box not found");
        return Ok((remain, None));
    };
    tracing::debug!(
        ?bbox,
        pos = input.len() - remain.len() - bbox.header.box_size as usize,
        "Got meta box"
    );
    let (_, bbox) = MetaBox::parse_box(bbox.data)?;
    tracing::debug!(?bbox, "meta box parsed");
    Ok((remain, Some(bbox)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use test_case::test_case;

    #[test_case("exif-one-entry.heic", 0x24-10)]
    #[test_case("exif.heic", 0xa3a-10)]
    fn heic_exif_data(path: &str, exif_size: usize) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let (exif, _state) = extract_exif_data(None, &buf[..]).unwrap();

        if exif_size == 0 {
            assert!(exif.is_none());
        } else {
            assert_eq!(exif.unwrap().len(), exif_size);
        }
    }
}

use nom::combinator::fail;
use nom::{IResult, Parser};

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
        return fail().parse(input);
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
    use tracing::level_filters::LevelFilter;

    /// Build a minimal `ftyp` box followed by `tail` bytes.  The ftyp body
    /// holds `heic` major brand, zero minor version, and one `heic` compat
    /// brand (20 bytes total including the 8-byte header).
    fn ftyp_with_tail(tail: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&20u32.to_be_bytes()); // box size
        buf.extend_from_slice(b"ftyp");
        buf.extend_from_slice(b"heic"); // major brand
        buf.extend_from_slice(&0u32.to_be_bytes()); // minor
        buf.extend_from_slice(b"heic"); // compat brand
        buf.extend_from_slice(tail);
        buf
    }

    #[test_case("exif-one-entry.heic", 0x24-10)]
    #[test_case("exif.heic", 0xa3a-10)]
    #[test_case("exif.avif", 0xa3a-10)]
    fn heic_exif_data(path: &str, exif_size: usize) {
        // Enable DEBUG level so the `tracing::debug!` format-arg expressions
        // inside `parse_meta_box` are actually evaluated (covers line 71).
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(LevelFilter::DEBUG)
            .try_init();
        let buf = read_sample(path).unwrap();
        let (exif, _state) = extract_exif_data(None, &buf[..]).unwrap();
        assert_eq!(exif.unwrap().len(), exif_size);
    }

    #[test]
    fn heif_second_pass_with_state() {
        // Drive the Some(HeifExifSize(size)) branch (lines 17-20).
        // check_exif_header2 expects a be_u32 prefix followed by "Exif\0\0".
        let mut exif_bytes: Vec<u8> = Vec::new();
        exif_bytes.extend_from_slice(&0u32.to_be_bytes()); // tiff offset prefix
        exif_bytes.extend_from_slice(b"Exif\0\0");
        exif_bytes.extend_from_slice(b"II*\0\x08\0\0\0\x00\0\0\0");
        let state = Some(ParsingState::HeifExifSize(exif_bytes.len()));
        let (data, _) = extract_exif_data(state, &exif_bytes).unwrap();
        assert!(data.is_some());
    }

    #[test]
    fn heif_second_pass_short_buffer_errors() {
        // HeifExifSize advertises more bytes than the buffer carries — the
        // streaming `take` fails and the error-mapping closure on line 19
        // runs.
        let state = Some(ParsingState::HeifExifSize(64));
        let buf = vec![0u8; 4];
        let result = extract_exif_data(state, &buf);
        assert!(result.is_err());
    }

    #[test]
    fn heif_clear_and_skip_when_exif_past_eof() {
        // exif.heic's meta box occupies bytes 0x24..0xE1E.  Truncating just
        // past the meta box leaves the ftyp + meta intact while cutting the
        // exif payload that lives further into the file — exactly the
        // ClearAndSkip path on lines 29-31.
        let buf = read_sample("exif.heic").unwrap();
        let cut = 0xE1E.min(buf.len());
        let truncated = &buf[..cut];
        let err = extract_exif_data(None, truncated).expect_err("expected ClearAndSkip");
        assert!(matches!(err.err, ParsingError::ClearAndSkip(_)));
        assert!(matches!(err.state, Some(ParsingState::HeifExifSize(_))));
    }

    #[test]
    fn heif_bad_ftyp_fails() {
        // Build a syntactically valid box whose type is NOT "ftyp", so
        // BoxHolder::parse succeeds but parse_meta_box hits the explicit
        // `fail()` on line 62.
        let mut buf = Vec::new();
        buf.extend_from_slice(&16u32.to_be_bytes()); // box size = 16
        buf.extend_from_slice(b"mdat"); // box type, deliberately not ftyp
        buf.extend_from_slice(&[0u8; 8]); // 8 bytes of body to satisfy take(16)
        let result = parse_meta_box(&buf);
        assert!(result.is_err(), "non-ftyp lead box must error");
    }

    #[test]
    fn heif_extract_no_meta_returns_none() {
        // ftyp present but no meta box afterward — drives
        // `extract_exif_data` through the `meta is None` arm on line 42.
        let buf = ftyp_with_tail(&[]);
        let (data, state) = extract_exif_data(None, &buf).unwrap();
        assert!(data.is_none());
        assert!(state.is_none());
    }

    #[test]
    fn heif_meta_box_not_found() {
        // ftyp present but no meta box afterward — covers lines 66-67.
        let buf = ftyp_with_tail(&[]);
        let (_, meta) = parse_meta_box(&buf).unwrap();
        assert!(meta.is_none());
    }

    #[test]
    #[should_panic]
    fn heif_unexpected_state_panics_or_errors() {
        // Pass a state that isn't HeifExifSize — covers the `_ =>` arm
        // (lines 45-50). The .unwrap() at the end panics on the returned
        // Err.
        let state = Some(ParsingState::Cr3ExifSize(10));
        let buf = vec![0u8; 32];
        let _ = extract_exif_data(state, &buf).unwrap();
    }
}

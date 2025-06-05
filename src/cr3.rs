use nom::IResult;

use crate::{
    bbox::Cr3MoovBox,
    error::{nom_error_to_parsing_error_with_state, ParsingError, ParsingErrorState},
    exif::{check_exif_header2, TiffHeader},
    parser::ParsingState,
};

pub(crate) fn parse_moov_box(input: &[u8]) -> IResult<&[u8], Option<Cr3MoovBox>> {
    Cr3MoovBox::parse(input)
}

pub(crate) fn extract_exif_data(
    state: Option<ParsingState>,
    buf: &[u8],
) -> Result<(Option<&[u8]>, Option<ParsingState>), ParsingErrorState> {
    let (data, state) = match state {
        Some(ParsingState::Cr3ExifSize(size)) => {
            let (_, data) = nom::bytes::streaming::take(size)(buf)
                .map_err(|e| nom_error_to_parsing_error_with_state(e, state.clone()))?;
            (Some(data), state)
        }
        None => {
            let (_, moov) =
                parse_moov_box(buf).map_err(|e| nom_error_to_parsing_error_with_state(e, state))?;

            if let Some(moov) = moov {
                if let Some(range) = moov.exif_data_offset() {
                    if range.end > buf.len() {
                        let state = ParsingState::Cr3ExifSize(range.len());
                        let clear_and_skip = ParsingError::ClearAndSkip(range.start);
                        return Err(ParsingErrorState::new(clear_and_skip, Some(state)));
                    } else {
                        (Some(&buf[range]), None)
                    }
                } else {
                    return Err(ParsingErrorState::new(
                        ParsingError::Failed(
                            "CR3 file contains no EXIF data: Canon UUID box found but no CMT1 offset available".into(),
                        ),
                        None,
                    ));
                }
            } else {
                (None, None)
            }
        }
        _ => unreachable!(),
    };

    // For CR3 files, the CMT1 data already contains TIFF header, so we don't need to check for EXIF header
    let data = data.and_then(|x| {
        if TiffHeader::parse(x).is_ok() {
            Some(x)
        } else {
            // Try to find TIFF header if not at the beginning
            check_exif_header2(x).map(|x| x.0).ok()
        }
    });

    Ok((data, state))
}

#[cfg(test)]
mod tests {
    use crate::bbox::Cr3MoovBox;
    use crate::testkit::*;
    use crate::{MediaParser, MediaSource};
    use std::io::Read;
    use test_case::test_case;

    #[test_case("canon-r6.cr3")]
    fn cr3_parse_with_media_parser(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let mut parser = MediaParser::new();
        let ms = MediaSource::file_path(format!("testdata/{}", path)).unwrap();
        assert!(ms.has_exif());

        let iter: crate::ExifIter = parser.parse(ms).unwrap();
        let exif: crate::Exif = iter.into();

        let mut expect = String::new();
        open_sample(&format!("{path}.sorted.txt"))
            .unwrap()
            .read_to_string(&mut expect)
            .unwrap();

        assert_eq!(sorted_exif_entries(&exif).join("\n"), expect.trim());
    }

    #[test_case("canon-r6.cr3")]
    fn cr3_moov_box_parsing(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let (_, moov_box) = Cr3MoovBox::parse(&buf[..]).unwrap();

        assert!(moov_box.is_some(), "Moov box should be found");
        let moov_box = moov_box.unwrap();

        let canon_box = moov_box.uuid_canon_box().unwrap();

        assert!(
            canon_box.exif_data_offset().is_some(),
            "CMT1 box should be found"
        );
        assert!(
            canon_box.cmt2_data_offset().is_some(),
            "CMT2 box should be found"
        );
        assert!(
            canon_box.cmt3_data_offset().is_some(),
            "CMT3 box should be found"
        );

        // Verify the offsets are reasonable
        let cmt1 = canon_box.exif_data_offset().unwrap();
        assert!(cmt1.start < cmt1.end, "CMT1 offset range should be valid");
        assert!(
            cmt1.end <= buf.len(),
            "CMT1 offset should be within file bounds"
        );
    }

    #[test_case("canon-r6.cr3")]
    fn test_cmt_api_access(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let (_, moov_box) = Cr3MoovBox::parse(&buf[..]).unwrap();
        let moov_box = moov_box.expect("Should have moov box");

        // Test CMT1 access (should be available)
        assert!(
            moov_box.exif_data_offset().is_some(),
            "Should have CMT1 data"
        );
    }
}

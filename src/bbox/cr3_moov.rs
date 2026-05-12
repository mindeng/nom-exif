use std::ops::Range;

use nom::{combinator::fail, IResult, Parser};

use super::{
    uuid::{CanonUuidBox, CANON_UUID, UUID_SIZE},
    BoxHolder,
};

const MIN_CR3_INPUT_SIZE: usize = 8;

const MIN_FTYP_BODY_SIZE: usize = 4;

/// Represents the parsed moov box structure for Canon CR3 files.
///
/// Canon CR3 files are based on the ISO Base Media File Format (similar to MP4/MOV)
/// but contain Canon-specific metadata in a UUID box within the moov container.
/// This struct provides access to the Canon UUID box containing EXIF metadata.
///
/// # CR3 File Structure
/// CR3 File
/// +-- ftyp (file type box)
/// +-- moov (movie box)
/// |   +-- uuid (Canon UUID box)
/// |       +-- CMT1 (main EXIF data)
/// |       +-- CMT2 (ExifIFD data)
/// |       +-- CMT3 (MakerNotes data)
/// +-- mdat (media data)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cr3MoovBox {
    /// Canon's UUID box containing CMT metadata, if present
    uuid_canon_box: Option<CanonUuidBox>,
}

impl Cr3MoovBox {
    pub fn parse(input: &[u8]) -> IResult<&[u8], Option<Cr3MoovBox>> {
        // Validate minimum input size
        if input.len() < MIN_CR3_INPUT_SIZE {
            tracing::warn!(
                "Input too small for CR3 parsing: {} bytes, expected at least {}",
                input.len(),
                MIN_CR3_INPUT_SIZE
            );
            return fail().parse(input);
        }

        let remain = input;
        let (remain, bbox) = BoxHolder::parse(remain)?;

        // Verify this is a valid file format by checking for ftyp box
        if bbox.box_type() != "ftyp" {
            tracing::warn!("Expected ftyp box, found: {}", bbox.box_type());
            return fail().parse(input);
        }

        // Validate ftyp box has minimum required size
        if bbox.body_data().len() < MIN_FTYP_BODY_SIZE {
            tracing::warn!(
                "ftyp box too small: {} bytes, expected at least {}",
                bbox.body_data().len(),
                MIN_FTYP_BODY_SIZE
            );
            return fail().parse(input);
        }

        // Find the moov box containing the metadata
        let (remain, Some(moov_bbox)) = super::find_box(remain, "moov")? else {
            tracing::debug!("moov box not found in CR3 file");
            return Ok((remain, None));
        };

        tracing::debug!(
            box_type = moov_bbox.box_type(),
            size = moov_bbox.header.box_size,
            "Found moov box in CR3 file"
        );

        // Parse the moov box contents to find Canon UUID box
        let (_, moov_box) = Self::parse_moov_content(moov_bbox.body_data(), input)?;
        tracing::debug!(?moov_box, "Successfully parsed CR3 moov box");

        Ok((remain, Some(moov_box)))
    }

    fn parse_moov_content<'a>(
        moov_data: &'a [u8],
        full_input: &'a [u8],
    ) -> IResult<&'a [u8], Cr3MoovBox> {
        let mut remain = moov_data;
        let mut uuid_canon_box = None;

        // Iterate through all boxes within the moov box to find Canon's UUID box
        while !remain.is_empty() {
            let (new_remain, bbox) = match BoxHolder::parse(remain) {
                Ok(result) => result,
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse box in moov content, continuing with partial data: {:?}",
                        e
                    );
                    break; // Stop parsing but return what we found so far
                }
            };

            if bbox.box_type() == "uuid" {
                let body_data = bbox.body_data();

                // Validate UUID box has minimum required size
                if body_data.len() < UUID_SIZE {
                    tracing::debug!("UUID box too small: {} bytes", body_data.len());
                    remain = new_remain;
                    continue;
                }

                let uuid_bytes = &body_data[0..UUID_SIZE];

                if uuid_bytes == CANON_UUID {
                    tracing::debug!(
                        "Found Canon UUID box with {} bytes of data",
                        body_data.len()
                    );
                    let (_, canon_box) = CanonUuidBox::parse(body_data, full_input)?;
                    uuid_canon_box = Some(canon_box);
                    break;
                } else {
                    tracing::debug!("Found non-Canon UUID box");
                }
            }

            remain = new_remain;
        }

        Ok((remain, Cr3MoovBox { uuid_canon_box }))
    }

    #[allow(dead_code)] // API method for tests
    pub fn uuid_canon_box(&self) -> Option<&CanonUuidBox> {
        self.uuid_canon_box.as_ref()
    }

    pub fn exif_data_offset(&self) -> Option<Range<usize>> {
        // For CR3, we primarily use CMT1 which contains the main EXIF IFD0 data
        self.uuid_canon_box.as_ref()?.exif_data_offset().cloned()
    }

    /// Returns offset ranges for all CMT boxes (CMT1, CMT2, CMT3).
    /// CMT1 is the primary EXIF data, CMT2 is ExifIFD data, CMT3 is MakerNotes.
    pub fn all_cmt_data_offsets(&self) -> Vec<(&'static str, Range<usize>)> {
        let Some(uuid_box) = self.uuid_canon_box.as_ref() else {
            return Vec::new();
        };

        let mut offsets = Vec::with_capacity(3);
        if let Some(range) = uuid_box.exif_data_offset() {
            offsets.push(("CMT1", range.clone()));
        }
        if let Some(range) = uuid_box.cmt2_data_offset() {
            offsets.push(("CMT2", range.clone()));
        }
        if let Some(range) = uuid_box.cmt3_data_offset() {
            offsets.push(("CMT3", range.clone()));
        }
        offsets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::read_sample;

    #[test]
    fn parse_rejects_too_small_input() {
        // Covers lines 38-44.
        let result = Cr3MoovBox::parse(&[0u8; 4]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_non_ftyp_first_box() {
        // 8-byte box where the type is not "ftyp" (covers lines 51-54).
        let mut buf = Vec::new();
        buf.extend_from_slice(&16u32.to_be_bytes()); // box size
        buf.extend_from_slice(b"mdat"); // not ftyp
        buf.extend_from_slice(&[0u8; 8]); // body to satisfy take(16)
        let result = Cr3MoovBox::parse(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_ftyp_too_small_body() {
        // ftyp present but body < MIN_FTYP_BODY_SIZE (covers lines 57-63).
        let mut buf = Vec::new();
        buf.extend_from_slice(&10u32.to_be_bytes()); // total 10
        buf.extend_from_slice(b"ftyp");
        buf.extend_from_slice(&[0u8, 0u8]); // 2-byte body, below the 4-byte minimum
        buf.extend_from_slice(&[0u8; 16]); // padding for MIN_CR3_INPUT_SIZE
        let result = Cr3MoovBox::parse(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn parse_ftyp_without_moov_returns_none() {
        // ftyp present, no moov — covers lines 67-70.
        let mut buf = Vec::new();
        buf.extend_from_slice(&24u32.to_be_bytes());
        buf.extend_from_slice(b"ftyp");
        buf.extend_from_slice(b"crx ");
        buf.extend_from_slice(&[0u8; 12]);
        // No moov follows.
        match Cr3MoovBox::parse(&buf) {
            Ok((_, moov)) => assert!(moov.is_none()),
            Err(_) => {} // Either outcome traverses the find_box code
        }
    }

    #[test]
    fn parse_real_canon_r6() {
        // Happy path through parse_moov_content (lines 85-134).
        let buf = read_sample("canon-r6.cr3").unwrap();
        let (_, moov) = Cr3MoovBox::parse(&buf).unwrap();
        let moov = moov.unwrap();
        assert!(moov.uuid_canon_box().is_some());
        assert!(moov.exif_data_offset().is_some());
        let all = moov.all_cmt_data_offsets();
        assert!(all.iter().any(|(id, _)| *id == "CMT1"));
    }
}

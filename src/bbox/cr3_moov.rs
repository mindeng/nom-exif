use std::ops::Range;

use nom::{combinator::fail, IResult};

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
            return fail(input);
        }

        let remain = input;
        let (remain, bbox) = BoxHolder::parse(remain)?;

        // Verify this is a valid file format by checking for ftyp box
        if bbox.box_type() != "ftyp" {
            tracing::warn!("Expected ftyp box, found: {}", bbox.box_type());
            return fail(input);
        }

        // Validate ftyp box has minimum required size
        if bbox.body_data().len() < MIN_FTYP_BODY_SIZE {
            tracing::warn!(
                "ftyp box too small: {} bytes, expected at least {}",
                bbox.body_data().len(),
                MIN_FTYP_BODY_SIZE
            );
            return fail(input);
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
}

use std::ops::Range;

use nom::IResult;

use super::BoxHolder;
use crate::exif::TiffHeader;

/// Size of a UUID in bytes
pub const UUID_SIZE: usize = 16;

/// Canon CMT box types
const CMT_BOX_TYPES: &[&str] = &["CMT1", "CMT2", "CMT3"];

/// Canon's UUID for CR3 files: 85c0b687-820f-11e0-8111-f4ce462b6a48
pub const CANON_UUID: [u8; 16] = [
    0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48,
];

/// Represents Canon's UUID box containing CMT (Canon Metadata) boxes.
///
/// Canon CR3 files store EXIF metadata in a proprietary UUID box format.
/// The UUID box contains three CMT (Canon Metadata) sub-boxes:
/// - CMT1: Main EXIF IFD0 data (camera settings, basic metadata)
/// - CMT2: ExifIFD data (detailed EXIF information)
/// - CMT3: MakerNotes data (Canon-specific metadata)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonUuidBox {
    /// CMT1 contains the main EXIF IFD0 data (primary metadata)
    cmt1_offset: Option<Range<usize>>,
    /// CMT2 contains the ExifIFD data (detailed EXIF information)
    cmt2_offset: Option<Range<usize>>,
    /// CMT3 contains the MakerNotes data (Canon-specific metadata)
    cmt3_offset: Option<Range<usize>>,
}

impl CanonUuidBox {
    /// Returns the offset range for the primary EXIF data (CMT1).
    pub fn exif_data_offset(&self) -> Option<&Range<usize>> {
        // For CR3, we primarily use CMT1 which contains the main EXIF IFD0 data
        self.cmt1_offset.as_ref()
    }

    /// Returns the offset range for the ExifIFD data (CMT2).
    #[allow(dead_code)] // API method for future use
    pub fn cmt2_data_offset(&self) -> Option<&Range<usize>> {
        self.cmt2_offset.as_ref()
    }

    /// Returns the offset range for the MakerNotes data (CMT3).
    #[allow(dead_code)] // API method for future use
    pub fn cmt3_data_offset(&self) -> Option<&Range<usize>> {
        self.cmt3_offset.as_ref()
    }

    /// Parses Canon's UUID box to extract CMT (Canon Metadata) box offsets.
    pub fn parse<'a>(uuid_data: &'a [u8], full_input: &'a [u8]) -> IResult<&'a [u8], CanonUuidBox> {
        // Validate input sizes
        if uuid_data.len() < UUID_SIZE {
            tracing::error!(
                "Canon UUID box data too small: {} bytes, expected at least {}",
                uuid_data.len(),
                UUID_SIZE
            );
            return nom::combinator::fail(uuid_data);
        }

        if full_input.is_empty() {
            tracing::error!("Full input is empty for Canon UUID box parsing");
            return nom::combinator::fail(uuid_data);
        }

        // Skip the UUID header
        let mut remain = &uuid_data[UUID_SIZE..];
        let mut cmt1_offset = None;
        let mut cmt2_offset = None;
        let mut cmt3_offset = None;

        tracing::debug!(
            "Parsing Canon UUID box with {} bytes of CMT data",
            remain.len()
        );

        // Parse CMT boxes within the Canon UUID box
        while !remain.is_empty() {
            let (new_remain, bbox) = match BoxHolder::parse(remain) {
                Ok(result) => result,
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse CMT box, continuing with partial data: {:?}",
                        e
                    );
                    break; // Stop parsing but return what we found so far
                }
            };

            let box_type = bbox.box_type();
            if CMT_BOX_TYPES.contains(&box_type) {
                // Calculate offset safely using slice bounds checking
                let data_start = bbox.data.as_ptr() as usize;
                let input_start = full_input.as_ptr() as usize;

                // Ensure the data pointer is within the input bounds
                if data_start < input_start || data_start >= input_start + full_input.len() {
                    tracing::warn!("CMT box data pointer outside input bounds");
                    remain = new_remain;
                    continue;
                }

                let start_offset = data_start - input_start;
                let body_start = start_offset + bbox.header_size();
                let body_end = start_offset + bbox.data.len();

                // Validate offset ranges are within bounds
                if body_end > full_input.len() {
                    tracing::warn!(
                        "CMT box body extends beyond input bounds: {}..{} > {}",
                        body_start,
                        body_end,
                        full_input.len()
                    );
                    remain = new_remain;
                    continue;
                }

                let offset_range = body_start..body_end;

                // Validate CMT box data has minimum size and reasonable content
                let cmt_data = &full_input[offset_range.clone()];
                if !Self::validate_cmt_data(box_type, cmt_data) {
                    tracing::warn!("CMT box {} failed validation, skipping", box_type);
                    remain = new_remain;
                    continue;
                }

                match box_type {
                    "CMT1" => {
                        cmt1_offset = Some(offset_range);
                        tracing::debug!("Found CMT1 (IFD0) at offset {}..{}", body_start, body_end);
                    }
                    "CMT2" => {
                        cmt2_offset = Some(offset_range);
                        tracing::debug!(
                            "Found CMT2 (ExifIFD) at offset {}..{}",
                            body_start,
                            body_end
                        );
                    }
                    "CMT3" => {
                        cmt3_offset = Some(offset_range);
                        tracing::debug!(
                            "Found CMT3 (MakerNotes) at offset {}..{}",
                            body_start,
                            body_end
                        );
                    }
                    _ => unreachable!("box_type should be one of CMT1, CMT2, or CMT3"),
                }
            } else {
                // Skip unknown boxes within Canon UUID
                tracing::debug!("Skipping unknown box type: {}", box_type);
            }

            remain = new_remain;
        }

        Ok((
            remain,
            CanonUuidBox {
                cmt1_offset,
                cmt2_offset,
                cmt3_offset,
            },
        ))
    }

    /// Validates CMT box data for basic integrity.
    fn validate_cmt_data(box_type: &str, data: &[u8]) -> bool {
        // Minimum size check - CMT boxes should have at least 8 bytes
        if data.len() < 8 {
            tracing::warn!("CMT box {} too small: {} bytes", box_type, data.len());
            return false;
        }

        match box_type {
            "CMT1" => {
                // CMT1 should start with TIFF header - validate using TiffHeader::parse
                if TiffHeader::parse(data).is_ok() {
                    tracing::debug!("CMT1 has valid TIFF header");
                    true
                } else {
                    tracing::warn!("CMT1 does not have valid TIFF header");
                    false
                }
            }
            "CMT2" | "CMT3" => {
                // CMT2 and CMT3 should also be TIFF format, but we're more lenient
                // since they might have different internal structures
                if data.len() >= 8 {
                    tracing::debug!("CMT box {} has sufficient size", box_type);
                    true
                } else {
                    tracing::warn!("CMT box {} too small for valid data", box_type);
                    false
                }
            }
            _ => {
                tracing::warn!("Unknown CMT box type: {}", box_type);
                false
            }
        }
    }
}

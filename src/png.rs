//! PNG chunk parser — pure-function implementation.
//!
//! This module is the layer that walks the PNG chunk stream and extracts:
//! - The EXIF data range (either an `eXIf` chunk or a hex-encoded TIFF blob
//!   in a legacy `Raw profile type {exif,APP1}` `tEXt` chunk — phase 5
//!   adds the legacy decoding).
//! - The `tEXt` chunks as Latin-1-decoded `(key, value)` pairs.
//!
//! The parser is **stateless and pure**: it operates on a `&[u8]` buffer
//! and returns either a `PngParseOut` (success) or a `ParsingErrorState`
//! (`Need(n)` to fill more bytes, `Skip(n)` to clear-and-skip, or
//! `Failed(msg)` for unrecoverable parse errors). The caller (`MediaParser`)
//! drives I/O.

use std::ops::Range;

use crate::error::{ParsingError, ParsingErrorState};

/// Output of [`extract_chunks`]: where the EXIF data lives (if any) and
/// every `tEXt` (key, value) pair encountered, in file order.
#[derive(Debug)]
pub(crate) struct PngParseOut {
    pub exif: Option<PngExifSource>,
    pub text_chunks: Vec<(String, String)>,
}

/// Where the EXIF data was found in the PNG.
#[derive(Debug)]
pub(crate) enum PngExifSource {
    /// PNG 1.5 `eXIf` chunk — TIFF body sits at this byte range inside
    /// the parser buffer. Use this with `bytes::Bytes::slice` for zero-copy.
    EXif(Range<usize>),

    /// Legacy hex-encoded TIFF inside `Raw profile type {exif,APP1}` `tEXt`.
    /// Already hex-decoded + APP1 prefix stripped — owned bytes. Phase 5
    /// adds the actual decoding logic; until then this variant is unused.
    Legacy(Vec<u8>),
}

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Maximum size of a single `tEXt` chunk we'll capture. Above this
/// threshold the chunk is skipped (defensive against crafted inputs).
const MAX_TEXT_CHUNK_SIZE: u32 = 1024 * 1024; // 1 MiB

/// Maximum cumulative captured `tEXt` byte-length. After exceeding this,
/// further `tEXt` chunks are skipped (already-captured entries kept).
const MAX_TEXT_CHUNKS_TOTAL: usize = 16 * 1024 * 1024; // 16 MiB

/// Walk the PNG chunk stream and extract EXIF + tEXt entries.
///
/// Pure function: no I/O, takes a buffer slice, returns either output
/// or a `ParsingErrorState` requesting more bytes / skipping bytes.
#[tracing::instrument(skip(buf))]
pub(crate) fn extract_chunks(buf: &[u8]) -> Result<PngParseOut, ParsingErrorState> {
    // Verify signature.
    if buf.len() < PNG_SIGNATURE.len() {
        return Err(ParsingErrorState::new(
            ParsingError::Need(PNG_SIGNATURE.len() - buf.len()),
            None,
        ));
    }
    if &buf[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(ParsingErrorState::new(
            ParsingError::Failed("PNG: bad signature".into()),
            None,
        ));
    }

    let mut out = PngParseOut {
        exif: None,
        text_chunks: Vec::new(),
    };
    let mut text_total: usize = 0;
    let _ = text_total;

    let mut cursor = PNG_SIGNATURE.len();

    loop {
        // Need 8 bytes for the chunk header (length:4 + type:4).
        if buf.len() - cursor < 8 {
            return Err(ParsingErrorState::new(
                ParsingError::Need(8 - (buf.len() - cursor)),
                None,
            ));
        }
        let length = u32::from_be_bytes([
            buf[cursor],
            buf[cursor + 1],
            buf[cursor + 2],
            buf[cursor + 3],
        ]);
        let ctype = &buf[cursor + 4..cursor + 8];

        match ctype {
            b"IEND" => break,
            b"eXIf" => {
                let total = 8 + length as usize + 4;
                let remaining = buf.len() - cursor;
                if total > remaining {
                    return Err(ParsingErrorState::new(
                        ParsingError::Need(total - remaining),
                        None,
                    ));
                }
                let data_start = cursor + 8;
                let data_end = data_start + length as usize;
                // Priority: eXIf always wins (highest precedence).
                out.exif = Some(PngExifSource::EXif(data_start..data_end));
                cursor += total;
            }
            _ => {
                let total = 8 + length as usize + 4;
                let remaining = buf.len() - cursor;
                if total > remaining {
                    return Err(ParsingErrorState::new(
                        ParsingError::ClearAndSkip(total - remaining),
                        None,
                    ));
                }
                cursor += total;
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_minimal_png() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(PNG_SIGNATURE);
        // IHDR chunk (1x1 grayscale)
        out.extend_from_slice(&13u32.to_be_bytes());
        out.extend_from_slice(b"IHDR");
        out.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]);
        out.extend_from_slice(&[0, 0, 0, 0]); // CRC
                                              // IEND chunk
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(b"IEND");
        out.extend_from_slice(&[0, 0, 0, 0]); // CRC
        out
    }

    #[test]
    fn extract_chunks_minimal_png() {
        let buf = build_minimal_png();
        let result = extract_chunks(&buf).unwrap();
        assert!(result.exif.is_none());
        assert!(result.text_chunks.is_empty());
    }

    #[test]
    fn extract_chunks_bad_signature() {
        let buf = b"\x00\x00\x00\x00\x00\x00\x00\x00not_png".to_vec();
        let err = extract_chunks(&buf).unwrap_err();
        assert!(matches!(err.err, ParsingError::Failed(_)));
    }

    #[test]
    fn extract_chunks_truncated_signature() {
        let buf = b"\x89PNG".to_vec();
        let err = extract_chunks(&buf).unwrap_err();
        assert!(matches!(err.err, ParsingError::Need(_)));
    }

    fn build_chunk(ctype: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(ctype);
        out.extend_from_slice(data);
        out.extend_from_slice(&[0, 0, 0, 0]); // CRC (unverified)
        out
    }

    fn build_png_with_chunks(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(PNG_SIGNATURE);
        out.extend_from_slice(&build_chunk(
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0],
        ));
        for c in chunks {
            out.extend_from_slice(c);
        }
        out.extend_from_slice(&build_chunk(b"IEND", &[]));
        out
    }

    #[test]
    fn extract_chunks_with_exif() {
        // Tiny "TIFF" body — content doesn't matter at this layer.
        let exif_payload = b"II*\x00\x08\x00\x00\x00MM\x00\x2a";
        let exif_chunk = build_chunk(b"eXIf", exif_payload);
        let buf = build_png_with_chunks(&[exif_chunk]);
        let result = extract_chunks(&buf).unwrap();
        let exif_range = match result.exif {
            Some(PngExifSource::EXif(r)) => r,
            _ => panic!("expected EXif source"),
        };
        assert_eq!(&buf[exif_range], exif_payload);
        assert!(result.text_chunks.is_empty());
    }
}

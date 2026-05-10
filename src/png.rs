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

#[cfg(test)]
mod tests {
    // Tests added in subsequent tasks.
}

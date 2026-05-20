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
use crate::parser::ParsingState;

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

/// Decode bytes as Latin-1 into a `String`. Infallible — every Latin-1
/// byte maps to a Unicode code point (U+0000..U+00FF). Per PNG spec, `tEXt`
/// chunks use Latin-1 encoding; we do not sniff for UTF-8.
fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Decode the value of a `Raw profile type *` `tEXt` chunk.
///
/// ImageMagick writes these chunks with a header preamble:
/// ```text
/// \n
/// exif\n
///        54\n           <- length in bytes (decimal, with leading whitespace)
/// 4949 2a00 0800 0000 ...   <- hex bytes
/// ```
///
/// This helper:
/// 1. Skips the leading `\n` line.
/// 2. Skips the second line (`exif`, `app1`, etc).
/// 3. Skips the third line (length).
/// 4. Hex-decodes the rest, ignoring all whitespace.
fn decode_raw_profile_value(s: &str) -> Result<Vec<u8>, ()> {
    let mut lines = s.lines();
    // Skip the empty first line, the type line, and the length line.
    // Tolerate variations: just consume the first 3 newlines worth of header.
    lines.next().ok_or(())?;
    lines.next().ok_or(())?;
    lines.next().ok_or(())?;
    let body: String = lines.collect();
    hex_decode(&body)
}

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut high: Option<u8> = None;
    for c in s.bytes() {
        let nibble = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return Err(()),
        };
        match high.take() {
            None => high = Some(nibble),
            Some(h) => out.push((h << 4) | nibble),
        }
    }
    if high.is_some() {
        return Err(());
    }
    Ok(out)
}

/// Walk the PNG chunk stream and extract EXIF + tEXt entries.
///
/// Pure function: no I/O, takes a buffer slice plus the resume-state
/// from any prior call, returns either output or a `ParsingErrorState`
/// requesting more bytes / skipping bytes.
///
/// `state` is `None` while `buf` is anchored at byte 0 of the file
/// (initial call, or after a `Need` which only grows the buffer).
/// After a `ClearAndSkip` the parser has dropped the buffer and the
/// resumed `buf` starts at a fresh file offset, so the returned state
/// flips to `Some(ParsingState::PngPastSignature)` to tell the next
/// call not to look for the 8-byte signature at `buf[..8]`.
///
/// `ClearAndSkip(n)` is interpreted by the parser as "advance the
/// parser's logical position by `n` bytes from where it is now". The
/// closure sees `buf` already offset to that position, so the skip
/// request must cover *both* the bytes the walker consumed inside
/// `buf` (`cursor`) and the chunk bytes still beyond it. That is
/// `cursor + total`, not `total - remaining` — the latter would only
/// account for bytes past the buffer's end and leave the walker
/// stranded mid-chunk on retry (issue #55).
#[tracing::instrument(skip(buf))]
pub(crate) fn extract_chunks(
    buf: &[u8],
    state: Option<ParsingState>,
) -> Result<PngParseOut, ParsingErrorState> {
    let past_signature = matches!(state, Some(ParsingState::PngPastSignature));
    // Preserves the incoming flag across error returns. A `Need` keeps
    // whatever the caller already had; only `ClearAndSkip` flips a
    // previously-false flag to true (handled at the skip sites).
    let preserve = || past_signature.then_some(ParsingState::PngPastSignature);
    let skipped = || Some(ParsingState::PngPastSignature);

    let mut cursor = if past_signature {
        // Resumed after a ClearAndSkip; buf[0] is a chunk-header
        // boundary, not the PNG signature.
        0
    } else {
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
        PNG_SIGNATURE.len()
    };

    let mut out = PngParseOut {
        exif: None,
        text_chunks: Vec::new(),
    };
    let mut text_total: usize = 0;
    let mut exif_priority: u8 = 0; // 0 = none, 1 = legacy exif, 2 = legacy APP1, 3 = eXIf

    loop {
        // Need 8 bytes for the chunk header (length:4 + type:4).
        if buf.len() - cursor < 8 {
            return Err(ParsingErrorState::new(
                ParsingError::Need(8 - (buf.len() - cursor)),
                preserve(),
            ));
        }
        let length = u32::from_be_bytes([
            buf[cursor],
            buf[cursor + 1],
            buf[cursor + 2],
            buf[cursor + 3],
        ]);
        let ctype = &buf[cursor + 4..cursor + 8];

        // Compute total chunk size = 8 (header) + length (data) + 4 (CRC).
        // On 32-bit targets, `length as usize + 12` can wrap when length is
        // close to u32::MAX; bail out as malformed instead.
        let total = match (length as usize).checked_add(12) {
            Some(t) => t,
            None => {
                return Err(ParsingErrorState::new(
                    ParsingError::Failed("PNG: chunk length overflows addressable size".into()),
                    preserve(),
                ));
            }
        };

        match ctype {
            b"IEND" => break,
            b"eXIf" => {
                let remaining = buf.len() - cursor;
                if total > remaining {
                    return Err(ParsingErrorState::new(
                        ParsingError::Need(total - remaining),
                        preserve(),
                    ));
                }
                let data_start = cursor + 8;
                let data_end = data_start + length as usize;
                // eXIf has priority 3 (highest), always wins.
                out.exif = Some(PngExifSource::EXif(data_start..data_end));
                exif_priority = 3;
                cursor += total;
            }
            b"tEXt" => {
                if length > MAX_TEXT_CHUNK_SIZE {
                    // Defensive: skip oversized chunks.
                    let remaining = buf.len() - cursor;
                    if total > remaining {
                        return Err(ParsingErrorState::new(
                            ParsingError::ClearAndSkip(cursor + total),
                            skipped(),
                        ));
                    }
                    cursor += total;
                    continue;
                }
                let remaining = buf.len() - cursor;
                if total > remaining {
                    return Err(ParsingErrorState::new(
                        ParsingError::Need(total - remaining),
                        preserve(),
                    ));
                }
                let data = &buf[cursor + 8..cursor + 8 + length as usize];
                // tEXt format: Latin-1 keyword + 0x00 + Latin-1 text
                if let Some(nul_pos) = data.iter().position(|&b| b == 0) {
                    let key = decode_latin1(&data[..nul_pos]);
                    let value = decode_latin1(&data[nul_pos + 1..]);

                    // Legacy EXIF detection
                    let candidate_priority: u8 = match key.as_str() {
                        "Raw profile type APP1" => 2,
                        "Raw profile type exif" => 1,
                        _ => 0,
                    };
                    if candidate_priority > 0 && candidate_priority > exif_priority {
                        if let Ok(mut bytes) = decode_raw_profile_value(&value) {
                            // Strip APP1's leading "Exif\0\0" if present.
                            if key.ends_with("APP1") && bytes.starts_with(b"Exif\0\0") {
                                bytes.drain(0..6);
                            }
                            // Validate as TIFF (must have a valid byte-order marker
                            // + magic number) before accepting.
                            if bytes.len() >= 8 && crate::exif::TiffHeader::parse(&bytes).is_ok() {
                                out.exif = Some(PngExifSource::Legacy(bytes));
                                exif_priority = candidate_priority;
                            }
                            // else: silently drop the legacy candidate, keep raw text entry below
                        }
                        // hex_decode failure → silently drop too
                    }

                    let entry_size = key.len() + value.len();
                    if text_total + entry_size <= MAX_TEXT_CHUNKS_TOTAL {
                        text_total += entry_size;
                        out.text_chunks.push((key, value));
                    }
                    // else: silently skip (already-captured entries kept).
                }
                // else: malformed tEXt (no NUL separator) — silently skip.
                cursor += total;
            }
            _ => {
                let remaining = buf.len() - cursor;
                if total > remaining {
                    return Err(ParsingErrorState::new(
                        ParsingError::ClearAndSkip(cursor + total),
                        skipped(),
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
        let result = extract_chunks(&buf, None).unwrap();
        assert!(result.exif.is_none());
        assert!(result.text_chunks.is_empty());
    }

    #[test]
    fn extract_chunks_bad_signature() {
        let buf = b"\x00\x00\x00\x00\x00\x00\x00\x00not_png".to_vec();
        let err = extract_chunks(&buf, None).unwrap_err();
        assert!(matches!(err.err, ParsingError::Failed(_)));
    }

    #[test]
    fn extract_chunks_truncated_signature() {
        let buf = b"\x89PNG".to_vec();
        let err = extract_chunks(&buf, None).unwrap_err();
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
        let result = extract_chunks(&buf, None).unwrap();
        let exif_range = match result.exif {
            Some(PngExifSource::EXif(r)) => r,
            _ => panic!("expected EXif source"),
        };
        assert_eq!(&buf[exif_range], exif_payload);
        assert!(result.text_chunks.is_empty());
    }

    #[test]
    fn extract_chunks_with_text() {
        let mut text_data = Vec::new();
        text_data.extend_from_slice(b"Title");
        text_data.push(0);
        text_data.extend_from_slice(b"Hello world");
        let chunks = vec![build_chunk(b"tEXt", &text_data)];
        let buf = build_png_with_chunks(&chunks);
        let result = extract_chunks(&buf, None).unwrap();
        assert!(result.exif.is_none());
        assert_eq!(result.text_chunks.len(), 1);
        assert_eq!(result.text_chunks[0].0, "Title");
        assert_eq!(result.text_chunks[0].1, "Hello world");
    }

    #[test]
    fn extract_chunks_text_duplicate_keys() {
        let mut t1 = Vec::new();
        t1.extend_from_slice(b"Comment");
        t1.push(0);
        t1.extend_from_slice(b"first");
        let mut t2 = Vec::new();
        t2.extend_from_slice(b"Comment");
        t2.push(0);
        t2.extend_from_slice(b"second");
        let chunks = vec![build_chunk(b"tEXt", &t1), build_chunk(b"tEXt", &t2)];
        let buf = build_png_with_chunks(&chunks);
        let result = extract_chunks(&buf, None).unwrap();
        assert_eq!(result.text_chunks.len(), 2);
        assert_eq!(result.text_chunks[0], ("Comment".into(), "first".into()));
        assert_eq!(result.text_chunks[1], ("Comment".into(), "second".into()));
    }

    #[test]
    fn extract_chunks_text_no_nul_separator() {
        // Malformed tEXt with no NUL byte — should be silently skipped.
        let chunks = vec![build_chunk(b"tEXt", b"NoNulSeparator")];
        let buf = build_png_with_chunks(&chunks);
        let result = extract_chunks(&buf, None).unwrap();
        assert!(result.text_chunks.is_empty());
    }

    #[test]
    fn extract_chunks_text_latin1_decode() {
        // Latin-1 character outside ASCII (é = 0xE9)
        let mut data = Vec::new();
        data.extend_from_slice(b"Caption");
        data.push(0);
        data.extend_from_slice(b"caf\xE9");
        let chunks = vec![build_chunk(b"tEXt", &data)];
        let buf = build_png_with_chunks(&chunks);
        let result = extract_chunks(&buf, None).unwrap();
        assert_eq!(result.text_chunks[0].1, "café");
    }

    #[test]
    fn extract_chunks_truncated_inside_exif() {
        // PNG signature + IHDR + start of eXIf chunk header (claiming a 100-byte
        // body) but the body is missing.
        let mut buf = Vec::new();
        buf.extend_from_slice(PNG_SIGNATURE);
        buf.extend_from_slice(&build_chunk(b"IHDR", &[0; 13]));
        // Manually emit eXIf header claiming 100 bytes
        buf.extend_from_slice(&100u32.to_be_bytes());
        buf.extend_from_slice(b"eXIf");
        // No body — caller must request Need.

        let err = extract_chunks(&buf, None).unwrap_err();
        match err.err {
            ParsingError::Need(n) => assert!(n >= 100),
            other => panic!("expected Need(>=100), got {other:?}"),
        }
    }

    #[test]
    fn extract_chunks_skips_large_idat() {
        // IDAT chunk declaring a 50_000-byte body that is NOT in the buffer —
        // should produce ParsingError::ClearAndSkip with PngPastSignature so
        // the resumed call (whose buf no longer starts at the signature)
        // doesn't re-check buf[..8].
        let mut buf = Vec::new();
        buf.extend_from_slice(PNG_SIGNATURE);
        buf.extend_from_slice(&build_chunk(b"IHDR", &[0; 13]));
        // IDAT header only, claiming 50_000 bytes
        buf.extend_from_slice(&50_000u32.to_be_bytes());
        buf.extend_from_slice(b"IDAT");

        let err = extract_chunks(&buf, None).unwrap_err();
        // Skip distance must equal `cursor + total` — i.e. PNG signature (8)
        // + IHDR chunk (25) + IDAT total (50_000 body + 12 framing). The old
        // buggy `total - remaining` formula would have under-counted by the
        // entire walker cursor and stranded the parser mid-IDAT on retry.
        match err.err {
            ParsingError::ClearAndSkip(n) => assert_eq!(n, 8 + 25 + 50_000 + 12),
            other => panic!("expected ClearAndSkip, got {other:?}"),
        }
        assert!(
            matches!(err.state, Some(ParsingState::PngPastSignature)),
            "ClearAndSkip must hand back PngPastSignature so the resumed \
             call skips the signature check on the mid-stream slice"
        );
    }

    #[test]
    fn extract_chunks_resumes_past_signature_with_state() {
        // After a ClearAndSkip the next call receives a buf that starts
        // mid-file. Carrying PngPastSignature in state must let the parser
        // skip the buf[..8] signature check and parse the next chunk.
        let mut tail = Vec::new();
        // Just an IEND chunk's bytes — no signature in front.
        tail.extend_from_slice(&0u32.to_be_bytes());
        tail.extend_from_slice(b"IEND");
        tail.extend_from_slice(&[0, 0, 0, 0]); // CRC

        let result = extract_chunks(&tail, Some(ParsingState::PngPastSignature))
            .expect("must not check signature");
        assert!(result.exif.is_none());
        assert!(result.text_chunks.is_empty());
    }

    #[test]
    fn extract_chunks_text_too_large_skipped() {
        // tEXt chunk declaring 2 MiB length — should be skipped without
        // entering text_chunks. We don't actually allocate 2 MiB; emit
        // the header only and let extract_chunks request a Skip.
        let mut buf = Vec::new();
        buf.extend_from_slice(PNG_SIGNATURE);
        buf.extend_from_slice(&build_chunk(b"IHDR", &[0; 13]));
        // tEXt header claiming length > MAX_TEXT_CHUNK_SIZE
        let bogus_length = MAX_TEXT_CHUNK_SIZE + 1;
        buf.extend_from_slice(&bogus_length.to_be_bytes());
        buf.extend_from_slice(b"tEXt");
        // No body provided — but since extract_chunks should skip oversized
        // tEXt, we expect a ClearAndSkip error (not capture).

        let err = extract_chunks(&buf, None).unwrap_err();
        assert!(matches!(err.err, ParsingError::ClearAndSkip(_)));
    }

    #[test]
    fn hex_decode_basic() {
        assert_eq!(hex_decode("4849").unwrap(), b"HI");
        assert_eq!(hex_decode("48 49").unwrap(), b"HI");
        assert_eq!(hex_decode("48\n49").unwrap(), b"HI");
        assert_eq!(hex_decode("aBcD").unwrap(), vec![0xab, 0xcd]);
    }

    #[test]
    fn hex_decode_rejects_invalid() {
        assert!(hex_decode("XX").is_err());
        assert!(hex_decode("48a").is_err()); // odd-length
    }

    #[test]
    fn decode_raw_profile_imagemagick_format() {
        // Mimics ImageMagick's "Raw profile type exif" value layout.
        let v = "\nexif\n      4\n4849 5050\n";
        let bytes = decode_raw_profile_value(v).unwrap();
        assert_eq!(bytes, b"HIPP");
    }

    #[test]
    fn extract_chunks_malicious_text_length_max_u32_does_not_panic() {
        // tEXt with length = u32::MAX. Must not allocate 4 GB or panic.
        // On 32-bit targets, length + 12 overflows usize — the parser must
        // bail with Failed rather than wrap. On 64-bit, length + 12 fits
        // and the buffer-shortage check produces Need/ClearAndSkip.
        let mut buf = Vec::new();
        buf.extend_from_slice(PNG_SIGNATURE);
        buf.extend_from_slice(&build_chunk(b"IHDR", &[0; 13]));
        buf.extend_from_slice(&u32::MAX.to_be_bytes());
        buf.extend_from_slice(b"tEXt");

        // ParsingError has only Need / ClearAndSkip / Failed variants — any
        // of the three is acceptable here; the contract is "no panic, no
        // wrap-around, no infinite loop".
        let _err = extract_chunks(&buf, None).unwrap_err();
    }

    #[test]
    fn extract_chunks_chunk_length_overflow_is_rejected() {
        // Synthesize a length that always overflows usize regardless of
        // target pointer width: only achievable on 32-bit usize because
        // u32::MAX as u64 + 12 fits u64. We assert the more general
        // contract: u32::MAX-length chunks never advance the cursor by a
        // wrapped value (no panic, no infinite loop, no out-of-bounds read).
        let mut buf = Vec::new();
        buf.extend_from_slice(PNG_SIGNATURE);
        buf.extend_from_slice(&build_chunk(b"IHDR", &[0; 13]));
        // Chunk length = u32::MAX, type = unknown ("XXXX") — the `_` arm.
        buf.extend_from_slice(&u32::MAX.to_be_bytes());
        buf.extend_from_slice(b"XXXX");

        let _err = extract_chunks(&buf, None).unwrap_err();
    }

    /// Minimal little-endian TIFF: II + 0x002A + IFD0 offset = 8 + IFD0 with 0 entries.
    fn minimal_tiff_le() -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(b"II"); // little-endian
        t.extend_from_slice(&[0x2a, 0x00]); // magic 42
        t.extend_from_slice(&[0x08, 0, 0, 0]); // IFD0 offset = 8
        t.extend_from_slice(&[0, 0]); // IFD0: 0 entries
        t.extend_from_slice(&[0, 0, 0, 0]); // next IFD = 0
        t
    }

    /// Encode a TIFF blob into the ImageMagick "Raw profile type X" tEXt
    /// value layout: 3-line header + hex bytes.
    fn raw_profile_value(profile_type: &str, tiff: &[u8]) -> String {
        let hex: String = tiff.iter().map(|b| format!("{b:02x}")).collect();
        // Wrap the hex into 72-char lines like ImageMagick (not strictly
        // necessary for our parser; ignored as whitespace).
        let mut wrapped = String::new();
        for chunk in hex.as_bytes().chunks(72) {
            wrapped.push_str(std::str::from_utf8(chunk).unwrap());
            wrapped.push('\n');
        }
        format!("\n{}\n      {}\n{}", profile_type, tiff.len(), wrapped)
    }

    #[test]
    fn extract_chunks_legacy_exif() {
        let tiff = minimal_tiff_le();
        let value = raw_profile_value("exif", &tiff);
        let mut data = Vec::new();
        data.extend_from_slice(b"Raw profile type exif");
        data.push(0);
        data.extend_from_slice(value.as_bytes());
        let chunks = vec![build_chunk(b"tEXt", &data)];
        let buf = build_png_with_chunks(&chunks);

        let result = extract_chunks(&buf, None).unwrap();
        match result.exif {
            Some(PngExifSource::Legacy(bytes)) => assert_eq!(bytes, tiff),
            other => panic!("expected Legacy, got {:?}", other),
        }
        // Original tEXt entry is preserved.
        assert_eq!(result.text_chunks.len(), 1);
        assert_eq!(result.text_chunks[0].0, "Raw profile type exif");
    }

    #[test]
    fn extract_chunks_legacy_app1() {
        let tiff = minimal_tiff_le();
        // APP1 carries an "Exif\0\0" prefix before TIFF.
        let mut app1 = Vec::new();
        app1.extend_from_slice(b"Exif\0\0");
        app1.extend_from_slice(&tiff);
        let value = raw_profile_value("app1", &app1);
        let mut data = Vec::new();
        data.extend_from_slice(b"Raw profile type APP1");
        data.push(0);
        data.extend_from_slice(value.as_bytes());
        let chunks = vec![build_chunk(b"tEXt", &data)];
        let buf = build_png_with_chunks(&chunks);

        let result = extract_chunks(&buf, None).unwrap();
        match result.exif {
            Some(PngExifSource::Legacy(bytes)) => assert_eq!(bytes, tiff),
            other => panic!("expected Legacy, got {:?}", other),
        }
    }

    #[test]
    fn extract_chunks_exif_overrides_legacy() {
        let tiff_legacy = minimal_tiff_le();
        let tiff_exif = {
            let mut t = minimal_tiff_le();
            // Differentiate so we can verify which one was kept.
            t.extend_from_slice(&[0xFF; 4]);
            t
        };
        let legacy_value = raw_profile_value("exif", &tiff_legacy);
        let mut legacy_data = Vec::new();
        legacy_data.extend_from_slice(b"Raw profile type exif");
        legacy_data.push(0);
        legacy_data.extend_from_slice(legacy_value.as_bytes());

        // Order: legacy first, then eXIf. eXIf must still win.
        let chunks = vec![
            build_chunk(b"tEXt", &legacy_data),
            build_chunk(b"eXIf", &tiff_exif),
        ];
        let buf = build_png_with_chunks(&chunks);

        let result = extract_chunks(&buf, None).unwrap();
        match result.exif {
            Some(PngExifSource::EXif(range)) => {
                assert_eq!(&buf[range], tiff_exif);
            }
            other => panic!("expected EXif (eXIf wins), got {:?}", other),
        }
    }

    #[test]
    fn extract_chunks_invalid_legacy_silently_dropped() {
        // Malformed value: not valid hex.
        let mut data = Vec::new();
        data.extend_from_slice(b"Raw profile type exif");
        data.push(0);
        data.extend_from_slice(b"not hex at all\nzzz");
        let chunks = vec![build_chunk(b"tEXt", &data)];
        let buf = build_png_with_chunks(&chunks);

        let result = extract_chunks(&buf, None).unwrap();
        assert!(result.exif.is_none(), "malformed legacy must be dropped");
        // Raw tEXt entry still preserved.
        assert_eq!(result.text_chunks.len(), 1);
    }
}

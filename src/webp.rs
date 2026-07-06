//! WebP (RIFF) chunk parser — pure-function EXIF extractor.
//!
//! WebP is a RIFF container: `"RIFF"` + fileSize(LE u32) + `"WEBP"`, then a
//! sequence of chunks (`FourCC` + size(LE u32) + payload + one pad byte when
//! size is odd). EXIF lives in an `EXIF` chunk (raw TIFF), present only in
//! extended (VP8X) files, and sits *after* the image bitstream — so the
//! walker must skip potentially-large `VP8 `/`VP8L`/`ANMF` chunks to reach it.
//!
//! Unlike PNG there is no terminator chunk, so the walk is bounded by the
//! RIFF size field. That bound (bytes left in the chunk region) is threaded
//! across a `ClearAndSkip` via [`ParsingState::WebpPastHeader`] so the walker
//! stops cleanly (`Ok(None)` → `ExifNotFound`) instead of reading to EOF.
//!
//! Stateless and pure: operates on a `&[u8]` buffer plus the resume-state
//! from any prior call. The caller (`MediaParser`) drives all I/O.

use crate::error::{MalformedKind, ParsingError, ParsingErrorState};
use crate::parser::ParsingState;

const WEBP_HEADER_LEN: usize = 12; // "RIFF" + size(4) + "WEBP"
const CHUNK_HEADER_LEN: usize = 8; // FourCC(4) + size(4)

/// Walk the RIFF chunk stream and return the `EXIF` chunk payload (raw TIFF),
/// borrowed from `buf`, if present.
///
/// `state` is `Some(WebpPastHeader(stream_left))` only after a `ClearAndSkip`:
/// `buf[0]` is then a chunk boundary rather than the 12-byte file header, and
/// `stream_left` is the number of chunk-region bytes remaining from `buf[0]`.
pub(crate) fn extract_exif(
    state: Option<ParsingState>,
    buf: &[u8],
) -> Result<(Option<&[u8]>, Option<ParsingState>), ParsingErrorState> {
    // True only when resuming after a ClearAndSkip (buf[0] is a chunk
    // boundary). Captured before `state` is consumed by the match below.
    let past_header = matches!(state, Some(ParsingState::WebpPastHeader(_)));

    // (cursor start, chunk-region bytes remaining from cursor)
    let (mut cursor, mut stream_left) = match state {
        Some(ParsingState::WebpPastHeader(left)) => (0usize, left),
        _ => {
            if buf.len() < WEBP_HEADER_LEN {
                return Err(ParsingErrorState::new(
                    ParsingError::Need(WEBP_HEADER_LEN - buf.len()),
                    None,
                ));
            }
            if &buf[0..4] != b"RIFF" || &buf[8..12] != b"WEBP" {
                return Err(ParsingErrorState::new(
                    ParsingError::Failed {
                        kind: MalformedKind::WebpChunk,
                        message: "WebP: bad RIFF/WEBP header".into(),
                    },
                    None,
                ));
            }
            // RIFF size = whole file minus 8 ("RIFF" + size field). The chunk
            // region is everything after the "WEBP" FourCC, i.e. riff_size - 4.
            let riff_size = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
            (WEBP_HEADER_LEN, riff_size.saturating_sub(4))
        }
    };

    // Preserve the resume bound across `Need` returns (buffer only grows).
    // Only meaningful once past the header: in the initial header path a
    // `Need` must return `None` so the resumed call re-validates the 12-byte
    // RIFF header still sitting at buf[0] (mirrors png.rs). Returning
    // `WebpPastHeader` there would reset the cursor to 0 and misread "RIFF".
    let preserve = |left: usize| past_header.then_some(ParsingState::WebpPastHeader(left));

    loop {
        // No room in the declared chunk region for another chunk header → done.
        if stream_left < CHUNK_HEADER_LEN {
            return Ok((None, Some(ParsingState::WebpPastHeader(stream_left))));
        }
        // Buffer doesn't yet hold the next chunk header — ask for more bytes.
        let in_buf = buf.len() - cursor;
        if in_buf < CHUNK_HEADER_LEN {
            return Err(ParsingErrorState::new(
                ParsingError::Need(CHUNK_HEADER_LEN - in_buf),
                preserve(stream_left),
            ));
        }

        let fourcc = &buf[cursor..cursor + 4];
        let size = u32::from_le_bytes([
            buf[cursor + 4],
            buf[cursor + 5],
            buf[cursor + 6],
            buf[cursor + 7],
        ]);
        // total = header(8) + payload(size) + pad(1 if size is odd).
        let pad = (size & 1) as usize;
        let total = match (size as usize).checked_add(CHUNK_HEADER_LEN + pad) {
            Some(t) => t,
            None => {
                return Err(ParsingErrorState::new(
                    ParsingError::Failed {
                        kind: MalformedKind::WebpChunk,
                        message: "WebP: chunk size overflows addressable size".into(),
                    },
                    preserve(stream_left),
                ));
            }
        };

        // Chunk claims more than the RIFF container allows — stop at the
        // container boundary. Placed before the EXIF branch so we never
        // return bytes from outside the declared RIFF region.
        if total > stream_left {
            return Ok((None, Some(ParsingState::WebpPastHeader(0))));
        }

        if fourcc == b"EXIF" {
            if total > in_buf {
                return Err(ParsingErrorState::new(
                    ParsingError::Need(total - in_buf),
                    preserve(stream_left),
                ));
            }
            let data_start = cursor + CHUNK_HEADER_LEN;
            let data_end = data_start + size as usize;
            let mut payload = &buf[data_start..data_end];
            // Some encoders wrongly prepend the JPEG APP1 "Exif\0\0" marker.
            if payload.starts_with(b"Exif\0\0") {
                payload = &payload[6..];
            }
            // A chunk containing only the stray marker (no TIFF body) is not
            // usable EXIF — report it as absent rather than an empty slice.
            if payload.is_empty() {
                return Ok((None, preserve(stream_left)));
            }
            return Ok((Some(payload), preserve(stream_left)));
        }

        // Non-EXIF chunk. If it isn't fully buffered, skip it whole: advance
        // the parser by `cursor + total` (bytes walked in `buf` plus this
        // chunk) and resume past the header with the reduced bound.
        if total > in_buf {
            return Err(ParsingErrorState::new(
                ParsingError::ClearAndSkip(cursor + total),
                Some(ParsingState::WebpPastHeader(stream_left - total)),
            ));
        }
        cursor += total;
        stream_left -= total;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal little-endian TIFF: II + 0x002A + IFD0 offset 8 + empty IFD0.
    fn minimal_tiff_le() -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(b"II");
        t.extend_from_slice(&[0x2a, 0x00]);
        t.extend_from_slice(&[0x08, 0, 0, 0]);
        t.extend_from_slice(&[0, 0]);
        t.extend_from_slice(&[0, 0, 0, 0]);
        t
    }

    fn chunk(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(fourcc);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(0); // pad to even
        }
        out
    }

    /// Wrap chunk bytes in a RIFF/WEBP container with a correct size field.
    fn webp(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(b"WEBP");
        for c in chunks {
            body.extend_from_slice(c);
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn no_exif_returns_none() {
        let buf = webp(&[chunk(b"VP8X", &[0u8; 10])]);
        let (exif, _) = extract_exif(None, &buf).unwrap();
        assert!(exif.is_none());
    }

    #[test]
    fn extracts_exif_payload() {
        let tiff = minimal_tiff_le();
        let buf = webp(&[chunk(b"VP8X", &[0u8; 10]), chunk(b"EXIF", &tiff)]);
        let (exif, _) = extract_exif(None, &buf).unwrap();
        assert_eq!(exif.unwrap(), tiff.as_slice());
    }

    #[test]
    fn strips_exif_prefix() {
        let tiff = minimal_tiff_le();
        let mut payload = Vec::new();
        payload.extend_from_slice(b"Exif\0\0");
        payload.extend_from_slice(&tiff);
        let buf = webp(&[chunk(b"EXIF", &payload)]);
        let (exif, _) = extract_exif(None, &buf).unwrap();
        assert_eq!(exif.unwrap(), tiff.as_slice());
    }

    #[test]
    fn odd_length_chunk_padding_accounted() {
        // A 3-byte chunk (odd → 1 pad byte) before EXIF. If padding is
        // miscounted, the EXIF FourCC won't align and extraction fails.
        let tiff = minimal_tiff_le();
        let buf = webp(&[chunk(b"ICCP", &[1, 2, 3]), chunk(b"EXIF", &tiff)]);
        let (exif, _) = extract_exif(None, &buf).unwrap();
        assert_eq!(exif.unwrap(), tiff.as_slice());
    }

    #[test]
    fn bad_header_fails() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(b"WAVE"); // not WEBP
        buf.extend_from_slice(&[0u8; 8]);
        let err = extract_exif(None, &buf).unwrap_err();
        assert!(matches!(
            err.err,
            ParsingError::Failed {
                kind: MalformedKind::WebpChunk,
                ..
            }
        ));
    }

    #[test]
    fn truncated_header_needs_more() {
        let buf = b"RIFF\x00\x00".to_vec();
        let err = extract_exif(None, &buf).unwrap_err();
        assert!(matches!(err.err, ParsingError::Need(_)));
    }

    #[test]
    fn large_leading_chunk_clear_and_skips() {
        // A VP8L chunk declaring 50_000 bytes not present in the buffer must
        // yield ClearAndSkip(cursor + total) + WebpPastHeader(remaining).
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        // riff_size large enough to contain the declared chunk + an EXIF later.
        buf.extend_from_slice(&1_000_000u32.to_le_bytes());
        buf.extend_from_slice(b"WEBP");
        // VP8L header only, claiming 50_000 bytes of body.
        buf.extend_from_slice(b"VP8L");
        buf.extend_from_slice(&50_000u32.to_le_bytes());
        let err = extract_exif(None, &buf).unwrap_err();
        match err.err {
            // cursor at skip time = 12 (header); total = 8 + 50_000 (+0 pad).
            ParsingError::ClearAndSkip(n) => assert_eq!(n, 12 + 8 + 50_000),
            other => panic!("expected ClearAndSkip, got {other:?}"),
        }
        assert!(matches!(err.state, Some(ParsingState::WebpPastHeader(_))));
    }

    #[test]
    fn resumes_past_header_finds_exif() {
        // Simulate the post-ClearAndSkip resume: buf starts at a chunk
        // boundary (no RIFF header), state carries the remaining bound.
        let tiff = minimal_tiff_le();
        let exif_chunk = chunk(b"EXIF", &tiff);
        let stream_left = exif_chunk.len();
        let (exif, _) =
            extract_exif(Some(ParsingState::WebpPastHeader(stream_left)), &exif_chunk).unwrap();
        assert_eq!(exif.unwrap(), tiff.as_slice());
    }

    #[test]
    fn size_max_does_not_panic() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&u32::MAX.to_le_bytes());
        buf.extend_from_slice(b"WEBP");
        buf.extend_from_slice(b"VP8L");
        buf.extend_from_slice(&u32::MAX.to_le_bytes());
        // Any of Ok(None)/Need/ClearAndSkip/Failed is fine; the contract is
        // "no panic, no wrap-around, no infinite loop".
        let _ = extract_exif(None, &buf);
    }

    #[test]
    fn header_only_need_then_resume_finds_exif() {
        // First call sees only the 12-byte RIFF header (streaming I/O). It must
        // return Need with state None (NOT WebpPastHeader) so the resumed call
        // re-validates the header at buf[0] instead of misreading "RIFF" as a
        // chunk header. This is the regression test for the Critical fix.
        let tiff = minimal_tiff_le();
        let full = webp(&[chunk(b"EXIF", &tiff)]);
        let header_only = &full[..WEBP_HEADER_LEN];
        let err = extract_exif(None, header_only).unwrap_err();
        assert!(matches!(err.err, ParsingError::Need(_)));
        assert!(
            err.state.is_none(),
            "initial-path Need must not carry WebpPastHeader"
        );
        let (exif, _) = extract_exif(err.state, &full).unwrap();
        assert_eq!(exif.unwrap(), tiff.as_slice());
    }

    #[test]
    fn non_exif_chunk_exceeding_riff_bound_returns_none() {
        // RIFF declares a 12-byte chunk region but a chunk claims 1000 bytes.
        // The walker must stop at the container boundary → Ok(None).
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&16u32.to_le_bytes()); // chunk region = 16 - 4 = 12
        buf.extend_from_slice(b"WEBP");
        buf.extend_from_slice(b"VP8L");
        buf.extend_from_slice(&1000u32.to_le_bytes());
        let (exif, _) = extract_exif(None, &buf).unwrap();
        assert!(exif.is_none());
    }

    #[test]
    fn exif_chunk_only_prefix_returns_none() {
        // An EXIF chunk containing just the stray "Exif\0\0" marker and no
        // TIFF body must not yield an empty EXIF slice.
        let buf = webp(&[chunk(b"EXIF", b"Exif\0\0")]);
        let (exif, _) = extract_exif(None, &buf).unwrap();
        assert!(exif.is_none());
    }
}

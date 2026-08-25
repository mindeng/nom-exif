# WebP EXIF Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add WebP image EXIF/GPS extraction to `nom-exif`, wired into the existing unified `parse_exif` / `read_exif` (sync + async) APIs.

**Architecture:** WebP is a RIFF container (`"RIFF"` + fileSize(LE u32) + `"WEBP"`, then FourCC-tagged chunks). EXIF lives in an `EXIF` chunk (raw TIFF) *after* the image bitstream. Rather than a PNG-style special-case path, WebP rides the **generic** EXIF path: a pure walker `webp::extract_exif(state, buf) -> Result<(Option<&[u8]>, Option<ParsingState>), ParsingErrorState>` slots into `exif::extract_exif_with_mime`, reusing `extract_exif_range`, `range_to_iter`, and both the sync and async drivers with zero async-specific code. The walker bounds itself by the RIFF size field (WebP has no terminator chunk), skipping large non-EXIF chunks via `ClearAndSkip`.

**Tech Stack:** Rust, `nom`, `bytes::Bytes` (zero-copy buffer sharing), `tokio` (optional async feature), `test-case` (parameterized tests).

---

## Refinement over the spec

The spec described `ParsingState::WebpPastHeader` as a unit variant (mirroring PNG's `PngPastSignature`). During code-level design one change was needed: **WebP has no explicit terminator chunk** (PNG stops at `IEND`), so the walker must stop when it reaches the end of the RIFF chunk region declared by the header's size field. That bound (bytes remaining in the chunk stream) must survive a `ClearAndSkip` that re-anchors the buffer, so the variant carries it: **`WebpPastHeader(usize)`**. Without this, a valid WebP with no EXIF would loop `Need` until the driver hit EOF and surface a read *error* instead of a clean `Error::ExifNotFound`.

## File Structure

- **Create** `src/webp.rs` — pure RIFF chunk walker; extracts the `EXIF` chunk payload. One responsibility: WebP byte structure → EXIF slice. Self-contained unit tests.
- **Modify** `src/error.rs` — add `MalformedKind::WebpChunk`.
- **Modify** `src/parser.rs` — add `ParsingState::WebpPastHeader(usize)` (+ `Display`); extend small-file EOF tolerance from `Png` to `Png | Webp`.
- **Modify** `src/file.rs` — add `MediaMimeImage::Webp`, `check_webp`, wire into MIME detection.
- **Modify** `src/exif.rs` — dispatch `MediaMimeImage::Webp` to `webp::extract_exif`; handle the new `ParsingState` arm.
- **Modify** `src/lib.rs` — declare `mod webp;`.
- **Modify** `README.md`, `CHANGELOG.md` — docs.

Task order keeps the crate compiling and green after every task.

---

### Task 1: Add `MalformedKind::WebpChunk`

**Files:**
- Modify: `src/error.rs:200-207` (enum), `src/error.rs:209-221` (Display), `src/error.rs:287-298` (coverage test)

- [ ] **Step 1: Extend the coverage test to require the new variant**

In `src/error.rs`, add `MalformedKind::WebpChunk` to the array in `malformed_kind_covers_all_structural_units`:

```rust
    #[test]
    fn malformed_kind_covers_all_structural_units() {
        for k in [
            MalformedKind::JpegSegment,
            MalformedKind::TiffHeader,
            MalformedKind::IfdEntry,
            MalformedKind::IsoBmffBox,
            MalformedKind::EbmlElement,
            MalformedKind::PngChunk,
            MalformedKind::WebpChunk,
        ] {
            let _ = format!("{k:?}");
        }
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib error:: 2>&1 | tail -20`
Expected: FAIL — compile error, `no variant named WebpChunk found for enum MalformedKind`.

- [ ] **Step 3: Add the variant and its Display arm**

In the `MalformedKind` enum (after `PngChunk`):

```rust
pub enum MalformedKind {
    JpegSegment,
    TiffHeader,
    IfdEntry,
    IsoBmffBox,
    EbmlElement,
    PngChunk,
    WebpChunk,
}
```

In `impl std::fmt::Display for MalformedKind`, add to the match (after the `PngChunk` arm):

```rust
            Self::PngChunk => "png chunk",
            Self::WebpChunk => "webp chunk",
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib error:: 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/error.rs
git commit -m "feat: add MalformedKind::WebpChunk"
```

---

### Task 2: Add `ParsingState::WebpPastHeader(usize)`

**Files:**
- Modify: `src/parser.rs:501-521` (enum + Display)
- Modify: `src/exif.rs:248-253` (state→header match in `extract_exif_range`)

This is plumbing so later tasks compile. `ParsingState` is an exhaustive-matched enum, so every `match` over it must gain an arm. The two known sites are below; if `cargo build` flags another, add `ParsingState::WebpPastHeader(_) => None` (or the context-appropriate arm) there too.

- [ ] **Step 1: Add the enum variant**

In `src/parser.rs`, in `pub(crate) enum ParsingState` (after `PngPastSignature`):

```rust
pub(crate) enum ParsingState {
    TiffHeader(TiffHeader),
    HeifExifSize(usize),
    Cr3ExifSize(usize),
    /// PNG chunk walker has already validated the 8-byte signature.
    PngPastSignature,
    /// WebP RIFF walker has validated the 12-byte header and skipped some
    /// chunks. Carries the number of chunk-stream bytes still remaining
    /// (from the resumed buffer's start) so the walker can stop cleanly at
    /// the end of the RIFF chunk region — WebP has no terminator chunk.
    WebpPastHeader(usize),
}
```

- [ ] **Step 2: Add the Display arm**

In `impl Display for ParsingState`, in the match:

```rust
            ParsingState::PngPastSignature => f.write_str("ParsingState: PngPastSignature"),
            ParsingState::WebpPastHeader(n) => {
                Display::fmt(&format!("ParsingState: WebpPastHeader({n})"), f)
            }
```

- [ ] **Step 3: Add the `extract_exif_range` arm**

In `src/exif.rs`, in `extract_exif_range`, the `state.and_then(|x| match x { ... })` block (around line 248):

```rust
    let header = state.and_then(|x| match x {
        ParsingState::TiffHeader(h) => Some(h),
        ParsingState::HeifExifSize(_) => None,
        ParsingState::Cr3ExifSize(_) => None,
        ParsingState::PngPastSignature => None,
        ParsingState::WebpPastHeader(_) => None,
    });
```

- [ ] **Step 4: Verify the crate still builds**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean (a `WebpPastHeader` never-constructed dead-code note is acceptable until Task 4 wires it).

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs src/exif.rs
git commit -m "feat: add ParsingState::WebpPastHeader(usize)"
```

---

### Task 3: WebP MIME detection

**Files:**
- Modify: `src/file.rs:59-69` (enum), `src/file.rs:91-101` (TryFrom chain), `src/file.rs:231-239` (near `check_png`), `src/file.rs` tests

Adding `MediaMimeImage::Webp` makes the exhaustive match in `exif::extract_exif_with_mime` incomplete — **Task 4 adds that arm**. To keep this task compiling on its own, add a temporary `unreachable!` arm here and replace it in Task 4.

- [ ] **Step 1: Write the failing detection test**

In `src/file.rs`, inside `mod v3_tests` (near the bottom, `use super::*;` already present there), add:

```rust
    #[test]
    fn webp_riff_header_detected_as_image() {
        // Minimal RIFF/WEBP header + one chunk header's worth of bytes.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&16u32.to_le_bytes()); // riff size (value irrelevant to detection)
        buf.extend_from_slice(b"WEBP");
        buf.extend_from_slice(b"VP8X");
        buf.extend_from_slice(&[0u8; 4]);
        let res: Result<MediaMime, Error> = buf.as_slice().try_into();
        assert!(matches!(res, Ok(MediaMime::Image(MediaMimeImage::Webp))));
    }

    #[test]
    fn riff_without_webp_fourcc_is_not_webp() {
        // A RIFF/WAVE header must NOT be misdetected as WebP.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(&[0u8; 8]);
        let res: Result<MediaMime, Error> = buf.as_slice().try_into();
        assert!(matches!(res, Err(Error::UnsupportedFormat)));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib file:: 2>&1 | tail -20`
Expected: FAIL — compile error, `no variant named Webp found for enum MediaMimeImage`.

- [ ] **Step 3: Add the enum variant**

In `src/file.rs`, in `pub(crate) enum MediaMimeImage` (after `Png`):

```rust
pub(crate) enum MediaMimeImage {
    Jpeg,
    Heic,
    Heif,
    Avif,
    Tiff,
    Raf,
    Cr3,
    Png,
    Webp,
}
```

- [ ] **Step 4: Add `check_webp` and wire it into the TryFrom chain**

In `src/file.rs`, add near `check_png` (after the `check_png` fn, around line 239):

```rust
fn check_webp(input: &[u8]) -> Result<(), ()> {
    // RIFF container tagged WEBP: "RIFF"(4) + fileSize(4) + "WEBP"(4).
    if input.len() >= 12 && &input[0..4] == b"RIFF" && &input[8..12] == b"WEBP" {
        Ok(())
    } else {
        Err(())
    }
}
```

In `impl TryFrom<&[u8]> for MediaMime`, add a branch after the `check_png` branch:

```rust
        } else if check_png(input).is_ok() {
            MediaMime::Image(MediaMimeImage::Png)
        } else if check_webp(input).is_ok() {
            MediaMime::Image(MediaMimeImage::Webp)
        } else if check_jpeg(input).is_ok() {
```

- [ ] **Step 5: Add the temporary exhaustive-match arm so the crate compiles**

In `src/exif.rs`, in `extract_exif_with_mime`'s match (after the `MediaMimeImage::Png => { ... }` arm, around line 400), add:

```rust
        MediaMimeImage::Webp => {
            // Wired to the real walker in the next task.
            unreachable!("WebP dispatch not yet wired");
        }
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --lib file:: 2>&1 | tail -20`
Expected: PASS (both new tests green).

- [ ] **Step 7: Commit**

```bash
git add src/file.rs src/exif.rs
git commit -m "feat: detect WebP (RIFF/WEBP) as an image MIME"
```

---

### Task 4: WebP chunk walker (`src/webp.rs`)

**Files:**
- Create: `src/webp.rs`
- Modify: `src/lib.rs:365` (add `mod webp;` after `mod png;`)

The walker is the core. It is a pure function tested in isolation via synthetic buffers before any dispatch wiring (Task 5) uses it.

- [ ] **Step 1: Create `src/webp.rs` with the walker and its unit tests**

Create `src/webp.rs` with this exact content:

```rust
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
            let riff_size =
                u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
            (WEBP_HEADER_LEN, riff_size.saturating_sub(4))
        }
    };

    // Preserve the resume bound across `Need` returns (buffer only grows).
    let preserve = |left: usize| Some(ParsingState::WebpPastHeader(left));

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
            return Ok((Some(payload), preserve(stream_left)));
        }

        // Chunk claims more than the RIFF container allows — treat as the end
        // of useful data rather than skipping past EOF.
        if total > stream_left {
            return Ok((None, Some(ParsingState::WebpPastHeader(0))));
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
        // A 3-byte VP8X-ish chunk (odd → 1 pad byte) before EXIF. If padding
        // is miscounted, the EXIF FourCC won't align and extraction fails.
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
        let (exif, _) = extract_exif(
            Some(ParsingState::WebpPastHeader(stream_left)),
            &exif_chunk,
        )
        .unwrap();
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
}
```

- [ ] **Step 2: Declare the module**

In `src/lib.rs`, after `mod png;` (line 365):

```rust
mod png;
mod webp;
```

- [ ] **Step 3: Run the walker unit tests to verify they pass**

Run: `cargo test --lib webp:: 2>&1 | tail -25`
Expected: PASS — all `webp::tests::*` green. (A dead-code note for `extract_exif` is acceptable; Task 5 wires it.)

- [ ] **Step 4: Commit**

```bash
git add src/webp.rs src/lib.rs
git commit -m "feat: add WebP RIFF chunk walker (webp::extract_exif)"
```

---

### Task 5: Wire WebP into the generic EXIF dispatch + EOF tolerance

**Files:**
- Modify: `src/exif.rs:396-400` (replace the temporary `unreachable!` arm)
- Modify: `src/parser.rs` (EOF tolerance: `parse_exif` ~804-807, `parse_image_metadata` ~975, `parse_exif_async` ~1094-1097, `parse_image_metadata_async` ~1146)
- Modify: `src/exif.rs` (add `webp` to imports if needed)
- Test: end-to-end sync test in `src/parser.rs` tests module

- [ ] **Step 1: Write the failing end-to-end test**

In `src/parser.rs`, inside the existing `#[cfg(test)] mod ...` that holds `parse_exif_unified_from_memory_jpg`, add these helpers + test. (They build a synthetic WebP entirely in memory — no sample file needed.)

```rust
    // --- WebP synthetic-buffer helpers (mirrors src/webp.rs test builders) ---
    #[cfg(test)]
    fn webp_minimal_tiff_le() -> Vec<u8> {
        // II + 0x002A + IFD0 offset 8 + IFD0 with one tag (Make="A") + next=0.
        let mut t = Vec::new();
        t.extend_from_slice(b"II");
        t.extend_from_slice(&[0x2a, 0x00]);
        t.extend_from_slice(&[0x08, 0, 0, 0]); // IFD0 at offset 8
        t.extend_from_slice(&[0x01, 0x00]); // 1 entry
        // Tag 0x010F (Make), type 2 (ASCII), count 2, inline value "A\0".
        t.extend_from_slice(&[0x0f, 0x01]); // tag 0x010F
        t.extend_from_slice(&[0x02, 0x00]); // type ASCII
        t.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // count 2
        t.extend_from_slice(b"A\0\0\0"); // value "A\0" padded to 4 bytes
        t.extend_from_slice(&[0, 0, 0, 0]); // next IFD = 0
        t
    }

    #[cfg(test)]
    fn webp_with_exif(tiff: &[u8]) -> Vec<u8> {
        let mut exif_chunk = Vec::new();
        exif_chunk.extend_from_slice(b"EXIF");
        exif_chunk.extend_from_slice(&(tiff.len() as u32).to_le_bytes());
        exif_chunk.extend_from_slice(tiff);
        if tiff.len() % 2 == 1 {
            exif_chunk.push(0);
        }
        let mut vp8x = Vec::new();
        vp8x.extend_from_slice(b"VP8X");
        vp8x.extend_from_slice(&10u32.to_le_bytes());
        vp8x.extend_from_slice(&[0u8; 10]);

        let mut body = Vec::new();
        body.extend_from_slice(b"WEBP");
        body.extend_from_slice(&vp8x);
        body.extend_from_slice(&exif_chunk);

        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn parse_exif_webp_from_memory() {
        let tiff = webp_minimal_tiff_le();
        let buf = webp_with_exif(&tiff);
        let mut parser = MediaParser::new();
        let ms = MediaSource::from_memory(buf).unwrap();
        let iter = parser.parse_exif(ms).unwrap();
        let exif: crate::Exif = iter.into();
        assert_eq!(
            exif.get(crate::ExifTag::Make).and_then(|v| v.as_str()),
            Some("A")
        );
    }

    #[test]
    fn parse_exif_webp_no_exif_returns_not_found() {
        // RIFF/WEBP with only a VP8X chunk (no EXIF).
        let mut vp8x = Vec::new();
        vp8x.extend_from_slice(b"VP8X");
        vp8x.extend_from_slice(&10u32.to_le_bytes());
        vp8x.extend_from_slice(&[0u8; 10]);
        let mut body = Vec::new();
        body.extend_from_slice(b"WEBP");
        body.extend_from_slice(&vp8x);
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
        buf.extend_from_slice(&body);

        let mut parser = MediaParser::new();
        let ms = MediaSource::from_memory(buf).unwrap();
        let res = parser.parse_exif(ms);
        assert!(matches!(res, Err(crate::Error::ExifNotFound)));
    }

    #[test]
    fn parse_exif_webp_streaming_small_file() {
        // Exercises the Png|Webp small-file EOF tolerance on the streaming
        // path: a sub-INIT_BUF_SIZE WebP fully consumed during MIME prefill.
        use std::io::Cursor;
        let tiff = webp_minimal_tiff_le();
        let buf = webp_with_exif(&tiff);
        let mut parser = MediaParser::new();
        let ms = MediaSource::seekable(Cursor::new(buf)).unwrap();
        let iter = parser.parse_exif(ms).unwrap();
        let exif: crate::Exif = iter.into();
        assert_eq!(
            exif.get(crate::ExifTag::Make).and_then(|v| v.as_str()),
            Some("A")
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib parse_exif_webp 2>&1 | tail -30`
Expected: FAIL — `parse_exif_webp_from_memory` panics via the `unreachable!("WebP dispatch not yet wired")` arm from Task 3.

- [ ] **Step 3: Replace the `unreachable!` arm with the real dispatch**

In `src/exif.rs`, in `extract_exif_with_mime`, replace the temporary WebP arm:

```rust
        MediaMimeImage::Webp => crate::webp::extract_exif(state, buf)?,
```

(This returns `(Option<&[u8]>, Option<ParsingState>)`, matching the `let (exif_data, state) = match img_type { ... }` binding — same shape as the `Heic | Heif | Avif` and `Cr3` arms.)

- [ ] **Step 4: Extend the small-file EOF tolerance from Png to Png | Webp**

In `src/parser.rs`, there are four sites that special-case PNG for the "reader exhausted during MIME prefill" case. Update each to also accept WebP.

Site A — `parse_exif` (around line 804):

```rust
                let is_png_or_webp = matches!(
                    ms.mime,
                    crate::file::MediaMime::Image(
                        crate::file::MediaMimeImage::Png | crate::file::MediaMimeImage::Webp
                    )
                );
                match self.fill_buf(&mut ms.reader, INIT_BUF_SIZE) {
                    Ok(_) => {}
                    Err(e)
                        if is_png_or_webp
                            && !self.buffer().is_empty()
                            && e.kind() == io::ErrorKind::UnexpectedEof => {}
                    Err(e) => return Err(e.into()),
                }
```

Site B — `parse_image_metadata` (around line 975): replace

```rust
                let is_png = mime_img == crate::file::MediaMimeImage::Png;
```

with

```rust
                let is_png = matches!(
                    mime_img,
                    crate::file::MediaMimeImage::Png | crate::file::MediaMimeImage::Webp
                );
```

Site C — `parse_exif_async` (around line 1094): mirror Site A, renaming the local to `is_png_or_webp` and matching `Png | Webp`:

```rust
                    let is_png_or_webp = matches!(
                        ms.mime,
                        crate::file::MediaMime::Image(
                            crate::file::MediaMimeImage::Png | crate::file::MediaMimeImage::Webp
                        )
                    );
                    match <Self as AsyncBufParser>::fill_buf(self, &mut ms.reader, INIT_BUF_SIZE)
                        .await
                    {
                        Ok(_) => {}
                        Err(e)
                            if is_png_or_webp
                                && !self.buffer().is_empty()
                                && e.kind() == io::ErrorKind::UnexpectedEof => {}
                        Err(e) => return Err(e.into()),
                    }
```

Site D — `parse_image_metadata_async` (around line 1146): replace

```rust
                    let is_png = mime_img == crate::file::MediaMimeImage::Png;
```

with

```rust
                    let is_png = matches!(
                        mime_img,
                        crate::file::MediaMimeImage::Png | crate::file::MediaMimeImage::Webp
                    );
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib parse_exif_webp 2>&1 | tail -30`
Expected: PASS — all three WebP end-to-end tests green.

- [ ] **Step 6: Run the full lib test suite to check for regressions**

Run: `cargo test --lib 2>&1 | tail -15`
Expected: PASS — no regressions.

- [ ] **Step 7: Commit**

```bash
git add src/exif.rs src/parser.rs
git commit -m "feat: extract EXIF from WebP images"
```

---

### Task 6: Async end-to-end test

**Files:**
- Test: `src/parser.rs` tests module (async test, `#[cfg(feature = "tokio")]`)

- [ ] **Step 1: Write the async end-to-end test**

In `src/parser.rs`, near the existing `media_parser_parse_exif_async` test, add. It reuses the `webp_minimal_tiff_le` / `webp_with_exif` helpers from Task 5.

```rust
    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn parse_exif_webp_async_from_memory() {
        use crate::AsyncMediaSource;
        let tiff = webp_minimal_tiff_le();
        let buf = webp_with_exif(&tiff);
        let mut parser = MediaParser::new();
        let ms = AsyncMediaSource::from_memory(buf).unwrap();
        let iter = parser.parse_exif_async(ms).await.unwrap();
        let exif: crate::Exif = iter.into();
        assert_eq!(
            exif.get(crate::ExifTag::Make).and_then(|v| v.as_str()),
            Some("A")
        );
    }
```

Note: `AsyncMediaSource::from_memory` is synchronous (returns `crate::Result<Self>`, no `.await`); only `parse_exif_async` is awaited. This matches the existing `AsyncMediaSource::from_memory(raw).unwrap()` usage in the test module.

- [ ] **Step 2: Run the async test to verify it passes**

Run: `cargo test --lib --features tokio parse_exif_webp_async 2>&1 | tail -20`
Expected: PASS. (No production code change needed — the async generic path already routes through `extract_exif_with_mime`. If it fails to compile on the `from_memory` call shape, fix per the note in Step 1.)

- [ ] **Step 3: Commit**

```bash
git add src/parser.rs
git commit -m "test: WebP EXIF via async parse path"
```

---

### Task 7: Documentation

**Files:**
- Modify: `README.md:36` (Supported File Types)
- Modify: `CHANGELOG.md` (top entry)

- [ ] **Step 1: Update the README supported-types line**

In `README.md`, change the Image line (line 36) from:

```markdown
- **Image**: JPEG, PNG, HEIC/HEIF, AVIF, TIFF, Phase One IIQ, Fujifilm RAF, Canon CR3
```

to:

```markdown
- **Image**: JPEG, PNG, WebP, HEIC/HEIF, AVIF, TIFF, Phase One IIQ, Fujifilm RAF, Canon CR3
```

- [ ] **Step 2: Add a CHANGELOG entry**

Open `CHANGELOG.md` and read the top of the file to match its existing format (headings, date style). Add a new entry at the top describing the change, following whatever "Unreleased"/version convention is already used there. The entry text:

```markdown
- feat: add WebP (RIFF/WEBP) image support — extracts EXIF/GPS from the
  `EXIF` chunk via the unified `parse_exif` / `read_exif` (sync + async) APIs.
```

Place it under the existing "Unreleased" section if present, otherwise create one above the most recent released version, matching the file's style.

- [ ] **Step 3: Verify formatting and the full build**

Run: `cargo fmt --check && cargo clippy --all-targets 2>&1 | tail -15`
Expected: `cargo fmt --check` produces no output (clean); clippy reports no new warnings for `src/webp.rs` or the modified files.

- [ ] **Step 4: Commit**

```bash
git add README.md CHANGELOG.md
git commit -m "docs: document WebP EXIF support"
```

---

## Final Verification

- [ ] **Full test suite (default features)**

Run: `cargo test 2>&1 | tail -20`
Expected: all green.

- [ ] **Full test suite (with tokio)**

Run: `cargo test --features tokio 2>&1 | tail -20`
Expected: all green, including `parse_exif_webp_async_from_memory`.

- [ ] **Format + lint**

Run: `cargo fmt --check && cargo clippy --all-targets --all-features 2>&1 | tail -20`
Expected: clean.

## Self-Review Notes (coverage against spec)

- Spec §4.1 (file.rs MIME) → Task 3.
- Spec §4.2 (webp.rs walker) → Task 4. Walker returns `(Option<&[u8]>, Option<ParsingState>)`, strips `Exif\0\0`, handles Need/ClearAndSkip/Failed, odd-size padding, overflow guard.
- Spec §4.3 (parser.rs ParsingState + EOF tolerance) → Task 2 (variant) + Task 5 Step 4 (tolerance). Variant carries `usize` (refinement, documented above).
- Spec §4.4 (exif.rs dispatch + range match) → Task 2 Step 3 (range match) + Task 5 Step 3 (dispatch).
- Spec §4.5 (lib.rs mod) → Task 4 Step 2.
- Spec §4.6 (MalformedKind::WebpChunk) → Task 1.
- Spec §4.7 (docs + tests) → Task 5 (sync e2e), Task 6 (async e2e), Task 7 (README/CHANGELOG).
- Spec §5 (error handling) → covered by walker (Task 4) + `range_to_iter` (existing) producing `ExifNotFound`, verified by `parse_exif_webp_no_exif_returns_not_found` (Task 5).
```

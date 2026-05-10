# PNG P2 — Pure chunk parser

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `src/png.rs` containing the pure-function PNG chunk walker `extract_chunks(buf) -> Result<PngParseOut, ParsingErrorState>`. Returns `PngExifSource` (the EXIF byte source) + `Vec<(String, String)>` (the tEXt chunks). No integration with `MediaParser` yet — that lands in P4.

**Architecture:** Single `extract_chunks` function takes a buffer and walks chunks: signature → loop on (length, type) headers → handle `eXIf` (return `PngExifSource::EXif(Range)`), `tEXt` (capture as `(key, value)` pair, also detect legacy "Raw profile type *" but defer hex-decoding to P5), `IEND` (stop), unknown (skip via `Skip(n)`). Streaming-friendly: returns `Need(n)` / `Skip(n)` errors for the parse loop to drive I/O. Stateless — no `ParsingState` threading.

**Tech Stack:** `nom` for byte parsing, `std::ops::Range`, `crate::error::ParsingError`, `crate::error::ParsingErrorState`.

---

## File Structure

| File | Change |
|---|---|
| `src/png.rs` | NEW. The whole module — `PngParseOut`, `PngExifSource`, `extract_chunks`, plus pure-function unit tests in a `mod tests` block at the bottom. |
| `src/lib.rs` | Add `mod png;` declaration (private). |

---

## Task 2.1: Create `src/png.rs` skeleton with types

**Files:**
- Create: `src/png.rs`
- Modify: `src/lib.rs` (add `mod png;`)

- [ ] **Step 1: Create the module file with public types**

Create `src/png.rs`:

```rust
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
```

- [ ] **Step 2: Add `mod png;` declaration**

Edit `src/lib.rs` — find the `mod` declarations (around line 316) and add `mod png;` near `mod jpeg;`:

```rust
mod jpeg;
mod mov;
mod parser;
#[cfg(feature = "tokio")]
mod parser_async;
mod png;  // NEW
mod raf;
mod slice;
```

- [ ] **Step 3: Verify compile**

Run: `cargo check --all-features`
Expected: clean. (Module exists with no public items used yet.)

- [ ] **Step 4: Commit**

```bash
git add src/png.rs src/lib.rs
git commit -m "$(cat <<'EOF'
feat: scaffold src/png.rs module with PngParseOut + PngExifSource

Empty module with the public-types shape that the chunk-parser
function will populate. No actual parsing logic yet — that lands
in subsequent commits in this phase.
EOF
)"
```

---

## Task 2.2: Implement `extract_chunks` signature check + chunk header loop

**Files:**
- Modify: `src/png.rs`

- [ ] **Step 1: Add the function with signature/header validation only**

Edit `src/png.rs` — insert after the constants and before `mod tests`:

```rust
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
            // Other chunks handled in subsequent commits.
            _ => {
                // For now, skip everything that isn't IEND.
                let total = 8 + length as usize + 4; // header + data + CRC
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
```

(Note: the `let _ = text_total;` is a temporary suppression. Real use lands in Task 2.4.)

- [ ] **Step 2: Add a unit test for signature validation**

Edit `src/png.rs::tests` — insert:

```rust
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
```

- [ ] **Step 3: Run tests**

Run: `cargo test --all-features png::tests`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/png.rs
git commit -m "$(cat <<'EOF'
feat: extract_chunks signature + header loop

Validates the PNG 8-byte signature, walks chunk headers, returns on
IEND. Unknown chunks are skipped via ParsingError::ClearAndSkip
(letting the I/O driver seek past them). No metadata extraction
yet — eXIf and tEXt handling come in subsequent commits.
EOF
)"
```

---

## Task 2.3: Implement `eXIf` chunk extraction

**Files:**
- Modify: `src/png.rs`

- [ ] **Step 1: Add `eXIf` arm to the match**

Edit `src/png.rs` — replace the `_ => { ... skip ... }` arm with:

```rust
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
```

- [ ] **Step 2: Add a test fixture builder helper**

Edit `src/png.rs::tests` — add a chunk-builder helper:

```rust
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
        out.extend_from_slice(&build_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]));
        for c in chunks {
            out.extend_from_slice(c);
        }
        out.extend_from_slice(&build_chunk(b"IEND", &[]));
        out
    }
```

- [ ] **Step 3: Add a test for `eXIf` extraction**

```rust
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test --all-features png::tests::extract_chunks`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/png.rs
git commit -m "$(cat <<'EOF'
feat: extract_chunks recognizes eXIf chunk

When an eXIf chunk is encountered, captures its byte range as
PngExifSource::EXif(Range). Range is into the input buffer — no
allocation; the caller can use bytes::Bytes::slice for zero-copy.

eXIf has the highest priority: when both eXIf and a legacy
Raw-profile-type tEXt are present, eXIf wins. Legacy handling
lands in phase 5.
EOF
)"
```

---

## Task 2.4: Implement `tEXt` chunk extraction (no legacy detection yet)

**Files:**
- Modify: `src/png.rs`

- [ ] **Step 1: Add Latin-1 decode helper**

Edit `src/png.rs` — insert near the constants (or above `extract_chunks`):

```rust
/// Decode bytes as Latin-1 into a `String`. Infallible — every Latin-1
/// byte maps to a Unicode code point (U+0000..U+00FF). Per PNG spec, `tEXt`
/// chunks use Latin-1 encoding; we do not sniff for UTF-8.
fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}
```

- [ ] **Step 2: Add `tEXt` handling in the match**

Edit `src/png.rs` — update the match in `extract_chunks` to add a `tEXt` arm:

```rust
        match ctype {
            b"IEND" => break,
            b"eXIf" => { /* unchanged from Task 2.3 */ }
            b"tEXt" => {
                if length > MAX_TEXT_CHUNK_SIZE {
                    // Defensive: skip oversized chunks.
                    let total = 8 + length as usize + 4;
                    let remaining = buf.len() - cursor;
                    if total > remaining {
                        return Err(ParsingErrorState::new(
                            ParsingError::ClearAndSkip(total - remaining),
                            None,
                        ));
                    }
                    cursor += total;
                    continue;
                }
                let total = 8 + length as usize + 4;
                let remaining = buf.len() - cursor;
                if total > remaining {
                    return Err(ParsingErrorState::new(
                        ParsingError::Need(total - remaining),
                        None,
                    ));
                }
                let data = &buf[cursor + 8..cursor + 8 + length as usize];
                // tEXt format: Latin-1 keyword + 0x00 + Latin-1 text
                if let Some(nul_pos) = data.iter().position(|&b| b == 0) {
                    let key = decode_latin1(&data[..nul_pos]);
                    let value = decode_latin1(&data[nul_pos + 1..]);
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
            _ => { /* unchanged from Task 2.3 */ }
        }
```

- [ ] **Step 3: Remove the temporary `let _ = text_total;` suppression**

Edit `src/png.rs` — find `let _ = text_total;` and delete it; `text_total` is now used.

- [ ] **Step 4: Add tests**

Edit `src/png.rs::tests`:

```rust
    #[test]
    fn extract_chunks_with_text() {
        let mut text_data = Vec::new();
        text_data.extend_from_slice(b"Title");
        text_data.push(0);
        text_data.extend_from_slice(b"Hello world");
        let chunks = vec![build_chunk(b"tEXt", &text_data)];
        let buf = build_png_with_chunks(&chunks);
        let result = extract_chunks(&buf).unwrap();
        assert!(result.exif.is_none());
        assert_eq!(result.text_chunks.len(), 1);
        assert_eq!(result.text_chunks[0].0, "Title");
        assert_eq!(result.text_chunks[0].1, "Hello world");
    }

    #[test]
    fn extract_chunks_text_duplicate_keys() {
        let mut t1 = Vec::new(); t1.extend_from_slice(b"Comment"); t1.push(0); t1.extend_from_slice(b"first");
        let mut t2 = Vec::new(); t2.extend_from_slice(b"Comment"); t2.push(0); t2.extend_from_slice(b"second");
        let chunks = vec![build_chunk(b"tEXt", &t1), build_chunk(b"tEXt", &t2)];
        let buf = build_png_with_chunks(&chunks);
        let result = extract_chunks(&buf).unwrap();
        assert_eq!(result.text_chunks.len(), 2);
        assert_eq!(result.text_chunks[0], ("Comment".into(), "first".into()));
        assert_eq!(result.text_chunks[1], ("Comment".into(), "second".into()));
    }

    #[test]
    fn extract_chunks_text_no_nul_separator() {
        // Malformed tEXt with no NUL byte — should be silently skipped.
        let chunks = vec![build_chunk(b"tEXt", b"NoNulSeparator")];
        let buf = build_png_with_chunks(&chunks);
        let result = extract_chunks(&buf).unwrap();
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
        let result = extract_chunks(&buf).unwrap();
        assert_eq!(result.text_chunks[0].1, "café");
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo test --all-features png::tests`
Expected: 8 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/png.rs
git commit -m "$(cat <<'EOF'
feat: extract_chunks captures tEXt key/value pairs

tEXt chunks split on the first 0x00 byte; both halves Latin-1
decoded into String (infallible — every Latin-1 byte maps to a
Unicode code point). Duplicate keys preserved (PNG spec permits
this). Malformed tEXt (no NUL) silently skipped.

Defensive bounds:
- MAX_TEXT_CHUNK_SIZE = 1 MiB rejects oversized individual chunks.
- MAX_TEXT_CHUNKS_TOTAL = 16 MiB caps cumulative captured size.

Legacy "Raw profile type *" recognition (hex-decode → EXIF blob)
deferred to phase 5. For now, those entries appear in text_chunks
verbatim like any other tEXt.
EOF
)"
```

---

## Task 2.5: Streaming behavior tests (`Need` / `Skip`)

**Files:**
- Modify: `src/png.rs::tests`

- [ ] **Step 1: Add tests verifying `Need(n)` is returned mid-chunk**

```rust
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

        let err = extract_chunks(&buf).unwrap_err();
        match err.err {
            ParsingError::Need(n) => assert!(n >= 100),
            other => panic!("expected Need(>=100), got {other:?}"),
        }
    }

    #[test]
    fn extract_chunks_skips_large_idat() {
        // IDAT chunk declaring a 50_000-byte body that is NOT in the buffer —
        // should produce ParsingError::ClearAndSkip.
        let mut buf = Vec::new();
        buf.extend_from_slice(PNG_SIGNATURE);
        buf.extend_from_slice(&build_chunk(b"IHDR", &[0; 13]));
        // IDAT header only, claiming 50_000 bytes
        buf.extend_from_slice(&50_000u32.to_be_bytes());
        buf.extend_from_slice(b"IDAT");

        let err = extract_chunks(&buf).unwrap_err();
        match err.err {
            ParsingError::ClearAndSkip(n) => assert!(n >= 50_000),
            other => panic!("expected ClearAndSkip(>=50_000), got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test --all-features png::tests`
Expected: all png tests pass (10 total now).

- [ ] **Step 3: Commit**

```bash
git add src/png.rs
git commit -m "$(cat <<'EOF'
test: streaming Need/Skip behavior of extract_chunks

Verifies:
- Truncated inside eXIf body → ParsingError::Need
- Truncated before unknown chunk body → ParsingError::ClearAndSkip
  (large IDAT chunks are streaming-friendly: never enter the parse
  buffer, the I/O driver seeks past them)
EOF
)"
```

---

## Task 2.6: Defensive bounds tests

**Files:**
- Modify: `src/png.rs::tests`

- [ ] **Step 1: Add tests for `MAX_TEXT_CHUNK_SIZE` and `MAX_TEXT_CHUNKS_TOTAL`**

```rust
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

        let err = extract_chunks(&buf).unwrap_err();
        assert!(matches!(err.err, ParsingError::ClearAndSkip(_)));
    }

    #[test]
    fn extract_chunks_malicious_text_length_max_u32_does_not_panic() {
        // tEXt with length = u32::MAX. Must not allocate 4 GB or panic.
        let mut buf = Vec::new();
        buf.extend_from_slice(PNG_SIGNATURE);
        buf.extend_from_slice(&build_chunk(b"IHDR", &[0; 13]));
        buf.extend_from_slice(&u32::MAX.to_be_bytes());
        buf.extend_from_slice(b"tEXt");

        let err = extract_chunks(&buf).unwrap_err();
        // Either Need or ClearAndSkip — both acceptable; never panic.
        match err.err {
            ParsingError::Need(_) | ParsingError::ClearAndSkip(_) => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test --all-features png::tests`
Expected: all png tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/png.rs
git commit -m "$(cat <<'EOF'
test: defensive bounds in extract_chunks

Crafted inputs (oversized tEXt, u32::MAX length declaration) must
not allocate gigabytes or panic. Verifies the size-cap branches in
the tEXt arm trigger ClearAndSkip / Need without entering the
allocation path.
EOF
)"
```

---

## Task 2.7: Final verification of phase 2

- [ ] **Step 1: Full test suite green**

Run: `cargo test --all-features`
Expected: green; png module has ~12 tests.

- [ ] **Step 2: Format clean**

Run: `cargo fmt --check`
Expected: clean.

- [ ] **Step 3: Self-check exit criterion**

> `src/png.rs` exists with `extract_chunks` + `PngParseOut` + `PngExifSource`. ✓
> Unit tests cover `eXIf`-only, `tEXt`-only, IDAT skip, IEND termination, defensive bounds, `Need`/`Skip` returns. ✓
> No integration with parser dispatch yet. ✓
> `cargo test --all-features` green. ✓

Phase 2 complete. Proceed to P3.

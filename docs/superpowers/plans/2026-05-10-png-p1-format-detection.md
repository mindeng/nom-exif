# PNG P1 — Format detection + dispatch stub

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `MediaMimeImage::Png` to the mime detection enum, recognize the PNG 8-byte signature, and add a temporary `Err(ExifNotFound)` short-circuit in `parse_exif_iter` so the new variant compiles. Phase 4 replaces the short-circuit with the real `parse_png_exif_iter`.

**Architecture:** Single `MediaMimeImage::Png` variant. Signature check `\x89PNG\r\n\x1a\n` added to `MediaMime::try_from` in `src/file.rs`, placed *after* TIFF check (defensive ordering). The variant must be added to the exhaustive `match` in `extract_exif_with_mime` to keep the build green — at this phase, return `Err(ExifNotFound)` (replaced by special-case dispatch in P4).

**Tech Stack:** `nom`, signature byte matching.

---

## File Structure

| File | Change |
|---|---|
| `src/file.rs` | Add `MediaMimeImage::Png` variant; add PNG signature detection in `MediaMime::try_from`; add mime test fixture. |
| `src/exif.rs` | Add `MediaMimeImage::Png =>` arm in `extract_exif_with_mime`'s `match`, returning `Err(ParsingError::Failed(...))` stub. Add a temporary short-circuit in `parse_exif_iter` (sync + async) — but actually, since the dispatch happens inside `extract_exif_with_mime`, the stub there is enough. *Do NOT add the `if Png { return ... }` guard at the top of `parse_exif_iter` yet — the spec phase 1 calls for it but a cleaner design is to handle it inside `extract_exif_with_mime`.* See task notes. |
| `testdata/exif.png` | NEW — minimal placeholder PNG (just signature + IHDR + IEND, no metadata). For format-detection tests only. Real metadata-bearing fixtures land in P4/P5. |

---

## Task 1.1: Generate placeholder `testdata/exif.png`

**Files:**
- Create: `testdata/exif.png`

- [ ] **Step 1: Write a small Rust binary or shell script to emit a minimal valid PNG**

Run this Rust one-liner from the repo root (creates a 1×1 pixel PNG with no metadata):

```bash
cat > /tmp/make_png.rs << 'EOF'
use std::io::Write;
fn main() {
    let mut out = std::fs::File::create("testdata/exif.png").unwrap();
    // PNG signature
    out.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
    // IHDR: 1x1 grayscale 8-bit
    let ihdr_data = b"\x00\x00\x00\x01\x00\x00\x00\x01\x08\x00\x00\x00\x00";
    write_chunk(&mut out, b"IHDR", ihdr_data);
    // IDAT: tiny zlib-compressed grayscale pixel
    let idat_data = &[0x78, 0x9c, 0x62, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x01];
    write_chunk(&mut out, b"IDAT", idat_data);
    // IEND
    write_chunk(&mut out, b"IEND", b"");
}
fn write_chunk(out: &mut std::fs::File, ctype: &[u8; 4], data: &[u8]) {
    let len = (data.len() as u32).to_be_bytes();
    out.write_all(&len).unwrap();
    out.write_all(ctype).unwrap();
    out.write_all(data).unwrap();
    // CRC32 — we write zero. Spec says we don't validate, so it's fine.
    out.write_all(&[0, 0, 0, 0]).unwrap();
}
EOF
rustc /tmp/make_png.rs -o /tmp/make_png && /tmp/make_png
```

- [ ] **Step 2: Verify the file exists and is recognizable as PNG**

Run: `file testdata/exif.png && ls -la testdata/exif.png`
Expected: file recognized as PNG, size ~50 bytes.

- [ ] **Step 3: Run the existing mime detection on it (will fail until task 1.2)**

Run: `cargo test --all-features mime -- --nocapture 2>&1 | tail -5`
Expected: existing mime tests still pass; no test for PNG yet.

- [ ] **Step 4: Commit**

```bash
git add testdata/exif.png
git commit -m "$(cat <<'EOF'
test: add minimal testdata/exif.png placeholder

A 1x1 grayscale PNG with no metadata, used for mime-detection tests.
Real metadata-bearing PNG fixtures (with eXIf and tEXt chunks) are
generated programmatically by tests/png_fixtures.rs in later phases.

CRCs in the placeholder are zeroed; nom-exif's PNG parser does not
validate CRCs (consistent with how JPEG markers and HEIC boxes are
handled today).
EOF
)"
```

---

## Task 1.2: Add `MediaMimeImage::Png` and PNG signature detection

**Files:**
- Modify: `src/file.rs`

- [ ] **Step 1: Add the new enum variant**

Edit `src/file.rs` — find `pub(crate) enum MediaMimeImage` (around line 60) and add `Png` to the variants:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum MediaMimeImage {
    Jpeg,
    Heic,
    Heif,
    Avif,
    Tiff,
    Raf,
    Cr3,
    Png,  // NEW
}
```

- [ ] **Step 2: Add PNG signature detection in `MediaMime::try_from`**

Edit `src/file.rs` — find the `impl TryFrom<&[u8]> for MediaMime` block (around line 79). Add the PNG signature check **after** the TIFF check, **before** the JPEG check:

```rust
impl TryFrom<&[u8]> for MediaMime {
    type Error = crate::Error;
    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        let mime = if let Ok(x) = parse_bmff_mime(input) {
            x
        } else if let Ok(x) = get_ebml_doc_type(input) {
            if x == "webm" {
                MediaMime::Track(MediaMimeTrack::Webm)
            } else {
                MediaMime::Track(MediaMimeTrack::Matroska)
            }
        } else if TiffHeader::parse(input).is_ok() {
            MediaMime::Image(MediaMimeImage::Tiff)
        } else if check_png(input).is_ok() {
            MediaMime::Image(MediaMimeImage::Png)
        } else if check_jpeg(input).is_ok() {
            MediaMime::Image(MediaMimeImage::Jpeg)
        } else if RafInfo::check(input).is_ok() {
            MediaMime::Image(MediaMimeImage::Raf)
        } else {
            return Err(crate::Error::UnsupportedFormat);
        };

        Ok(mime)
    }
}
```

- [ ] **Step 3: Add the `check_png` helper**

Edit `src/file.rs` — add the helper near the other `check_*` helpers (after `parse_bmff_mime`, before `get_ftyp_and_major_brand`):

```rust
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

fn check_png(input: &[u8]) -> Result<(), ()> {
    if input.len() >= PNG_SIGNATURE.len() && &input[..PNG_SIGNATURE.len()] == PNG_SIGNATURE {
        Ok(())
    } else {
        Err(())
    }
}
```

- [ ] **Step 4: Add a mime test for PNG**

Edit `src/file.rs` — find the `#[test_case]` mime tests (around line 249) and add:

```rust
    #[test_case("exif.png", Image(Png))]
```

(Place it alphabetically among the other `#[test_case]` lines.)

- [ ] **Step 5: Run mime tests**

Run: `cargo test --all-features mime`
Expected: existing mime tests pass; new `mime::exif.png` case passes.

- [ ] **Step 6: Commit**

```bash
git add src/file.rs
git commit -m "$(cat <<'EOF'
feat: detect PNG files by 8-byte signature

Adds MediaMimeImage::Png variant and signature check
("\x89PNG\r\n\x1a\n") in MediaMime::try_from. The check sits after
TIFF detection (defensive ordering — no actual collision exists
since PNG/TIFF signatures differ in the first byte).

EXIF extraction for PNG is not yet wired up; that lands in phase 4.
This commit only makes the mime layer recognize the format.
EOF
)"
```

---

## Task 1.3: Add `MediaMimeImage::Png` arm to `extract_exif_with_mime` (stub)

**Files:**
- Modify: `src/exif.rs`

- [ ] **Step 1: Locate the `extract_exif_with_mime` match**

Run: `grep -n 'fn extract_exif_with_mime' src/exif.rs`
Expected: one hit (around line 255).

- [ ] **Step 2: Add the `Png` match arm**

Edit `src/exif.rs` — in the match expression inside `extract_exif_with_mime`, add the `Png` arm. Find the existing match (around lines 261-307) and add:

```rust
        MediaMimeImage::Png => {
            // Phase 1 stub: PNG dispatch lands in phase 4 via a
            // special-cased path inside `parse_exif_iter` (peer to
            // CR3). This arm exists only so the match stays
            // exhaustive. Phase 4 routes around it before this code
            // is reached.
            return Err(ParsingErrorState::new(
                ParsingError::Failed("PNG: parse_exif_iter dispatch missing (phase 1 stub)".into()),
                None,
            ));
        }
```

(Place it after the existing arms.)

- [ ] **Step 3: Verify build**

Run: `cargo build --all-features`
Expected: clean compile.

- [ ] **Step 4: Verify the existing test in `parser.rs` doesn't crash on `exif.png`**

Run: `cargo test --all-features parse_media -- --nocapture 2>&1 | tail`
Expected: existing tests pass; the placeholder `exif.png` fixture is NOT yet in the `parse_media` test list (we'll add it later when the real implementation is in place).

- [ ] **Step 5: Commit**

```bash
git add src/exif.rs
git commit -m "$(cat <<'EOF'
feat: add PNG arm to extract_exif_with_mime (stub)

Returns ParsingError::Failed at this stage. Phase 4 replaces this
with a special-cased dispatch at the top of parse_exif_iter (peer
to the existing CR3 special case), so this arm is unreachable in
the final design. It exists now only to satisfy the exhaustiveness
check in the match.
EOF
)"
```

---

## Task 1.4: Final verification of phase 1

- [ ] **Step 1: Full test suite green**

Run: `cargo test --all-features`
Expected: green.

- [ ] **Step 2: Format clean**

Run: `cargo fmt --check`
Expected: clean.

- [ ] **Step 3: Self-check exit criterion**

> `MediaMimeImage::Png` variant added. ✓
> PNG signature detected. ✓
> Mime test fixture passes. ✓
> `cargo test --all-features` green. ✓

Phase 1 complete. Proceed to P2.

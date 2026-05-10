# PNG P4 — `parse_png_exif_iter` + `parse_image_metadata` integration (eXIf path only)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire PNG into the EXIF pipeline. `parse_exif` on PNG returns `Ok(ExifIter)` (eXIf path) or `Err(ExifNotFound)` (no eXIf, no legacy yet — legacy lands in P5). New `MediaParser::parse_image_metadata` returns `ImageMetadata { exif, format }` for any image input. Test PNG fixtures (`exif.png`, `text-only.png`) generated programmatically.

**Architecture:** A new `parse_png_exif_iter` in `src/exif.rs` dispatches at the *top* of `parse_exif_iter` (peer to the existing CR3 special-case), calls `parser.load_and_parse(reader, skip, png::extract_chunks)`, then materializes `PngExifSource::EXif(Range)` into an `ExifIter` via the existing `share_buf` zero-copy path. New `parse_png_full` returns the `ImageMetadata` shape used by `parse_image_metadata`.

**Tech Stack:** `bytes::Bytes`, `nom`, existing `parse_exif_iter` infrastructure, `tokio` (async).

---

## File Structure

| File | Change |
|---|---|
| `src/exif.rs` | Add `parse_png_exif_iter` helper; add `if Png { return parse_png_exif_iter(...) }` short-circuit at the top of `parse_exif_iter` (sync + async). Replaces the P1 stub in the `extract_exif_with_mime` match — that arm becomes unreachable; convert to `unreachable!()` or keep it as the safety net returning `ExifNotFound`. |
| `src/parser.rs` | Add `MediaParser::parse_image_metadata<R: Read>(MediaSource<R>)` method. For PNG, calls a new `parse_png_full` helper; for non-PNG, falls back to existing `parse_exif_iter` and wraps the result. |
| `src/parser_async.rs` | Add `parse_image_metadata_async<R: AsyncRead>` mirroring the sync method. |
| `tests/png_fixtures.rs` | NEW — programmatic fixture builders for `exif.png` (eXIf + tEXt) and `text-only.png` (tEXt only). Used as a helper module for integration tests. |
| `tests/png.rs` | NEW — integration tests covering all entry points × all fixtures × file/memory/async. |

---

## Task 4.1: Build PNG fixture-generation helper

**Files:**
- Create: `tests/png_fixtures.rs`

- [ ] **Step 1: Create the fixture builder module**

Create `tests/png_fixtures.rs`:

```rust
//! PNG fixture builders for integration tests. Programmatically
//! generates minimal valid PNG byte sequences with specified chunk
//! contents.
//!
//! CRCs are written as zero — nom-exif's PNG parser does not validate
//! CRCs (consistent with how JPEG markers / HEIC boxes are handled).

#![allow(dead_code)]

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Build a single PNG chunk: length:4 + type:4 + data + crc:4 (zeros).
pub fn build_chunk(ctype: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ctype);
    out.extend_from_slice(data);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

/// Minimal 1×1 grayscale IHDR.
pub fn ihdr_minimal() -> Vec<u8> {
    build_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0])
}

/// Empty IEND.
pub fn iend() -> Vec<u8> {
    build_chunk(b"IEND", &[])
}

/// Tiny IDAT — gives the PNG some image data so it's not "header only".
pub fn idat_tiny() -> Vec<u8> {
    build_chunk(b"IDAT", &[0x78, 0x9c, 0x62, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x01])
}

/// Build an `eXIf` chunk wrapping the given TIFF bytes (no extra header).
pub fn exif_chunk(tiff: &[u8]) -> Vec<u8> {
    build_chunk(b"eXIf", tiff)
}

/// Build a `tEXt` chunk for the given (key, value).
pub fn text_chunk(key: &str, value: &str) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(key.as_bytes());
    data.push(0);
    data.extend_from_slice(value.as_bytes());
    build_chunk(b"tEXt", &data)
}

/// Compose a complete PNG buffer: signature + IHDR + chunks + IDAT + IEND.
/// The order is convention-following: ancillary chunks before IDAT.
pub fn build_png(ancillary: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(PNG_SIGNATURE);
    out.extend_from_slice(&ihdr_minimal());
    for c in ancillary {
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&idat_tiny());
    out.extend_from_slice(&iend());
    out
}

/// Extract the TIFF bytes from a JPEG APP1 segment in `testdata/exif.jpg`.
/// We piggy-back on the existing test fixture to get a real-world EXIF
/// blob without hand-crafting one.
pub fn tiff_from_jpeg_fixture() -> Vec<u8> {
    let raw = std::fs::read("testdata/exif.jpg").expect("testdata/exif.jpg missing");
    // Walk JPEG to find APP1 ("Exif\0\0" + TIFF).
    let mut i = 2; // skip SOI
    while i + 4 < raw.len() {
        if raw[i] != 0xFF {
            break;
        }
        let marker = raw[i + 1];
        let seg_len = u16::from_be_bytes([raw[i + 2], raw[i + 3]]) as usize;
        if marker == 0xE1 {
            // APP1: payload starts at i+4. Check "Exif\0\0" prefix.
            let payload = &raw[i + 4..i + 2 + seg_len];
            if payload.starts_with(b"Exif\x00\x00") {
                return payload[6..].to_vec();
            }
        }
        i += 2 + seg_len;
    }
    panic!("could not locate APP1/Exif segment in testdata/exif.jpg");
}
```

- [ ] **Step 2: Verify the helper compiles as a test module**

Run: `cargo test --all-features --test png_fixtures 2>&1 | head -10`
Expected: compiles (no `#[test]` functions, so "0 passed" is OK).

Note: Cargo treats every `.rs` file under `tests/` as an integration test binary. The `png_fixtures.rs` file as it stands has no `#[test]` items, so it'll just compile cleanly.

If we want it to be a *module* shared by other test files (rather than a standalone binary), we need to put it under `tests/common/` or use `#[path = "..."]` includes. For now, leave it as standalone and have other test files include it via `#[path = "png_fixtures.rs"] mod png_fixtures;`. (This is the standard Rust idiom.)

- [ ] **Step 3: Commit**

```bash
git add tests/png_fixtures.rs
git commit -m "$(cat <<'EOF'
test: add PNG fixture builders for integration tests

Programmatic PNG byte-stream builders: signature + chunks + IEND.
Also includes a helper to extract a real TIFF/EXIF blob from
testdata/exif.jpg's APP1 segment so we can build PNGs with real
EXIF data without hand-crafting it.

CRCs are zeroed — nom-exif's PNG parser doesn't verify CRCs
(consistent with JPEG marker handling).

This module is shared across integration tests via
`#[path = "png_fixtures.rs"] mod png_fixtures;` includes.
EOF
)"
```

---

## Task 4.2: Add `parse_png_exif_iter` helper in `src/exif.rs`

**Files:**
- Modify: `src/exif.rs`

- [ ] **Step 1: Add the helper after `parse_cr3_exif_iter`**

Edit `src/exif.rs` — insert this function after `parse_cr3_exif_iter` (around line 162):

```rust
/// Special parser for PNG files: walks the chunk stream via
/// `png::extract_chunks`, materializes the resulting [`PngExifSource`]
/// into an [`ExifIter`]. Phase 4: handles only the `eXIf` chunk path
/// (legacy `Raw profile type *` decoding lands in phase 5).
#[tracing::instrument(skip(reader, skip_by_seek))]
fn parse_png_exif_iter<R: Read>(
    parser: &mut MediaParser,
    reader: &mut R,
    skip_by_seek: crate::parser::SkipBySeekFn<R>,
) -> Result<ExifIter, crate::Error> {
    use crate::png::{PngExifSource, PngParseOut};

    let out: PngParseOut = parser.load_and_parse(reader, skip_by_seek, |buf, _| {
        crate::png::extract_chunks(buf)
    })?;

    let Some(source) = out.exif else {
        return Err(crate::Error::ExifNotFound);
    };

    match source {
        PngExifSource::EXif(range) => {
            let (full, position) = parser.share_buf();
            let abs = (range.start + position)..(range.end + position);
            let view = full.slice(abs);
            input_into_iter(view, None)
        }
        PngExifSource::Legacy(_) => {
            // Phase 5 implements legacy path. For now, treat as
            // "EXIF not found" — this branch is unreachable in
            // phase 4 because extract_chunks never produces Legacy
            // until phase 5 adds the recognition logic.
            Err(crate::Error::ExifNotFound)
        }
    }
}
```

- [ ] **Step 2: Add `Png` short-circuit at the top of `parse_exif_iter`**

Edit `src/exif.rs` — find the existing `parse_exif_iter` function (around line 27). Add a Png check immediately after the CR3 check:

```rust
pub(crate) fn parse_exif_iter<R: Read>(
    parser: &mut MediaParser,
    mime_img: MediaMimeImage,
    reader: &mut R,
    skip_by_seek: crate::parser::SkipBySeekFn<R>,
) -> Result<ExifIter, crate::Error> {
    // For CR3 files, we need special handling to get all CMT blocks
    if mime_img == MediaMimeImage::Cr3 {
        return parse_cr3_exif_iter(parser, reader, skip_by_seek);
    }
    // PNG: special-cased path peer to CR3.
    if mime_img == MediaMimeImage::Png {
        return parse_png_exif_iter(parser, reader, skip_by_seek);
    }

    // ... rest unchanged ...
```

- [ ] **Step 3: The `MediaMimeImage::Png` arm in `extract_exif_with_mime` is now unreachable**

Edit `src/exif.rs` — find the P1 stub arm (`MediaMimeImage::Png =>`) and replace its body with `unreachable!`:

```rust
        MediaMimeImage::Png => {
            // PNG is dispatched to parse_png_exif_iter at the top of
            // parse_exif_iter; this arm is unreachable in v3.3.
            unreachable!("PNG should have been dispatched at parse_exif_iter top");
        }
```

- [ ] **Step 4: Add async dispatch**

Edit `src/exif.rs` — find `parse_exif_iter_async` (around line 203). Add the same Png branch at the top:

```rust
#[cfg(feature = "tokio")]
pub(crate) async fn parse_exif_iter_async<P, R: AsyncRead + Unpin + Send>(
    parser: &mut P,
    mime_img: MediaMimeImage,
    reader: &mut R,
    skip_by_seek: crate::parser_async::AsyncSkipBySeekFn<R>,
) -> Result<ExifIter, crate::Error>
where
    P: crate::parser_async::AsyncBufParser + crate::parser::ShareBuf,
{
    if mime_img == MediaMimeImage::Png {
        return parse_png_exif_iter_async(parser, reader, skip_by_seek).await;
    }

    // ... rest unchanged ...
```

And add the `parse_png_exif_iter_async` helper after the sync one:

```rust
#[cfg(feature = "tokio")]
async fn parse_png_exif_iter_async<P, R>(
    parser: &mut P,
    reader: &mut R,
    skip_by_seek: crate::parser_async::AsyncSkipBySeekFn<R>,
) -> Result<ExifIter, crate::Error>
where
    P: crate::parser_async::AsyncBufParser + crate::parser::ShareBuf,
    R: AsyncRead + Unpin + Send,
{
    use crate::png::{PngExifSource, PngParseOut};

    let out: PngParseOut = parser
        .load_and_parse(reader, skip_by_seek, |buf, _| crate::png::extract_chunks(buf))
        .await?;

    let Some(source) = out.exif else {
        return Err(crate::Error::ExifNotFound);
    };

    match source {
        PngExifSource::EXif(range) => {
            let (full, position) = parser.share_buf();
            let abs = (range.start + position)..(range.end + position);
            let view = full.slice(abs);
            input_into_iter(view, None)
        }
        PngExifSource::Legacy(_) => Err(crate::Error::ExifNotFound),
    }
}
```

- [ ] **Step 5: Build and run existing tests**

Run: `cargo build --all-features && cargo test --all-features`
Expected: green. (No new tests yet — they come in Task 4.4. Existing tests unaffected because PNG isn't in any of them.)

- [ ] **Step 6: Commit**

```bash
git add src/exif.rs
git commit -m "$(cat <<'EOF'
feat: parse_png_exif_iter — PNG eXIf path integration

Dispatched at the top of parse_exif_iter (peer to the existing CR3
special case). For PNG inputs, calls png::extract_chunks via
load_and_parse, then materializes PngExifSource::EXif(Range) into
an ExifIter using the existing share_buf zero-copy path.

Legacy path (PngExifSource::Legacy) is wired up but currently
returns ExifNotFound — phase 5 implements the hex-decode + APP1
prefix strip that produces Legacy(Vec<u8>) values.

Async variant follows the same shape using AsyncBufParser.

The MediaMimeImage::Png arm in extract_exif_with_mime is now
unreachable (the top-level dispatch routes around it). Marked
unreachable!() to make this explicit.
EOF
)"
```

---

## Task 4.3: Add `MediaParser::parse_image_metadata` (sync)

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Add a private `parse_png_full` helper**

This produces the structured `(ExifIter, Vec<(String, String)>)` for PNG. Put it in `src/exif.rs` so it can share the chunk-walking dispatch with `parse_png_exif_iter`. Edit `src/exif.rs`:

```rust
/// Like [`parse_png_exif_iter`] but also returns the captured `tEXt`
/// chunks. Used by `MediaParser::parse_image_metadata` for PNG.
#[tracing::instrument(skip(reader, skip_by_seek))]
pub(crate) fn parse_png_full<R: Read>(
    parser: &mut MediaParser,
    reader: &mut R,
    skip_by_seek: crate::parser::SkipBySeekFn<R>,
) -> Result<(Option<ExifIter>, Vec<(String, String)>), crate::Error> {
    use crate::png::{PngExifSource, PngParseOut};

    let out: PngParseOut = parser.load_and_parse(reader, skip_by_seek, |buf, _| {
        crate::png::extract_chunks(buf)
    })?;

    let exif_iter = match out.exif {
        Some(PngExifSource::EXif(range)) => {
            let (full, position) = parser.share_buf();
            let abs = (range.start + position)..(range.end + position);
            let view = full.slice(abs);
            Some(input_into_iter(view, None)?)
        }
        Some(PngExifSource::Legacy(_)) => None, // P5 fills this in
        None => None,
    };

    Ok((exif_iter, out.text_chunks))
}
```

(Note: `pub(crate)` so `parser.rs` can call it.)

- [ ] **Step 2: Add `parse_image_metadata` on `MediaParser`**

Edit `src/parser.rs` — add a new method on `MediaParser` (sync). Put it near `parse_exif`:

```rust
    /// Parse all metadata from an image source: EXIF (if any) and
    /// format-specific extras (PNG `tEXt` chunks, etc.).
    ///
    /// Returns `Err(Error::ExifNotFound)` if neither EXIF nor any
    /// format-specific metadata is found. Returns
    /// `Err(Error::TrackNotFound)`-style errors on track inputs (use
    /// `parse_track` instead).
    ///
    /// **Lazy form** — this method returns `ImageMetadata<ExifIter>`.
    /// Convert to the eager `ImageMetadata<Exif>` via `.into()` if
    /// desired.
    pub fn parse_image_metadata<R: Read>(
        &mut self,
        mut ms: MediaSource<R>,
    ) -> crate::Result<ImageMetadata<crate::ExifIter>> {
        self.reset();
        let res: crate::Result<ImageMetadata<crate::ExifIter>> = (|| {
            // Memory-mode shortcut + buffer setup mirrors parse_exif.
            if let Some(memory) = ms.memory.take() {
                self.state.set_memory(memory);
            } else {
                self.acquire_buf();
                self.buf_mut().append(&mut ms.buf);
                self.fill_buf(&mut ms.reader, INIT_BUF_SIZE)?;
            }

            // Reject track inputs (parse_track is the right API).
            let mime_img = match ms.mime {
                crate::file::MediaMime::Image(img) => img,
                crate::file::MediaMime::Track(_) => return Err(crate::Error::ExifNotFound),
            };

            if mime_img == crate::file::MediaMimeImage::Png {
                let (exif, text_chunks) =
                    crate::exif::parse_png_full(self, &mut ms.reader, ms.skip_by_seek)?;
                let format = if text_chunks.is_empty() {
                    None
                } else {
                    Some(crate::ImageFormatMetadata::Png(crate::PngTextChunks {
                        entries: text_chunks,
                    }))
                };
                if exif.is_none() && format.is_none() {
                    return Err(crate::Error::ExifNotFound);
                }
                Ok(crate::ImageMetadata { exif, format })
            } else {
                // Non-PNG: existing parse_exif_iter path; format always None.
                let iter = crate::exif::parse_exif_iter(
                    self,
                    mime_img,
                    &mut ms.reader,
                    ms.skip_by_seek,
                )?;
                Ok(crate::ImageMetadata {
                    exif: Some(iter),
                    format: None,
                })
            }
        })();
        self.reset();
        res
    }
```

- [ ] **Step 3: Add a unit test**

Edit `src/parser.rs::tests`:

```rust
    #[test]
    fn parse_image_metadata_jpeg_returns_exif_only() {
        let mut parser = MediaParser::new();
        let ms = MediaSource::open("testdata/exif.jpg").unwrap();
        let img = parser.parse_image_metadata(ms).unwrap();
        assert!(img.exif.is_some());
        assert!(img.format.is_none());
    }

    #[test]
    fn parse_image_metadata_jpeg_from_memory() {
        let mut parser = MediaParser::new();
        let raw = std::fs::read("testdata/exif.jpg").unwrap();
        let ms = MediaSource::from_memory(raw).unwrap();
        let img = parser.parse_image_metadata(ms).unwrap();
        assert!(img.exif.is_some());
        assert!(img.format.is_none());
    }

    #[test]
    fn parse_image_metadata_on_track_returns_exif_not_found() {
        let mut parser = MediaParser::new();
        let ms = MediaSource::open("testdata/meta.mov").unwrap();
        let res = parser.parse_image_metadata(ms);
        assert!(matches!(res, Err(crate::Error::ExifNotFound)));
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --all-features parse_image_metadata`
Expected: 3 tests pass. Full suite still green.

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs src/exif.rs
git commit -m "$(cat <<'EOF'
feat: MediaParser::parse_image_metadata (sync)

Single method handles file/stream/memory sources via the same
<R: Read> bound used by parse_exif. Internal dispatch:
- PNG: parse_png_full producing (ExifIter, Vec<(String, String)>)
- Other images: existing parse_exif_iter; format is None
- Tracks: ExifNotFound (use parse_track)

Returns ImageMetadata<ExifIter>; convert via .into() for eager
ImageMetadata<Exif> if desired.

For PNG, the tEXt chunks become ImageFormatMetadata::Png(PngTextChunks).
If both exif and format are None, returns ExifNotFound.
EOF
)"
```

---

## Task 4.4: Add `MediaParser::parse_image_metadata_async`

**Files:**
- Modify: `src/parser.rs` (the `tokio_impl` module)

- [ ] **Step 1: Add a private `parse_png_full_async` helper in exif.rs**

Edit `src/exif.rs` — add after `parse_png_exif_iter_async`:

```rust
#[cfg(feature = "tokio")]
pub(crate) async fn parse_png_full_async<P, R>(
    parser: &mut P,
    reader: &mut R,
    skip_by_seek: crate::parser_async::AsyncSkipBySeekFn<R>,
) -> Result<(Option<ExifIter>, Vec<(String, String)>), crate::Error>
where
    P: crate::parser_async::AsyncBufParser + crate::parser::ShareBuf,
    R: AsyncRead + Unpin + Send,
{
    use crate::png::{PngExifSource, PngParseOut};

    let out: PngParseOut = parser
        .load_and_parse(reader, skip_by_seek, |buf, _| crate::png::extract_chunks(buf))
        .await?;

    let exif_iter = match out.exif {
        Some(PngExifSource::EXif(range)) => {
            let (full, position) = parser.share_buf();
            let abs = (range.start + position)..(range.end + position);
            let view = full.slice(abs);
            Some(input_into_iter(view, None)?)
        }
        Some(PngExifSource::Legacy(_)) => None,
        None => None,
    };

    Ok((exif_iter, out.text_chunks))
}
```

- [ ] **Step 2: Add `parse_image_metadata_async` in `tokio_impl`**

Edit `src/parser.rs` — inside `mod tokio_impl`, add:

```rust
        pub async fn parse_image_metadata_async<R: AsyncRead + Unpin + Send>(
            &mut self,
            mut ms: AsyncMediaSource<R>,
        ) -> crate::Result<crate::ImageMetadata<crate::ExifIter>> {
            self.reset();
            let res: crate::Result<crate::ImageMetadata<crate::ExifIter>> = async {
                if let Some(memory) = ms.memory.take() {
                    self.state.set_memory(memory);
                } else {
                    self.acquire_buf();
                    self.buf_mut().append(&mut ms.buf);
                    <Self as AsyncBufParser>::fill_buf(self, &mut ms.reader, INIT_BUF_SIZE).await?;
                }

                let mime_img = match ms.mime {
                    crate::file::MediaMime::Image(img) => img,
                    crate::file::MediaMime::Track(_) => return Err(crate::Error::ExifNotFound),
                };

                if mime_img == crate::file::MediaMimeImage::Png {
                    let (exif, text_chunks) = crate::exif::parse_png_full_async(
                        self,
                        &mut ms.reader,
                        ms.skip_by_seek,
                    )
                    .await?;
                    let format = if text_chunks.is_empty() {
                        None
                    } else {
                        Some(crate::ImageFormatMetadata::Png(crate::PngTextChunks {
                            entries: text_chunks,
                        }))
                    };
                    if exif.is_none() && format.is_none() {
                        return Err(crate::Error::ExifNotFound);
                    }
                    Ok(crate::ImageMetadata { exif, format })
                } else {
                    let iter = crate::exif::parse_exif_iter_async(
                        self,
                        mime_img,
                        &mut ms.reader,
                        ms.skip_by_seek,
                    )
                    .await?;
                    Ok(crate::ImageMetadata {
                        exif: Some(iter),
                        format: None,
                    })
                }
            }
            .await;
            self.reset();
            res
        }
```

- [ ] **Step 3: Add an async test**

Edit `src/parser.rs::tests` (gated on `tokio`):

```rust
    #[cfg(feature = "tokio")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn parse_image_metadata_async_jpeg() {
        use crate::parser_async::AsyncMediaSource;
        let mut parser = MediaParser::new();
        let ms = AsyncMediaSource::open("testdata/exif.jpg").await.unwrap();
        let img = parser.parse_image_metadata_async(ms).await.unwrap();
        assert!(img.exif.is_some());
        assert!(img.format.is_none());
    }
```

- [ ] **Step 4: Run async tests**

Run: `cargo test --all-features parse_image_metadata_async`
Expected: 1 test passes; full suite still green.

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs src/exif.rs
git commit -m "$(cat <<'EOF'
feat: parse_image_metadata_async (tokio)

Mirrors the sync parse_image_metadata using AsyncBufParser. PNG
chunk extraction reuses the same png::extract_chunks pure function
via the async load_and_parse, so no PNG-specific async code beyond
the dispatch wrapper.
EOF
)"
```

---

## Task 4.5: Generate test PNG fixtures and add integration tests

**Files:**
- Create: `tests/png.rs`
- Create: `testdata/exif.png` (overwrite the placeholder from P1.1 with a real EXIF-bearing PNG)
- Create: `testdata/text-only.png`

- [ ] **Step 1: Add a fixture-generation test (one-shot)**

This is a slightly unusual approach — we use a `#[test]` to write the fixture files. The alternative is a separate binary; using a test keeps everything in `cargo test`. Edit `tests/png_fixtures.rs`:

Add at the bottom:

```rust
#[cfg(test)]
mod gen {
    use super::*;

    /// Run via `cargo test --test png_fixtures gen::write_fixtures` to
    /// (re)generate testdata/*.png from this builder. Idempotent —
    /// existing files are overwritten.
    #[test]
    #[ignore = "fixture generation is opt-in; run with --ignored"]
    fn write_fixtures() {
        // exif.png: eXIf chunk + Title + Software tEXt
        let tiff = tiff_from_jpeg_fixture();
        let png = build_png(&[
            text_chunk("Title", "PNG with EXIF"),
            text_chunk("Software", "nom-exif fixture builder"),
            exif_chunk(&tiff),
        ]);
        std::fs::write("testdata/exif.png", &png).unwrap();

        // text-only.png: tEXt only, no EXIF
        let png = build_png(&[
            text_chunk("Title", "Just text"),
            text_chunk("Author", "test"),
        ]);
        std::fs::write("testdata/text-only.png", &png).unwrap();
    }
}
```

- [ ] **Step 2: Run the fixture generator**

```bash
cargo test --test png_fixtures gen::write_fixtures -- --ignored
```

Expected: test runs and creates `testdata/exif.png` (overwrites P1.1's placeholder) and `testdata/text-only.png`.

- [ ] **Step 3: Verify the fixtures**

Run: `ls -la testdata/*.png && file testdata/*.png`
Expected: both files exist, PNG-format detected.

Run: `cargo test --all-features mime::exif_png`
Expected: still passes (the larger fixture still has the same signature).

- [ ] **Step 4: Create `tests/png.rs` integration test file**

Create `tests/png.rs`:

```rust
//! Integration tests for PNG support. Each fixture is exercised through
//! the full set of public entry points.

#[path = "png_fixtures.rs"]
mod png_fixtures;

use nom_exif::{
    ExifTag, ImageFormatMetadata, MediaParser, MediaSource, read_exif,
};

#[test]
fn read_exif_on_exif_png_file() {
    let exif = read_exif("testdata/exif.png").unwrap();
    assert!(exif.get(ExifTag::Make).is_some());
}

#[test]
fn read_exif_on_text_only_png_returns_exif_not_found() {
    let res = read_exif("testdata/text-only.png");
    assert!(matches!(res, Err(nom_exif::Error::ExifNotFound)));
}

#[test]
fn parse_image_metadata_exif_png_file() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("testdata/exif.png").unwrap();
    let img = parser.parse_image_metadata(ms).unwrap();
    assert!(img.exif.is_some());
    let format = img.format.expect("expected PNG format metadata");
    let ImageFormatMetadata::Png(text_chunks) = format;
    assert_eq!(text_chunks.get("Title"), Some("PNG with EXIF"));
    assert_eq!(text_chunks.get("Software"), Some("nom-exif fixture builder"));
}

#[test]
fn parse_image_metadata_exif_png_from_memory() {
    let mut parser = MediaParser::new();
    let raw = std::fs::read("testdata/exif.png").unwrap();
    let ms = MediaSource::from_memory(raw).unwrap();
    let img = parser.parse_image_metadata(ms).unwrap();
    assert!(img.exif.is_some());
    assert!(img.format.is_some());
}

#[test]
fn parse_image_metadata_text_only_png_no_exif_but_format_present() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("testdata/text-only.png").unwrap();
    let img = parser.parse_image_metadata(ms).unwrap();
    assert!(img.exif.is_none());
    let format = img.format.expect("expected PNG format metadata");
    let ImageFormatMetadata::Png(text_chunks) = format;
    assert_eq!(text_chunks.get("Title"), Some("Just text"));
}

#[cfg(feature = "tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn parse_image_metadata_async_exif_png() {
    use nom_exif::AsyncMediaSource;
    let mut parser = MediaParser::new();
    let ms = AsyncMediaSource::open("testdata/exif.png").await.unwrap();
    let img = parser.parse_image_metadata_async(ms).await.unwrap();
    assert!(img.exif.is_some());
    assert!(img.format.is_some());
}

#[test]
fn lazy_to_eager_conversion_works() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("testdata/exif.png").unwrap();
    let lazy = parser.parse_image_metadata(ms).unwrap();
    let eager: nom_exif::ImageMetadata = lazy.into();
    assert!(eager.exif.is_some());
    assert!(eager.format.is_some());
}
```

- [ ] **Step 5: Run integration tests**

Run: `cargo test --all-features --test png`
Expected: 7 tests pass (or 6 without `tokio`).

Run: `cargo test --all-features` to verify no regressions.

- [ ] **Step 6: Commit**

```bash
git add tests/png_fixtures.rs tests/png.rs testdata/exif.png testdata/text-only.png
git commit -m "$(cat <<'EOF'
test: PNG integration tests for parse_image_metadata + read_exif

Generates two fixture PNGs programmatically (eXIf+tEXt and tEXt-only)
using a real EXIF blob extracted from testdata/exif.jpg's APP1
segment.

Tests cover:
- read_exif on EXIF-bearing PNG (file path)
- read_exif on tEXt-only PNG returns ExifNotFound (contract preserved)
- parse_image_metadata on EXIF-bearing PNG (file + memory routes)
- parse_image_metadata on tEXt-only returns Ok with format only
- parse_image_metadata_async (under #[cfg(feature = "tokio")])
- ImageMetadata<ExifIter> -> ImageMetadata<Exif> conversion

Legacy hex-encoded EXIF (Raw profile type *) is not yet covered;
that lands in phase 5.
EOF
)"
```

---

## Task 4.6: Final verification of phase 4

- [ ] **Step 1: Full test suite**

Run: `cargo test --all-features`
Expected: green.

- [ ] **Step 2: Format clean**

Run: `cargo fmt --check`
Expected: clean.

- [ ] **Step 3: Self-check exit criterion**

> `parse_png_exif_iter` integrated into `exif::parse_exif_iter` (replaces P1 stub). ✓
> `MediaParser::parse_image_metadata<R: Read>` + async variant added. ✓
> Non-PNG falls back to `parse_exif_iter` returning `ImageMetadata { exif: Some(iter), format: None }`. ✓
> Real PNG fixtures generated by `tests/png_fixtures.rs`. ✓
> Integration tests pass through file + memory routes, sync + async. ✓
> `eXIf` path only — legacy is P5. ✓
> `cargo test --all-features` green. ✓

Phase 4 complete. Proceed to P5.

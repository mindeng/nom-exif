# PNG P0 — Source-input unification + deprecation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify the source-input model so `parse_exif<R: Read>` / `parse_track<R: Read>` (sync + async) accept memory-mode sources directly, and deprecate the old `from_bytes` / `parse_*_from_bytes` / `read_*_from_bytes` family. No breaking change in v3.x — old code still compiles with deprecation warnings.

**Architecture:** Add `MediaSource::<std::io::Empty>::from_memory(bytes)` as the new memory-mode constructor (`Empty: Read`, so the resulting `MediaSource<Empty>` satisfies the existing `<R: Read>` bound). Refactor `parse_exif` / `parse_track` to branch on `ms.memory.is_some()` at runtime — same logic that today's `parse_exif_from_bytes` already takes. The deprecated `parse_*_from_bytes` methods become thin adapters that call into the unified path. Zero-copy property of memory mode preserved verbatim (uses the same `state.set_memory(memory)` path).

**Tech Stack:** `std::io::Empty`, `bytes::Bytes`, `cargo test --all-features`, `cargo fmt --check`.

---

## File Structure

| File | Change |
|---|---|
| `src/parser.rs` | Add `MediaSource::<Empty>::from_memory`; refactor `parse_exif` / `parse_track` to branch on memory; mark `MediaSource::<()>::from_bytes`, `parse_exif_from_bytes`, `parse_track_from_bytes` as `#[deprecated]`; add private `MediaSource::<()>::into_empty()` adapter so the deprecated methods can delegate. |
| `src/parser_async.rs` | Mirror the same refactor + deprecation for `parse_exif_async` / `parse_track_async` (if any `_from_bytes_async` variant exists; per spec there is none currently). Add async variants if needed for symmetry. |
| `src/lib.rs` | `#[deprecated]` on `read_exif_from_bytes`, `read_exif_iter_from_bytes`, `read_track_from_bytes`, `read_metadata_from_bytes`. Migrate doctest examples in module docstring from `from_bytes` to `from_memory`. |
| `README.md` | Migrate "In-Memory Bytes" section examples from `from_bytes` to `from_memory`. Add a deprecation note. |
| `docs/MIGRATION.md` | Add a "v3.0 → v3.3" subsection documenting the deprecation paths. |
| `Cargo.toml` | (Verify only — no change needed; ensure no `-Dwarnings` lurking in `[lints]`.) |
| `.github/workflows/*.yml` | (Verify only — no change needed; ensure CI doesn't `-Dwarnings`.) |

---

## Task 0.0: Pre-flight — CI policy audit + private helper

**Files:**
- Read: `.github/workflows/rust.yml`, `Cargo.toml`
- Modify: `src/parser.rs:140-200` (add `into_empty` helper)

- [ ] **Step 1: Verify CI doesn't treat warnings as errors**

Run: `grep -rn 'D *warnings\|deny.*warnings\|RUSTFLAGS' .github/workflows/ Cargo.toml`
Expected: no matches in `.github/workflows/`. If any, the deprecation warnings introduced later will fail CI — escalate before proceeding.

- [ ] **Step 2: Verify `cargo fmt` policy in pre-commit hook is set up**

Run: `cat .githooks/pre-commit 2>/dev/null && git config core.hooksPath`
Expected: pre-commit hook exists and runs `cargo fmt --check`. If `core.hooksPath` is unset, run `git config core.hooksPath .githooks` (per memory: cargo fmt is a hard CI gate).

- [ ] **Step 3: Add private `MediaSource::<()>::into_empty()` helper**

Edit `src/parser.rs` — find the `impl MediaSource<()>` block near the `from_bytes` method (around line 146-192) and add this method *inside* the same impl block (after `from_bytes`):

```rust
    /// Internal adapter: convert a v3.0-style `MediaSource<()>` (built via
    /// the deprecated `from_bytes`) into the unified `MediaSource<Empty>`
    /// shape so the deprecated `parse_*_from_bytes` methods can delegate to
    /// the unified `parse_*` methods. Memory contents are moved over
    /// verbatim, preserving zero-copy.
    pub(crate) fn into_empty(self) -> MediaSource<std::io::Empty> {
        MediaSource {
            reader: std::io::empty(),
            buf: self.buf,
            mime: self.mime,
            // Placeholder: never invoked in memory mode (clear_and_skip's
            // AdvanceOnly path is the only one taken).
            skip_by_seek: |_, _| Ok(false),
            memory: self.memory,
        }
    }
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --all-features`
Expected: no errors. (No tests yet — pure plumbing.)

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "$(cat <<'EOF'
refactor: add private MediaSource::<()>::into_empty() adapter

Internal scaffolding for the upcoming source-input unification. Lets
the deprecated parse_*_from_bytes methods delegate to the unified
parse_* methods once those are extended to handle memory mode.

No behavior change.
EOF
)"
```

---

## Task 0.1: Add `MediaSource::<Empty>::from_memory` constructor

**Files:**
- Modify: `src/parser.rs` (add new impl block after the `MediaSource<()>` block)
- Test: `src/parser.rs` (in the `tests` module)

- [ ] **Step 1: Add the new impl block**

Edit `src/parser.rs` — add this impl block *after* the existing `impl MediaSource<()>` block (around line 192, after `from_bytes`):

```rust
impl MediaSource<std::io::Empty> {
    /// Build a [`MediaSource`] from an in-memory byte payload.
    ///
    /// This is the v3.3 replacement for [`MediaSource::<()>::from_bytes`]
    /// (which is now `#[deprecated]`). Functionally identical — same
    /// zero-copy semantics, same accepted input types — but produces a
    /// `MediaSource<std::io::Empty>` so that the unified `parse_*<R: Read>`
    /// methods can accept it directly without a separate `_from_bytes`
    /// sibling.
    ///
    /// Accepts any type convertible into [`bytes::Bytes`] — `Bytes`,
    /// `Vec<u8>`, `&'static [u8]`, `String`, `Box<[u8]>`, and HTTP-stack
    /// body types that implement `Into<Bytes>` directly.
    ///
    /// The header (first up to 128 bytes) is sniffed for media kind, the
    /// same way [`MediaSource::open`] does it for files. The full payload
    /// is stored zero-copy: subsequent parsing through
    /// [`MediaParser::parse_exif`] / [`MediaParser::parse_track`] shares
    /// this `Bytes` directly with the returned `ExifIter` / sub-IFDs via
    /// reference counting.
    ///
    /// # Example
    ///
    /// ```rust
    /// use nom_exif::{MediaSource, MediaParser, MediaKind};
    ///
    /// let bytes = std::fs::read("./testdata/exif.jpg")?;
    /// let ms = MediaSource::from_memory(bytes)?;
    /// assert_eq!(ms.kind(), MediaKind::Image);
    ///
    /// let mut parser = MediaParser::new();
    /// let _iter = parser.parse_exif(ms)?;  // unified entry point
    /// # Ok::<(), nom_exif::Error>(())
    /// ```
    pub fn from_memory(bytes: impl Into<bytes::Bytes>) -> crate::Result<Self> {
        let bytes = bytes.into();
        let head_end = bytes.len().min(HEADER_PARSE_BUF_SIZE);
        let mime: MediaMime = bytes[..head_end].try_into()?;
        Ok(Self {
            reader: std::io::empty(),
            buf: Vec::new(),
            mime,
            // Placeholder: never invoked in memory mode (AdvanceOnly path).
            skip_by_seek: |_, _| Ok(false),
            memory: Some(bytes),
        })
    }
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check --all-features`
Expected: no errors.

- [ ] **Step 3: Add basic constructor tests**

Edit `src/parser.rs` — find the existing `mod tests` block (search for `media_source_from_bytes_image_jpg`) and add these tests inside it:

```rust
    #[test]
    fn media_source_from_memory_image_jpg() {
        let raw = std::fs::read("testdata/exif.jpg").unwrap();
        let ms = MediaSource::from_memory(raw).unwrap();
        assert_eq!(ms.kind(), MediaKind::Image);
        assert!(ms.memory.is_some());
    }

    #[test]
    fn media_source_from_memory_track_mov() {
        let raw = std::fs::read("testdata/meta.mov").unwrap();
        let ms = MediaSource::from_memory(raw).unwrap();
        assert_eq!(ms.kind(), MediaKind::Track);
    }

    #[test]
    fn media_source_from_memory_static_slice() {
        let raw: &'static [u8] = include_bytes!("../testdata/exif.jpg");
        let ms = MediaSource::from_memory(raw).unwrap();
        assert_eq!(ms.kind(), MediaKind::Image);
    }

    #[test]
    fn media_source_from_memory_rejects_too_short() {
        let raw = vec![0u8; 4];
        let res = MediaSource::from_memory(raw);
        assert!(res.is_err());
    }

    #[test]
    fn media_source_from_memory_rejects_unknown_mime() {
        let raw = vec![0xAAu8; 256];
        let res = MediaSource::from_memory(raw);
        assert!(res.is_err());
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --all-features media_source_from_memory`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "$(cat <<'EOF'
feat: add MediaSource::from_memory for unified memory-mode source

Returns MediaSource<std::io::Empty>, which satisfies the existing
<R: Read> bound on parse_exif / parse_track. This sets up the
upcoming unification — a single parse_* method per "what to parse",
with no separate _from_bytes sibling.

The old MediaSource::<()>::from_bytes constructor stays untouched in
this commit; it will be deprecated in a follow-up commit once the
parse_* methods are updated to handle memory-mode sources.
EOF
)"
```

---

## Task 0.2: Refactor `MediaParser::parse_exif` to handle memory mode

**Files:**
- Modify: `src/parser.rs` (the `parse_exif` method body)

- [ ] **Step 1: Locate the method**

Run: `grep -n 'pub fn parse_exif<R: Read>' src/parser.rs`
Expected: one hit (around line 688).

- [ ] **Step 2: Replace `parse_exif` body with the unified version**

Edit `src/parser.rs` — replace the existing `parse_exif` method body (look for `pub fn parse_exif<R: Read>`) with:

```rust
    /// Parse Exif metadata from an image source. Returns `Error::ExifNotFound`
    /// if the source is a `Track` (use [`Self::parse_track`] instead).
    ///
    /// As of v3.3, this method also accepts memory-mode sources built via
    /// [`MediaSource::from_memory`]. The deprecated [`Self::parse_exif_from_bytes`]
    /// is now a thin adapter that delegates here.
    ///
    /// `MediaParser` reuses its internal parse buffer across calls, so prefer
    /// reusing a single `MediaParser` over creating a new one per file. Drop
    /// the returned [`ExifIter`] (or convert it into [`crate::Exif`]) before
    /// the next `parse_*` call so the buffer can be reclaimed.
    pub fn parse_exif<R: Read>(&mut self, mut ms: MediaSource<R>) -> crate::Result<ExifIter> {
        self.reset();
        let res: crate::Result<ExifIter> = (|| {
            if let Some(memory) = ms.memory.take() {
                // Memory-mode: zero-copy share of caller-owned bytes.
                self.state.set_memory(memory);
                if !matches!(ms.mime, crate::file::MediaMime::Image(_)) {
                    return Err(crate::Error::ExifNotFound);
                }
                crate::exif::parse_exif_iter(
                    self,
                    ms.mime.unwrap_image(),
                    &mut ms.reader,
                    ms.skip_by_seek,
                )
            } else {
                // Streaming-mode: existing path verbatim.
                self.acquire_buf();
                self.buf_mut().append(&mut ms.buf);
                self.fill_buf(&mut ms.reader, INIT_BUF_SIZE)?;
                if !matches!(ms.mime, crate::file::MediaMime::Image(_)) {
                    return Err(crate::Error::ExifNotFound);
                }
                crate::exif::parse_exif_iter(
                    self,
                    ms.mime.unwrap_image(),
                    &mut ms.reader,
                    ms.skip_by_seek,
                )
            }
        })();
        self.reset();
        res
    }
```

(Note: the two branches share the same `parse_exif_iter` call. The duplication is intentional — easier to read than factoring out a helper.)

- [ ] **Step 3: Add a parallel test exercising the new unified route via `from_memory`**

Edit `src/parser.rs` — in the `tests` module, add:

```rust
    #[test]
    fn parse_exif_unified_from_memory_jpg() {
        let mut parser = MediaParser::new();
        let raw = std::fs::read("testdata/exif.jpg").unwrap();
        let ms = MediaSource::from_memory(raw).unwrap();
        let iter = parser.parse_exif(ms).unwrap();
        let exif: crate::Exif = iter.into();
        assert!(exif.get(crate::ExifTag::Make).is_some());
    }

    #[test]
    fn parse_exif_unified_from_memory_heic() {
        let mut parser = MediaParser::new();
        let raw = std::fs::read("testdata/exif.heic").unwrap();
        let ms = MediaSource::from_memory(raw).unwrap();
        let iter = parser.parse_exif(ms).unwrap();
        let exif: crate::Exif = iter.into();
        assert_eq!(
            exif.get(crate::ExifTag::Make).and_then(|v| v.as_str()),
            Some("Apple")
        );
    }

    #[test]
    fn parse_exif_unified_from_memory_zero_copy_preserved() {
        let raw = std::fs::read("testdata/exif.jpg").unwrap();
        let bytes = bytes::Bytes::from(raw);
        let _original_ptr = bytes.as_ptr();

        let mut parser = MediaParser::new();
        let ms = MediaSource::from_memory(bytes).unwrap();
        let iter = parser.parse_exif(ms).unwrap();

        // Memory mode must not poison the recycle cache — same invariant
        // the old parse_exif_from_bytes route asserts.
        assert!(parser.state.cached_ptr_for_test().is_none(),
            "memory mode must not write to the streaming-buf recycle cache");
        drop(iter);
    }

    #[test]
    fn parse_exif_unified_on_track_returns_exif_not_found() {
        let mut parser = MediaParser::new();
        let raw = std::fs::read("testdata/meta.mov").unwrap();
        let ms = MediaSource::from_memory(raw).unwrap();
        let res = parser.parse_exif(ms);
        assert!(matches!(res, Err(crate::Error::ExifNotFound)));
    }

    #[test]
    fn parse_exif_unified_on_truncated_returns_io_error() {
        let mut raw = std::fs::read("testdata/exif.jpg").unwrap();
        raw.truncate(200);
        let mut parser = MediaParser::new();
        let ms = MediaSource::from_memory(raw).unwrap();
        let res = parser.parse_exif(ms);
        assert!(res.is_err(), "expected error on truncated bytes, got {:?}", res);
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --all-features parse_exif_unified`
Expected: 5 new tests pass. Also run `cargo test --all-features` to verify no regressions in the existing memory-mode tests (`parse_exif_from_bytes_*`).

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "$(cat <<'EOF'
refactor: parse_exif handles memory-mode sources internally

parse_exif<R: Read> now branches on ms.memory.is_some() at runtime.
Memory-mode sources (built via MediaSource::from_memory) take the
same zero-copy state.set_memory path that parse_exif_from_bytes uses
today. Streaming sources are unchanged.

Existing parse_exif_from_bytes still works (will be marked
deprecated in a later commit).
EOF
)"
```

---

## Task 0.3: Refactor `MediaParser::parse_track` to handle memory mode

**Files:**
- Modify: `src/parser.rs` (the `parse_track` method body)

- [ ] **Step 1: Locate the method**

Run: `grep -n 'pub fn parse_track<R: Read>' src/parser.rs`
Expected: one hit (around line 715).

- [ ] **Step 2: Replace `parse_track` body**

Edit `src/parser.rs` — replace the existing `parse_track` body with a memory-mode-aware version. Mirror the structure of the new `parse_exif`:

```rust
    /// Parse track info from a video/audio source.
    ///
    /// In v3.1, this also accepts JPEG images that carry an embedded
    /// Pixel/Google Motion Photo trailer. As of v3.3, it also accepts
    /// memory-mode sources built via [`MediaSource::from_memory`]; the
    /// deprecated [`Self::parse_track_from_bytes`] is now a thin
    /// adapter that delegates here.
    pub fn parse_track<R: Read>(&mut self, mut ms: MediaSource<R>) -> crate::Result<TrackInfo> {
        self.reset();
        let res: crate::Result<TrackInfo> = (|| {
            if let Some(memory) = ms.memory.take() {
                // Memory mode: zero-copy.
                self.state.set_memory(memory);
                let mime_track = match ms.mime {
                    crate::file::MediaMime::Image(_) => return Err(crate::Error::TrackNotFound),
                    crate::file::MediaMime::Track(t) => t,
                };
                let out = self.load_and_parse(
                    &mut ms.reader,
                    ms.skip_by_seek,
                    |data, _| {
                        crate::video::parse_track_info(data, mime_track)
                            .map_err(|e| ParsingErrorState::new(e, None))
                    },
                )?;
                Ok(out)
            } else {
                // Streaming mode: existing path verbatim.
                self.acquire_buf();
                self.buf_mut().append(&mut ms.buf);
                self.fill_buf(&mut ms.reader, INIT_BUF_SIZE)?;
                match ms.mime {
                    crate::file::MediaMime::Image(crate::file::MediaMimeImage::Jpeg) => {
                        self.parse_jpeg_motion_photo(&mut ms.reader)
                    }
                    crate::file::MediaMime::Image(_) => Err(crate::Error::TrackNotFound),
                    crate::file::MediaMime::Track(mime_track) => {
                        let skip = ms.skip_by_seek;
                        Ok(self.load_and_parse(ms.reader.by_ref(), skip, |data, _| {
                            crate::video::parse_track_info(data, mime_track)
                                .map_err(|e| ParsingErrorState::new(e, None))
                        })?)
                    }
                }
            }
        })();
        self.reset();
        res
    }
```

(Note: the memory branch does NOT include the JPEG Motion Photo path. Reason: that path requires `read_to_end` on the source reader to find the trailer — incompatible with memory mode's zero-copy invariant. Memory-mode JPEG callers wanting Motion Photo data should use the streaming path. This matches the existing behavior of `parse_track_from_bytes` which also doesn't try to extract Motion Photos from in-memory bytes.)

- [ ] **Step 3: Add tests**

Edit `src/parser.rs` — in the `tests` module, add:

```rust
    #[test]
    fn parse_track_unified_from_memory_mov() {
        let mut parser = MediaParser::new();
        let raw = std::fs::read("testdata/meta.mov").unwrap();
        let ms = MediaSource::from_memory(raw).unwrap();
        let info = parser.parse_track(ms).unwrap();
        assert_eq!(info.get(crate::TrackInfoTag::Make), Some(&"Apple".into()));
    }

    #[test]
    fn parse_track_unified_from_memory_mp4() {
        let mut parser = MediaParser::new();
        let raw = std::fs::read("testdata/meta.mp4").unwrap();
        let ms = MediaSource::from_memory(raw).unwrap();
        let info = parser.parse_track(ms).unwrap();
        assert!(info.get(crate::TrackInfoTag::CreateDate).is_some());
    }

    #[test]
    fn parse_track_unified_on_image_returns_track_not_found() {
        let mut parser = MediaParser::new();
        let raw = std::fs::read("testdata/exif.jpg").unwrap();
        let ms = MediaSource::from_memory(raw).unwrap();
        let res = parser.parse_track(ms);
        assert!(matches!(res, Err(crate::Error::TrackNotFound)));
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --all-features parse_track_unified`
Expected: 3 new tests pass; full `cargo test --all-features` still green.

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "$(cat <<'EOF'
refactor: parse_track handles memory-mode sources internally

Same shape as the parse_exif refactor in the previous commit:
runtime branch on ms.memory.is_some() picks the memory-mode (zero
copy) or streaming path. Memory-mode JPEG callers do not get
Motion Photo extraction (matches existing parse_track_from_bytes
behavior).
EOF
)"
```

---

## Task 0.4: Refactor async `parse_exif_async` / `parse_track_async` to handle memory mode

**Files:**
- Modify: `src/parser.rs` (the `tokio_impl` mod, `parse_exif_async` / `parse_track_async`)

- [ ] **Step 1: Locate the async methods**

Run: `grep -n 'parse_exif_async\|parse_track_async' src/parser.rs`
Expected: definitions inside `mod tokio_impl` (around line 891 / 918).

- [ ] **Step 2: Replace `parse_exif_async` body**

Edit `src/parser.rs` — inside `mod tokio_impl`, replace the body of `parse_exif_async`:

```rust
        pub async fn parse_exif_async<R: AsyncRead + Unpin + Send>(
            &mut self,
            mut ms: AsyncMediaSource<R>,
        ) -> crate::Result<ExifIter> {
            self.reset();
            let res: crate::Result<ExifIter> = async {
                if let Some(memory) = ms.memory.take() {
                    self.state.set_memory(memory);
                    if !matches!(ms.mime, crate::file::MediaMime::Image(_)) {
                        return Err(crate::Error::ExifNotFound);
                    }
                    crate::exif::parse_exif_iter_async(
                        self,
                        ms.mime.unwrap_image(),
                        &mut ms.reader,
                        ms.skip_by_seek,
                    )
                    .await
                } else {
                    self.acquire_buf();
                    self.buf_mut().append(&mut ms.buf);
                    <Self as AsyncBufParser>::fill_buf(self, &mut ms.reader, INIT_BUF_SIZE).await?;
                    if !matches!(ms.mime, crate::file::MediaMime::Image(_)) {
                        return Err(crate::Error::ExifNotFound);
                    }
                    crate::exif::parse_exif_iter_async(
                        self,
                        ms.mime.unwrap_image(),
                        &mut ms.reader,
                        ms.skip_by_seek,
                    )
                    .await
                }
            }
            .await;
            self.reset();
            res
        }
```

- [ ] **Step 3: Replace `parse_track_async` body**

Replace the body of `parse_track_async`:

```rust
        pub async fn parse_track_async<R: AsyncRead + Unpin + Send>(
            &mut self,
            mut ms: AsyncMediaSource<R>,
        ) -> crate::Result<TrackInfo> {
            self.reset();
            let res: crate::Result<TrackInfo> = async {
                if let Some(memory) = ms.memory.take() {
                    self.state.set_memory(memory);
                    let mime_track = match ms.mime {
                        crate::file::MediaMime::Image(_) => return Err(crate::Error::TrackNotFound),
                        crate::file::MediaMime::Track(t) => t,
                    };
                    let out = <Self as AsyncBufParser>::load_and_parse(
                        self,
                        &mut ms.reader,
                        ms.skip_by_seek,
                        |data, _| {
                            crate::video::parse_track_info(data, mime_track)
                                .map_err(|e| ParsingErrorState::new(e, None))
                        },
                    )
                    .await?;
                    Ok(out)
                } else {
                    self.acquire_buf();
                    self.buf_mut().append(&mut ms.buf);
                    <Self as AsyncBufParser>::fill_buf(self, &mut ms.reader, INIT_BUF_SIZE).await?;
                    let mime_track = match ms.mime {
                        crate::file::MediaMime::Image(_) => return Err(crate::Error::TrackNotFound),
                        crate::file::MediaMime::Track(t) => t,
                    };
                    let skip = ms.skip_by_seek;
                    let out = <Self as AsyncBufParser>::load_and_parse(
                        self,
                        &mut ms.reader,
                        skip,
                        |data, _| {
                            crate::video::parse_track_info(data, mime_track)
                                .map_err(|e| ParsingErrorState::new(e, None))
                        },
                    )
                    .await?;
                    Ok(out)
                }
            }
            .await;
            self.reset();
            res
        }
```

- [ ] **Step 4: Add `from_memory` constructor for `AsyncMediaSource`**

Look at `src/parser_async.rs` to see if `AsyncMediaSource` has a `from_bytes` already; if so, add a parallel `from_memory`. If `AsyncMediaSource` doesn't currently support memory mode, add it now:

Run: `grep -n 'impl.*AsyncMediaSource\|from_bytes\|from_memory' src/parser_async.rs`

If no `from_bytes` exists in `AsyncMediaSource`: skip this step (memory-mode async sources weren't supported before either; the new `from_memory` is sync-only via `MediaSource::<Empty>::from_memory`, which is fine because async parser methods take `AsyncMediaSource<R>`, not `MediaSource<R>`).

If a `from_bytes` exists in `AsyncMediaSource`: add an `AsyncMediaSource::<Empty>::from_memory` constructor mirroring the sync one in `MediaSource::<Empty>::from_memory`. Edit `src/parser_async.rs` accordingly. **Important**: deferred — this is independent of P0's main goal. If `AsyncMediaSource` lacks memory mode entirely, file a follow-up note in Open Questions but proceed without async memory-mode for v3.3.

- [ ] **Step 5: Run async tests**

Run: `cargo test --all-features --features tokio parse_exif_async parse_track_async`
Expected: existing async tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/parser.rs src/parser_async.rs
git commit -m "$(cat <<'EOF'
refactor: parse_exif_async / parse_track_async handle memory mode

Mirror the sync refactor: runtime branch on memory.is_some(). The
existing async memory-mode entry points (if any) continue to work;
new memory-mode async sources can be parsed via the unified path
once AsyncMediaSource gains a from_memory constructor (deferred).
EOF
)"
```

---

## Task 0.5: Mark `MediaSource::<()>::from_bytes` and `parse_*_from_bytes` as `#[deprecated]`

**Files:**
- Modify: `src/parser.rs` (deprecation attributes on `MediaSource::<()>::from_bytes`, `MediaParser::parse_exif_from_bytes`, `MediaParser::parse_track_from_bytes`)

- [ ] **Step 1: Mark `MediaSource::<()>::from_bytes` deprecated**

Edit `src/parser.rs` — find `pub fn from_bytes` inside `impl MediaSource<()>`. Add a `#[deprecated]` attribute right above it:

```rust
    #[deprecated(
        since = "3.3.0",
        note = "Use `MediaSource::from_memory` and the unified `parse_*` \
                methods (which now accept memory-mode sources directly). \
                The `MediaSource<()>` shape will be removed in v4."
    )]
    pub fn from_bytes(bytes: impl Into<bytes::Bytes>) -> crate::Result<Self> {
        // ... existing body unchanged ...
    }
```

- [ ] **Step 2: Refactor `parse_exif_from_bytes` to delegate via `into_empty()`**

Edit `src/parser.rs` — replace `parse_exif_from_bytes` with a thin adapter:

```rust
    /// Parse Exif metadata from an in-memory byte payload built via the
    /// deprecated [`MediaSource::<()>::from_bytes`].
    ///
    /// **Deprecated since v3.3.0**: use [`Self::parse_exif`] with
    /// [`MediaSource::from_memory`] directly.
    #[deprecated(
        since = "3.3.0",
        note = "Use `parse_exif` directly — it now accepts memory-mode \
                sources built via `MediaSource::from_memory`."
    )]
    pub fn parse_exif_from_bytes(&mut self, ms: MediaSource<()>) -> crate::Result<ExifIter> {
        self.parse_exif(ms.into_empty())
    }
```

- [ ] **Step 3: Refactor `parse_track_from_bytes` similarly**

```rust
    /// **Deprecated since v3.3.0**: use [`Self::parse_track`] with
    /// [`MediaSource::from_memory`] directly.
    #[deprecated(
        since = "3.3.0",
        note = "Use `parse_track` with `MediaSource::from_memory`."
    )]
    pub fn parse_track_from_bytes(&mut self, ms: MediaSource<()>) -> crate::Result<TrackInfo> {
        self.parse_track(ms.into_empty())
    }
```

- [ ] **Step 4: Verify existing tests still compile (with deprecation warnings)**

Run: `cargo build --all-features 2>&1 | grep -i 'deprecated\|warning' | head -30`
Expected: many deprecation warnings inside the crate's own tests (`media_source_from_bytes_*`, `parse_exif_from_bytes_*`, etc.). Note the count for the next task.

- [ ] **Step 5: Run tests (warnings allowed, errors not)**

Run: `cargo test --all-features 2>&1 | tail -20`
Expected: tests pass (warnings are not errors). Confirm by checking the exit code: `echo $?` (after the test) should be 0.

- [ ] **Step 6: Commit**

```bash
git add src/parser.rs
git commit -m "$(cat <<'EOF'
deprecate: MediaSource::<()>::from_bytes and parse_*_from_bytes

Marks the v3.0 memory-mode API as deprecated in favor of the unified
v3.3 form (MediaSource::from_memory + parse_exif / parse_track).

The deprecated parse_*_from_bytes methods now delegate via the
private into_empty() adapter, eliminating duplicate dispatch logic.

Will be removed in v4. Tests still pass; deprecation warnings inside
the crate are addressed in a follow-up commit.
EOF
)"
```

---

## Task 0.6: `#[allow(deprecated)]` on internal tests that exercise deprecated paths

**Files:**
- Modify: `src/parser.rs` (test functions)
- Modify: `src/lib.rs` (test functions if any in `v3_top_level_tests`)

- [ ] **Step 1: List test functions that need `#[allow(deprecated)]`**

Run: `grep -n 'from_bytes\|parse_exif_from_bytes\|parse_track_from_bytes\|read_exif_from_bytes\|read_exif_iter_from_bytes\|read_track_from_bytes\|read_metadata_from_bytes' src/parser.rs src/lib.rs`
Expected: a list of test function bodies. Inspect each to determine whether the function name is a test (search backwards for `#[test]`).

- [ ] **Step 2: Annotate each test function**

For each test function in `src/parser.rs::tests` that uses deprecated symbols (e.g., `media_source_from_bytes_image_jpg`, `parse_exif_from_bytes_jpg_basic`, etc.), add `#[allow(deprecated)]` between the `#[test]` and the `fn` line:

```rust
    #[test]
    #[allow(deprecated)]
    fn media_source_from_bytes_image_jpg() {
        // ... existing body unchanged ...
    }
```

Apply to all of:
- `media_source_from_bytes_image_jpg`
- `media_source_from_bytes_track_mov`
- `media_source_from_bytes_static_slice`
- `media_source_from_bytes_rejects_too_short`
- `media_source_from_bytes_rejects_unknown_mime`
- `parse_exif_from_bytes_jpg_basic`
- `parse_exif_from_bytes_heic_basic`
- `parse_exif_from_bytes_zero_copy_shared_bytes`
- `parse_exif_from_bytes_on_track_returns_exif_not_found`
- `parse_exif_from_bytes_on_truncated_returns_io_error`
- `parse_track_from_bytes_mov_basic`
- `parse_track_from_bytes_mp4_basic`
- `parse_track_from_bytes_mkv_basic`
- `parse_track_from_bytes_on_image_returns_track_not_found`

In `src/lib.rs::v3_top_level_tests`, similarly annotate:
- `read_exif_from_bytes_jpg`
- `read_exif_iter_from_bytes_jpg`
- `read_track_from_bytes_mov`
- `read_metadata_from_bytes_dispatches_image`
- `read_metadata_from_bytes_dispatches_track`
- `read_exif_from_bytes_static_slice`

(Use the grep output from Step 1 to identify the exact set.)

- [ ] **Step 3: Verify warnings cleared**

Run: `cargo build --all-features 2>&1 | grep -c 'warning.*deprecated'`
Expected: small or zero count (ideally zero — only out-of-test deprecation warnings should remain, if any).

- [ ] **Step 4: Run tests**

Run: `cargo test --all-features`
Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs src/lib.rs
git commit -m "$(cat <<'EOF'
test: silence deprecation warnings in tests of deprecated paths

The deprecated MediaSource::<()>::from_bytes and parse_*_from_bytes
methods are still exercised by the existing test suite (they need
to keep working through v3.x). Annotate those specific tests with
#[allow(deprecated)] so deprecation warnings don't pollute CI.

User-facing deprecation warnings (when callers use the deprecated
methods in their own code) are unaffected.
EOF
)"
```

---

## Task 0.7: Mark top-level `read_*_from_bytes` helpers `#[deprecated]`

**Files:**
- Modify: `src/lib.rs` (top-level `read_*_from_bytes` functions)

- [ ] **Step 1: Locate the top-level helpers**

Run: `grep -n 'pub fn read_exif_from_bytes\|pub fn read_exif_iter_from_bytes\|pub fn read_track_from_bytes\|pub fn read_metadata_from_bytes' src/lib.rs`
Expected: 4 hits, around lines 240, 247, 254, 263.

- [ ] **Step 2: Add `#[deprecated]` and update bodies to delegate via `from_memory`**

Edit `src/lib.rs` — for each helper, add a deprecation attribute and change the body to use `from_memory`:

```rust
/// **Deprecated since v3.3.0**: use [`read_exif`] with
/// [`MediaSource::from_memory`] directly.
#[deprecated(
    since = "3.3.0",
    note = "Use `read_exif` with `MediaSource::from_memory`."
)]
pub fn read_exif_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Exif> {
    let iter = read_exif_iter_from_bytes(bytes)?;
    Ok(iter.into())
}

#[deprecated(
    since = "3.3.0",
    note = "Use `read_exif_iter` with `MediaSource::from_memory`."
)]
pub fn read_exif_iter_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<ExifIter> {
    let ms = MediaSource::from_memory(bytes)?;
    let mut parser = MediaParser::new();
    parser.parse_exif(ms)
}

#[deprecated(
    since = "3.3.0",
    note = "Use `read_track` with `MediaSource::from_memory`."
)]
pub fn read_track_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<TrackInfo> {
    let ms = MediaSource::from_memory(bytes)?;
    let mut parser = MediaParser::new();
    parser.parse_track(ms)
}

#[deprecated(
    since = "3.3.0",
    note = "Use `read_metadata` with `MediaSource::from_memory`."
)]
pub fn read_metadata_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Metadata> {
    let ms = MediaSource::from_memory(bytes)?;
    let mut parser = MediaParser::new();
    match ms.kind() {
        MediaKind::Image => parser.parse_exif(ms).map(|i| Metadata::Exif(i.into())),
        MediaKind::Track => parser.parse_track(ms).map(Metadata::Track),
    }
}
```

(Note: the bodies now use `from_memory` and the unified `parse_*` methods. The deprecated functions still work; they just internally use the new path. This eliminates the dependence on the deprecated `from_bytes`/`parse_*_from_bytes` chain inside the crate.)

- [ ] **Step 3: Run tests**

Run: `cargo test --all-features`
Expected: all green (the `read_*_from_bytes` tests already have `#[allow(deprecated)]` from Task 0.6).

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs
git commit -m "$(cat <<'EOF'
deprecate: top-level read_*_from_bytes helpers

read_exif_from_bytes, read_exif_iter_from_bytes,
read_track_from_bytes, read_metadata_from_bytes are now deprecated
and delegate internally to the unified MediaSource::from_memory +
parse_* path. Removed in v4.
EOF
)"
```

---

## Task 0.8: Migrate top-level `lib.rs` doctests from `from_bytes` to `from_memory`

**Files:**
- Modify: `src/lib.rs` (module-level docstring, line 1-130)

- [ ] **Step 1: Locate `from_bytes` references in the lib.rs module docstring**

Run: `grep -n 'from_bytes\|read_exif_from_bytes' src/lib.rs | head -20`
Expected: hits both inside doctests (`//!` blocks at top of file) and in pub fn bodies. Inspect each.

- [ ] **Step 2: Migrate the "Reading from in-memory bytes" doctest**

Edit `src/lib.rs` — find the `//! # Reading from in-memory bytes` section (around line 57) and replace the example:

```rust
//! ```rust
//! use nom_exif::{MediaSource, MediaParser, ExifTag};
//!
//! let raw = std::fs::read("./testdata/exif.jpg")?;
//! let ms = MediaSource::from_memory(raw)?;
//! let mut parser = MediaParser::new();
//! let iter = parser.parse_exif(ms)?;
//! let exif: nom_exif::Exif = iter.into();
//! assert_eq!(exif.get(ExifTag::Make).and_then(|v| v.as_str()), Some("vivo"));
//! # Ok::<(), nom_exif::Error>(())
//! ```
```

(Replaces the `read_exif_from_bytes` doctest in the original.)

- [ ] **Step 3: Add a brief deprecation note at the bottom of the in-memory section**

After the migrated doctest, add:

```rust
//! v3.0-style API (deprecated since v3.3): the top-level
//! `read_exif_from_bytes` family and `MediaSource::<()>::from_bytes`
//! still compile but produce deprecation warnings. Migrate to
//! `MediaSource::from_memory` + `parse_exif` / `read_exif`.
```

- [ ] **Step 4: Verify doctest compiles and passes**

Run: `cargo test --all-features --doc`
Expected: doctests pass.

- [ ] **Step 5: Run `cargo fmt` and `cargo doc` to verify**

```bash
cargo fmt --check
cargo doc --no-deps --all-features 2>&1 | grep -i warning | head
```
Expected: no fmt issues, no new doc warnings.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs
git commit -m "$(cat <<'EOF'
docs: migrate lib.rs in-memory examples to from_memory

The module-level docstring's "Reading from in-memory bytes" section
now demonstrates the v3.3 unified API. A short deprecation note at
the section's end points existing read_*_from_bytes users to the
new path.
EOF
)"
```

---

## Task 0.9: Migrate `README.md` examples from `from_bytes` to `from_memory`

**Files:**
- Modify: `README.md` (the In-Memory Bytes section, around line 107-138)

- [ ] **Step 1: Locate `from_bytes` references**

Run: `grep -n 'from_bytes\|read_exif_from_bytes' README.md`
Expected: several hits in the "In-Memory Bytes" section.

- [ ] **Step 2: Replace the section's primary example**

Edit `README.md` — in the "In-Memory Bytes" section, replace the existing example with the `from_memory` form:

```rust
use nom_exif::{MediaSource, MediaParser, ExifTag};

let raw: Vec<u8> = std::fs::read("./testdata/exif.jpg")?;
let ms = MediaSource::from_memory(raw)?;
let mut parser = MediaParser::new();
let iter = parser.parse_exif(ms)?;
let exif: nom_exif::Exif = iter.into();
let make = exif.get(ExifTag::Make).and_then(|v| v.as_str());
# let _ = make; Ok::<(), nom_exif::Error>(())
```

- [ ] **Step 3: Replace the batch-processing example**

```rust
use nom_exif::{MediaParser, MediaSource};

let mut parser = MediaParser::new();
let raw = std::fs::read("./testdata/exif.jpg")?;
let ms = MediaSource::from_memory(raw)?;
let iter = parser.parse_exif(ms)?;
# let _ = iter; Ok::<(), nom_exif::Error>(())
```

- [ ] **Step 4: Add a deprecation note paragraph**

After the migrated examples, add:

```markdown
**Migration note (v3.3+)**: `MediaSource::<()>::from_bytes`,
`read_exif_from_bytes`, and the other `*_from_bytes` helpers are
deprecated since v3.3.0 and will be removed in v4. Replace with
`MediaSource::from_memory` + `parse_exif` / `read_exif`. See
[`docs/MIGRATION.md`](docs/MIGRATION.md).
```

- [ ] **Step 5: Update the "accepts" line**

Find this paragraph in README:

```markdown
`MediaSource::from_bytes` accepts anything convertible into
`bytes::Bytes`: `Vec<u8>`, `&'static [u8]`, `Bytes`, and HTTP-body types
that implement `Into<Bytes>` directly.
```

Replace `from_bytes` with `from_memory`.

- [ ] **Step 6: Verify README still compiles its examples**

Run: `cargo test --all-features --doc 2>&1 | grep -A 2 README`
Expected: README examples (if marked as doctests) pass.

- [ ] **Step 7: Commit**

```bash
git add README.md
git commit -m "$(cat <<'EOF'
docs: migrate README in-memory examples to from_memory

The In-Memory Bytes section now demonstrates the v3.3 unified API.
Adds a Migration Note paragraph linking to docs/MIGRATION.md for
existing v3.0-3.2 users on the deprecated *_from_bytes path.
EOF
)"
```

---

## Task 0.10: Add MIGRATION.md "v3.0 → v3.3" subsection

**Files:**
- Modify: `docs/MIGRATION.md`

- [ ] **Step 1: Locate the migration document structure**

Run: `head -30 docs/MIGRATION.md`
Expected: existing v2 → v3 migration sections.

- [ ] **Step 2: Add a v3.0 → v3.3 subsection**

Edit `docs/MIGRATION.md` — append (or insert at the appropriate section position) the following:

```markdown
## v3.0 → v3.3 (in-memory bytes API rename)

v3.3 unifies the in-memory-bytes parsing path with file/stream
parsing. The v3.0 `*_from_bytes` family is deprecated (still
compiles in v3.x; removed in v4). Migration is mechanical:

| Old (v3.0–v3.2, deprecated) | New (v3.3+) |
|---|---|
| `MediaSource::<()>::from_bytes(bytes)` | `MediaSource::from_memory(bytes)` |
| `parser.parse_exif_from_bytes(ms)` | `parser.parse_exif(ms)` (after `from_memory`) |
| `parser.parse_track_from_bytes(ms)` | `parser.parse_track(ms)` (after `from_memory`) |
| `read_exif_from_bytes(bytes)` | `read_exif(...)` after wrapping bytes via `MediaSource::from_memory`; or just keep the old call (deprecated, still works) |
| `read_exif_iter_from_bytes(bytes)` | `read_exif_iter(...)` analog |
| `read_track_from_bytes(bytes)` | `read_track(...)` analog |
| `read_metadata_from_bytes(bytes)` | `read_metadata(...)` analog |

Behavior and zero-copy semantics are preserved verbatim — `from_memory`
returns `MediaSource<std::io::Empty>` instead of `MediaSource<()>`,
satisfying the existing `<R: Read>` bound on `parse_exif` /
`parse_track` so the unified methods can dispatch on the
`memory: Option<bytes::Bytes>` field at runtime.

Example:

```rust
// v3.0 (deprecated since v3.3)
use nom_exif::{MediaParser, MediaSource};
let raw = std::fs::read("./testdata/exif.jpg")?;
#[allow(deprecated)]
let ms = MediaSource::<()>::from_bytes(raw)?;
let mut parser = MediaParser::new();
#[allow(deprecated)]
let iter = parser.parse_exif_from_bytes(ms)?;

// v3.3+ (preferred)
use nom_exif::{MediaParser, MediaSource};
let raw = std::fs::read("./testdata/exif.jpg")?;
let ms = MediaSource::from_memory(raw)?;
let mut parser = MediaParser::new();
let iter = parser.parse_exif(ms)?;
# let _ = iter;
# Ok::<(), nom_exif::Error>(())
```
```

- [ ] **Step 3: Verify doctest in the new section compiles**

Run: `cargo test --all-features --doc 2>&1 | grep -A 2 MIGRATION`
Expected: passes (or no MIGRATION-specific doctest line, depending on whether the table is run as a doctest).

- [ ] **Step 4: Commit**

```bash
git add docs/MIGRATION.md
git commit -m "$(cat <<'EOF'
docs: add v3.0 → v3.3 migration subsection (in-memory bytes API)

Documents the deprecation of MediaSource::<()>::from_bytes,
parse_*_from_bytes, and read_*_from_bytes in favor of the unified
MediaSource::from_memory + parse_* path. Includes the migration
table and a side-by-side code example.

Removal of the deprecated symbols is scheduled for v4.
EOF
)"
```

---

## Task 0.11: Final verification of phase 0

**Files:** (no code change)

- [ ] **Step 1: Full test suite green**

Run: `cargo test --all-features`
Expected: all tests pass; no errors.

- [ ] **Step 2: Format check clean**

Run: `cargo fmt --check`
Expected: no output (clean).

- [ ] **Step 3: Doc build clean**

Run: `cargo doc --no-deps --all-features 2>&1 | grep -i warning | head`
Expected: no new warnings introduced by this phase.

- [ ] **Step 4: Verify deprecation warnings are present for external callers**

Create a small test file `/tmp/deprecation_test.rs`:

```rust
fn main() {
    let _ = nom_exif::read_exif_from_bytes(b"".to_vec());
}
```

Run: `cd /tmp && rustc --edition 2021 --extern nom_exif=$(find /Users/min/dev/nom-exif/target -name 'libnom_exif*.rlib' | head -1) -L $(find /Users/min/dev/nom-exif/target -name 'deps' | head -1) deprecation_test.rs 2>&1 | grep deprecated`
Expected: a `note: '...' is deprecated since v3.3.0: ...` warning. (This step is informational; the failure mode is "we forgot to deprecate something". OK to skip if the rustc invocation is finicky.)

- [ ] **Step 5: Tag end of phase 0**

```bash
git tag png-p0-done
```

(Tagging is optional but matches the existing v3 phase-completion convention.)

- [ ] **Step 6: Self-check against P0 exit criteria**

Re-read the P0 exit criterion in the master plan:

> `MediaSource::<std::io::Empty>::from_memory` constructor exists. ✓
> `parse_exif<R: Read>` / `parse_track<R: Read>` (sync + async) accept memory-mode sources via runtime branch on `memory.is_some()`. ✓
> `MediaSource::<()>::from_bytes`, `parse_*_from_bytes`, `read_*_from_bytes` are `#[deprecated]`. ✓
> Existing `parse_*_from_bytes` tests still green (with `#[allow(deprecated)]`). ✓
> New parallel tests cover the `from_memory` route. ✓
> README + lib.rs doc examples migrated. ✓
> MIGRATION.md gets v3.0 → v3.3 subsection. ✓
> `cargo test --all-features` green. ✓

Phase 0 complete. Proceed to P1.

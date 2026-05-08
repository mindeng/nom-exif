# v3 Phase 3 — Parser entry surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reshape the public **entry surface** of the library — `MediaSource`, `MediaParser`, the new top-level `read_*` convenience functions, and the cargo-feature names — to match the v3 spec, while leaving downstream value/iter types (`Exif`, `ExifIter`, `EntryValue`, `GPSInfo`, …) unchanged for P4/P5 to work on.

**Architecture:** The existing two-axis design (sync/async × seekable/unseekable, encoded as `MediaSource<R, S>` + `Skip<R>` trait) collapses to a single-axis design. The seek-vs-read decision is set once at construction (`seekable` requires `R: Read+Seek`, `unseekable` only `R: Read`) and stored as a captured function pointer inside the source — so the parser only sees `MediaSource<R: Read>` and never has to care which variant it received. Public API surface adds `MediaSource::open`/`from_file` (replacing `file_path`/`file`), `MediaKind::{Image, Track}` + `kind()` method, the two named `MediaParser::parse_exif`/`parse_track` methods (and `_async` variants under feature `tokio`), and a tier of one-shot top-level helpers (`read_exif`, `read_exif_iter`, `read_track`, `read_metadata` + async). The deletions — `tcp_stream`, `file_path`, `file`, `has_exif`/`has_track`, `MediaParser::parse<O>`, the `ParseOutput` trait, `AsyncMediaParser`, `AsyncParseOutput` — happen at the **end** of the phase so the build stays green between tasks.

**Tech Stack:** No new dependencies. Existing `tokio` feature renamed to `tokio` (sic — current name is `async`); existing `json_dump` feature renamed to `serde`. Internal `BufParser` / `AsyncBufParser` traits stay (P2 made them shared); they just stop carrying the `S: Skip<R>` type parameter.

**Spec sections covered:** §3.3 (parser model — full), §3.4 (sync/async — public half; internal half landed in P2), §3.11 (top-level convenience functions), §5.1 (entry/parse migration table), §5.8 (async migration table), §5.9 (cargo-feature rename table), §6.1 line items 4 (`Mime` → `MediaMime` rename, kept `pub(crate)`).

**Spec sections NOT covered (deferred):**
- §3.5–§3.7: `Exif`/`ExifIter` reshape — **P5**.
- §3.6: `EntryValue` accessor matrix — **P4**.
- §3.8: `Rational<T>` field privacy + accessors — **P4**.
- §3.9: `GPSInfo` / `LatLng` enum cleanup — **P4**.
- §3.10: `TrackInfo` cleanup (`gps_info` rename, drop `From<BTreeMap>` / `IntoIterator`) — **P6**.
- The full `lib.rs` doc-comment rewrite — **P6**. P3 only stubs it out.
- `prelude` module — **P6**.

---

## File map

- **Modify:**
  - `Cargo.toml` — rename features `async` → `tokio`, `json_dump` → `serde`. The `[dependencies]` `optional = true` lines stay; only the `[features]` table keys change (the dep names `tokio` / `serde` already match the new feature names, so the right-hand sides become `tokio = ["dep:tokio"]` / `serde = ["dep:serde"]` — see Task 1 for the exact diff).
  - `src/parser.rs` — the load-bearing file.
    - Add `MediaKind` enum + `MediaSource::kind()`.
    - Add `MediaSource::open(path)` + `MediaSource::from_file(file)`.
    - Drop `S` type parameter; replace `phantom: PhantomData<S>` with `skip_by_seek: SkipBySeekFn<R>` field; constructors set the fn-pointer based on their `R` bound.
    - Refactor `BufParser` trait so `clear_and_skip` / `load_and_parse_with_offset` take `&mut R` plus the skip fn pointer instead of the `S: Skip<R>` parameter.
    - Add `MediaParser::parse_exif` and `MediaParser::parse_track`.
    - Add `MediaParser::parse_exif_async` and `MediaParser::parse_track_async` (gated on `feature = "tokio"`).
    - End-of-phase deletes: `tcp_stream`, `file_path`, `file`, `has_exif`, `has_track`, `parse<O>`, `ParseOutput` trait + impls.
  - `src/parser_async.rs` — mirror changes for `AsyncMediaSource`. End-of-phase: delete the entire file's `AsyncMediaParser` struct (its parse methods migrate to `MediaParser` in Task 9).
  - `src/skip.rs` — after Task 6/7, the public `Skip` / `AsyncSkip` traits and `Seekable` / `Unseekable` zero-sized types are no longer used as type parameters. Most likely: keep the file but reduce it to two thin sync+async helper functions (`skip_by_seek_seekable` / `skip_by_read`); delete the trait + ZST. Final shape decided in Task 6/7 — pre-existing tests in `src/skip.rs` get rewritten to test the helpers directly. Delete in Task 13.
  - `src/exif.rs` — `parse_exif_iter::<R, S>` and `parse_exif_iter_async::<R, S>` lose their `S` parameter; signatures become `parse_exif_iter<R: Read>(parser, mime_img, reader, skip_by_seek)`. (Touched by Tasks 6/7.)
  - `src/file.rs` — rename internal `Mime` → `MediaMime` (and the `MimeImage` / `MimeVideo` sub-enums to `MediaMimeImage` / `MediaMimeTrack`; the variant `Mime::Video` becomes `MediaMime::Track` because v3 §3.3 unifies video and audio under the `Track` label). Stays `pub(crate)`. (Task 1 — done alongside cargo-feature renames since both are mechanical search-and-replace.)
  - `src/lib.rs` —
    - Re-export `MediaKind`, `Metadata`, `read_exif`, `read_exif_iter`, `read_track`, `read_metadata`, and async variants.
    - Add the new `read_*` convenience function bodies.
    - Update `#[cfg(feature = "async")]` → `#[cfg(feature = "tokio")]` everywhere.
    - End-of-phase: stub the v2 `//!` doc-comment with a minimal v3 placeholder (full rewrite is P6).
  - `src/values.rs`, `src/exif/tags.rs` — the four `#[cfg(feature = "json_dump")]` sites become `feature = "serde"` (Task 1).
  - `examples/rexiftool.rs` — migrate to v3 entry surface in Task 11; `#[cfg(feature = "json_dump")]` becomes `feature = "serde"` in Task 1.
  - `fuzz/fuzz_targets/media_parser.rs` — migrate to v3 surface in Task 11.
  - `src/cr3.rs`, `src/exif/exif_exif.rs`, `src/video.rs` — update doc-comment examples to v3 surface in Task 11.

- **Create:** none. (`Metadata` lives in `lib.rs`, `MediaKind` in `parser.rs`.)

---

## Task 0 — Pre-flight

- [ ] **Step 0.1: Confirm we're at `v3-p2-done`**

Run:
```bash
git describe --tags --exact-match HEAD 2>/dev/null
```
Expected: `v3-p2-done`. If empty or different, abort and ask the user — work has happened on top of P2 and the plan may need adjustment.

- [ ] **Step 0.2: Baseline build green**

Run:
```bash
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: `test result: ok. 201 passed; 0 failed; 1 ignored;` (or higher pass count if P2 added tests).

- [ ] **Step 0.3: Snapshot current entry-surface symbols (for the deletion checklist in Task 13)**

Run:
```bash
grep -nE 'pub (fn|struct|enum|trait) ' src/parser.rs src/parser_async.rs | grep -v 'pub(crate)' > /tmp/v3-p3-entry-symbols.before.txt
wc -l /tmp/v3-p3-entry-symbols.before.txt
```
Save the file. Used in Task 13 to verify the deletion list is exhaustive.

---

## Task 1 — Cargo feature renames + internal `Mime` → `MediaMime` rename

Spec: §5.9, §6.1 line item 4. Mechanical search-and-replace; do it first so all subsequent tasks operate on the renamed paths.

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs:331,352` (the two `#[cfg(feature = "async")]` blocks)
- Modify: `src/skip.rs:6,46,108,120,144,161` (six `#[cfg(feature = "async")]`)
- Modify: `src/exif.rs:151,241` (two `#[cfg(feature = "async")]`)
- Modify: `src/values.rs:9,488,702` (three `feature = "json_dump"`)
- Modify: `src/exif/tags.rs:6,10,75` (three `feature = "json_dump"`)
- Modify: `src/parser_async.rs:246` (one `#[cfg(feature = "async")]` in a doc-comment — keep it pointing at the *old* name? **No** — examples in doc-comments are user-facing, they must use the new name)
- Modify: `examples/rexiftool.rs:25,27,131,134` (four `feature = "json_dump"`)
- Modify: `src/file.rs` — rename `enum Mime`, `enum MimeImage`, `enum MimeVideo`, `Mime::Image`, `Mime::Video`, `unwrap_image`, `unwrap_video` plus all use-sites.

- [ ] **Step 1.1: Edit `Cargo.toml` features table**

Replace:
```toml
[features]
# default = ["async", "json_dump"]
async = ["tokio"]
json_dump = ["serde"]
```
With:
```toml
[features]
# default = ["tokio", "serde"]
tokio = ["dep:tokio"]
serde = ["dep:serde"]
```

Note: switching to `dep:` syntax decouples the feature name from the optional-dep auto-feature that cargo would otherwise create. (Without `dep:`, cargo synthesizes a feature named after each optional dep, which would clash with our explicit `tokio` / `serde` features.)

- [ ] **Step 1.2: Replace all `feature = "async"` → `feature = "tokio"` in `.rs`**

Run:
```bash
grep -rln 'feature = "async"' src examples | xargs sed -i '' 's/feature = "async"/feature = "tokio"/g'
```
(macOS BSD sed: the `-i ''` form is correct.)

Verify:
```bash
grep -rn 'feature = "async"' src examples
```
Expected: no output.

- [ ] **Step 1.3: Replace all `feature = "json_dump"` → `feature = "serde"` in `.rs`**

Run:
```bash
grep -rln 'feature = "json_dump"' src examples | xargs sed -i '' 's/feature = "json_dump"/feature = "serde"/g'
```

Verify:
```bash
grep -rn 'feature = "json_dump"' src examples
```
Expected: no output.

- [ ] **Step 1.4: Build with the new feature names**

Run:
```bash
cargo build --no-default-features --features tokio,serde 2>&1 | tail -20
```
Expected: clean build, no warnings about unknown features.

- [ ] **Step 1.5: Build with no features**

Run:
```bash
cargo build --no-default-features 2>&1 | tail -10
```
Expected: clean build.

- [ ] **Step 1.6: Internal rename `Mime` → `MediaMime`**

Edit `src/file.rs`. The four enums become:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum MediaMime {
    Image(MediaMimeImage),
    Track(MediaMimeTrack),
}

impl MediaMime {
    pub fn unwrap_image(self) -> MediaMimeImage {
        match self {
            MediaMime::Image(val) => val,
            MediaMime::Track(_) => panic!("called `MediaMime::unwrap_image()` on a `MediaMime::Track`"),
        }
    }
    pub fn unwrap_track(self) -> MediaMimeTrack {
        match self {
            MediaMime::Image(_) => panic!("called `MediaMime::unwrap_track()` on a `MediaMime::Image`"),
            MediaMime::Track(val) => val,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum MediaMimeImage {
    Jpeg,
    Heic,
    Heif,
    Tiff,
    Raf,
    Cr3,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum MediaMimeTrack {
    QuickTime,
    Mp4,
    Webm,
    Matroska,
    _3gpp,
}
```

Note the second-level rename: `MimeVideo` → `MediaMimeTrack` (because v3 unifies "video and audio" under the `Track` label per spec §3.3). The variant `Mime::Video` becomes `MediaMime::Track`. Update `unwrap_video` → `unwrap_track`.

- [ ] **Step 1.7: Propagate `Mime` → `MediaMime` rename through all use-sites**

Run:
```bash
grep -rln '\bMime\b\|MimeImage\|MimeVideo\|unwrap_video' src
```

Touched files (verify after editing):
- `src/file.rs` — already done in Step 1.6.
- `src/parser.rs` — `mime: Mime` field, `Mime::Image`/`Mime::Video` matches in `has_track`/`has_exif`, `unwrap_image`/`unwrap_video` calls in `ParseOutput::parse` impls.
- `src/parser_async.rs` — same field + match patterns.
- `src/exif.rs` — `MimeImage` references in function signatures and match patterns; `mime_img: MimeImage` parameter.
- `src/cr3.rs`, `src/heif.rs`, `src/jpeg.rs`, `src/raf.rs` — likely also reference `MimeImage` variants. Grep first.

Strategy: do all renames in one commit. The `#[case(... Image(Heic) ...)]` test cases in `src/file.rs` need to become `MediaMime::Image(MediaMimeImage::Heic)`.

- [ ] **Step 1.8: Smoke test**

Run:
```bash
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: 201 passed.

Run:
```bash
cargo build --examples --all-features 2>&1 | tail -3
```
Expected: clean.

- [ ] **Step 1.9: Commit**

```bash
git add -A
git commit -m "refactor!(features)!: rename async→tokio, json_dump→serde; Mime→MediaMime

- Cargo features renamed per v3 spec §5.9 / §8.7 / §8.8: 'async' is misleading
  (only ever wired to tokio), 'json_dump' is misleading (it's serde derive,
  not a JSON dumper). Switching to dep: syntax to keep feature names from
  colliding with auto-features.
- Internal Mime enum renamed to MediaMime; inner MimeVideo→MediaMimeTrack
  (audio and video both classified as 'Track' in v3 §3.3).
"
```

---

## Task 2 — Add `MediaKind` + `MediaSource::kind()` (sync)

Spec: §3.3.

**Files:**
- Modify: `src/parser.rs` — add `MediaKind` enum + `kind()` method on `MediaSource<R, S>`.
- Modify: `src/lib.rs:328` — `pub use parser::{MediaKind, MediaParser, MediaSource};`

- [ ] **Step 2.1: Write the failing test**

Add to `src/parser.rs` test module (after the `parse_track_on_image_returns_track_not_found` test):

```rust
#[test]
fn media_kind_classifies_image_and_track() {
    let img = MediaSource::file_path("testdata/exif.jpg").unwrap();
    assert_eq!(img.kind(), MediaKind::Image);

    let trk = MediaSource::file_path("testdata/meta.mov").unwrap();
    assert_eq!(trk.kind(), MediaKind::Track);
}
```

Run:
```bash
cargo test --lib media_kind_classifies_image_and_track 2>&1 | tail -5
```
Expected: FAIL — `MediaKind` not in scope.

- [ ] **Step 2.2: Implement `MediaKind` + `kind()`**

Add to `src/parser.rs`, just after the `MediaSource` struct definition (around line 50):

```rust
/// Top-level classification of a media source.
///
/// `Image` files carry EXIF metadata (parse with `MediaParser::parse_exif`);
/// `Track` files are time-based containers — video, audio, or both — and
/// carry track-info metadata (parse with `MediaParser::parse_track`). Pure
/// audio containers like `.mka` are classified as `Track`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Track,
}
```

(Closed enum per spec §8.6 — no `#[non_exhaustive]`.)

Add to the existing `impl<R: Read, S: Skip<R>> MediaSource<R, S>` block (around line 64, alongside `has_track` / `has_exif`):

```rust
pub fn kind(&self) -> MediaKind {
    match self.mime {
        MediaMime::Image(_) => MediaKind::Image,
        MediaMime::Track(_) => MediaKind::Track,
    }
}
```

(After Task 1, `Mime::Image` / `Mime::Video` became `MediaMime::Image` / `MediaMime::Track`.)

- [ ] **Step 2.3: Re-export `MediaKind`**

Edit `src/lib.rs` line 328:
```rust
pub use parser::{MediaKind, MediaParser, MediaSource};
```

- [ ] **Step 2.4: Verify the new test passes**

Run:
```bash
cargo test --lib media_kind_classifies_image_and_track 2>&1 | tail -5
```
Expected: PASS.

- [ ] **Step 2.5: Verify nothing else broke**

Run:
```bash
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: 202 passed (one more than baseline).

- [ ] **Step 2.6: Commit**

```bash
git add -A
git commit -m "feat(parser): MediaKind enum + MediaSource::kind() method

Adds the closed Image/Track classification per v3 spec §3.3. has_exif()
and has_track() coexist for now; deletion lands at end of P3."
```

---

## Task 3 — Add `AsyncMediaSource::kind()`

Spec: §3.3 (async parity).

**Files:**
- Modify: `src/parser_async.rs` — add `kind()` method.

- [ ] **Step 3.1: Implement `AsyncMediaSource::kind()`**

Add to the `impl<R: AsyncRead + Unpin, S: AsyncSkip<R>> AsyncMediaSource<R, S>` block (around line 39, alongside `has_track` / `has_exif`):

```rust
pub fn kind(&self) -> crate::MediaKind {
    match self.mime {
        crate::file::MediaMime::Image(_) => crate::MediaKind::Image,
        crate::file::MediaMime::Track(_) => crate::MediaKind::Track,
    }
}
```

- [ ] **Step 3.2: Add a smoke test**

Add to `src/parser_async.rs` test module:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn async_media_kind_classifies_image_and_track() {
    let img = AsyncMediaSource::file_path("testdata/exif.jpg").await.unwrap();
    assert_eq!(img.kind(), crate::MediaKind::Image);

    let trk = AsyncMediaSource::file_path("testdata/meta.mov").await.unwrap();
    assert_eq!(trk.kind(), crate::MediaKind::Track);
}
```

- [ ] **Step 3.3: Verify**

Run:
```bash
cargo test --lib --all-features async_media_kind 2>&1 | tail -5
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: PASS, total 203.

- [ ] **Step 3.4: Commit**

```bash
git add -A
git commit -m "feat(parser_async): AsyncMediaSource::kind() for parity with sync"
```

---

## Task 4 — Add `MediaSource::open` and `MediaSource::from_file`

Spec: §3.3.

**Files:**
- Modify: `src/parser.rs:119-127`.

- [ ] **Step 4.1: Write the failing test**

Add to `src/parser.rs` test module:

```rust
#[test]
fn media_source_open_and_from_file() {
    use std::fs::File;

    // open(path) opens the file internally
    let ms = MediaSource::open("testdata/exif.jpg").unwrap();
    assert_eq!(ms.kind(), MediaKind::Image);

    // from_file(file) takes an already-open File
    let f = File::open("testdata/exif.jpg").unwrap();
    let ms = MediaSource::from_file(f).unwrap();
    assert_eq!(ms.kind(), MediaKind::Image);
}
```

Run:
```bash
cargo test --lib media_source_open_and_from_file 2>&1 | tail -5
```
Expected: FAIL — `open` / `from_file` not found.

- [ ] **Step 4.2: Implement**

Replace the existing `impl MediaSource<File, Seekable>` block (around line 119) with:

```rust
impl MediaSource<File, Seekable> {
    /// Open a file at `path` and parse its header to detect the media format.
    ///
    /// This is the v3-preferred entry point for the common case of "I have a
    /// path on disk". For an already-open file handle use [`Self::from_file`];
    /// for a generic `Read + Seek` source use [`Self::seekable`].
    pub fn open<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::seekable(File::open(path)?)
    }

    /// Wrap an already-open `File` and parse its header.
    pub fn from_file(file: File) -> crate::Result<Self> {
        Self::seekable(file)
    }

    // v2-shape constructors (deleted at end of P3; tests still use them).
    pub fn file_path<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::open(path)
    }

    pub fn file(file: File) -> crate::Result<Self> {
        Self::from_file(file)
    }
}
```

I.e.: `file_path` and `file` survive as one-line forwarders to `open` / `from_file`. They get deleted in Task 13.

- [ ] **Step 4.3: Verify**

Run:
```bash
cargo test --lib media_source_open_and_from_file 2>&1 | tail -5
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: PASS, total 204.

- [ ] **Step 4.4: Commit**

```bash
git add -A
git commit -m "feat(parser): MediaSource::open and MediaSource::from_file

v3-spec §3.3 entry constructors. file_path/file kept as forwarders until
end of P3."
```

---

## Task 5 — Add `AsyncMediaSource::open` and `AsyncMediaSource::from_file`

Spec: §3.3 (async parity).

**Files:**
- Modify: `src/parser_async.rs:83-91`.

- [ ] **Step 5.1: Implement**

Replace the existing `impl AsyncMediaSource<File, Seekable>` block with:

```rust
impl AsyncMediaSource<File, Seekable> {
    /// Open a file at `path` (via `tokio::fs::File`) and parse its header.
    pub async fn open<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::build(File::open(path).await?).await
    }

    /// Wrap an already-open async `File` and parse its header.
    pub async fn from_file(file: File) -> crate::Result<Self> {
        Self::build(file).await
    }

    // v2-shape constructors (deleted at end of P3).
    pub async fn file_path<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::open(path).await
    }

    pub async fn file(file: File) -> crate::Result<Self> {
        Self::from_file(file).await
    }
}
```

- [ ] **Step 5.2: Smoke test**

Add to `src/parser_async.rs` test module:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn async_media_source_open_and_from_file() {
    let ms = AsyncMediaSource::open("testdata/exif.jpg").await.unwrap();
    assert_eq!(ms.kind(), crate::MediaKind::Image);

    let f = tokio::fs::File::open("testdata/exif.jpg").await.unwrap();
    let ms = AsyncMediaSource::from_file(f).await.unwrap();
    assert_eq!(ms.kind(), crate::MediaKind::Image);
}
```

Run:
```bash
cargo test --lib --all-features async_media_source_open 2>&1 | tail -5
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: PASS, total 205.

- [ ] **Step 5.3: Commit**

```bash
git add -A
git commit -m "feat(parser_async): AsyncMediaSource::open / from_file"
```

---

## Task 6 — Drop `S` type parameter from `MediaSource<R>` (sync internal refactor)

Spec: §3.3 (last paragraph). The biggest internal change in P3.

**Approach:** the v2 design uses a phantom type parameter (`S = Seekable | Unseekable`) plus a trait (`Skip<R>`) to dispatch the seek-vs-read choice at compile time. v3 collapses this to a captured function pointer on the source struct. The `seekable` constructor still requires `R: Read + Seek` (because its closure calls `r.seek_relative`); the `unseekable` constructor only requires `R: Read`. After construction, the parser sees just `MediaSource<R>` with a known `fn(&mut R, u64) -> io::Result<bool>` it can call.

**Files:**
- Modify: `src/parser.rs` — drop `S` from `MediaSource`, `BufParser` trait methods, `ParseOutput<R, S>` trait → `ParseOutput<R>`, `MediaParser::parse<R, S, O>` → `MediaParser::parse<R, O>`.
- Modify: `src/exif.rs` — `parse_exif_iter::<R, S>` → `parse_exif_iter<R: Read>`, taking `skip_by_seek` as a parameter.
- Modify: `src/skip.rs` — keep file but no longer reference it as a type-parameter source. Existing tests rewritten to test the helper functions directly.

- [ ] **Step 6.1: Define the `SkipBySeekFn<R>` type alias and rewrite `MediaSource<R>`**

Edit `src/parser.rs`. Near the top of the file (after the imports), add:

```rust
/// A function that tries to skip `n` bytes of `reader` by seeking. Returns
/// `Ok(true)` on success, `Ok(false)` if the reader does not support seek
/// (so the caller should fall back to reading-and-discarding), or
/// `Err(io::Error)` if seek itself failed (e.g. truncated file handle).
///
/// This is captured at construction time by `MediaSource::seekable` /
/// `unseekable`, replacing the v2 `S: Skip<R>` phantom parameter with a
/// runtime fn pointer.
pub(crate) type SkipBySeekFn<R> = fn(&mut R, u64) -> io::Result<bool>;
```

Replace the `MediaSource` struct (lines 44-49) with:

```rust
pub struct MediaSource<R> {
    pub(crate) reader: R,
    pub(crate) buf: Vec<u8>,
    pub(crate) mime: crate::file::MediaMime,
    pub(crate) skip_by_seek: SkipBySeekFn<R>,
}
```

Drop the `phantom: PhantomData<S>` field. Drop the `marker::PhantomData` import if no longer used.

- [ ] **Step 6.2: Rewrite the `MediaSource` impl blocks**

Replace lines 51-133 (the four impl blocks: Debug, build, seekable, unseekable, file ops, tcp_stream) with:

```rust
impl<R> Debug for MediaSource<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaSource")
            .field("mime", &self.mime)
            .finish_non_exhaustive()
    }
}

impl<R: Read> MediaSource<R> {
    fn build(mut reader: R, skip_by_seek: SkipBySeekFn<R>) -> crate::Result<Self> {
        let mut buf = Vec::with_capacity(HEADER_PARSE_BUF_SIZE);
        reader
            .by_ref()
            .take(HEADER_PARSE_BUF_SIZE as u64)
            .read_to_end(&mut buf)?;
        let mime: crate::file::MediaMime = buf.as_slice().try_into()?;
        Ok(Self { reader, buf, mime, skip_by_seek })
    }

    pub fn kind(&self) -> MediaKind {
        match self.mime {
            crate::file::MediaMime::Image(_) => MediaKind::Image,
            crate::file::MediaMime::Track(_) => MediaKind::Track,
        }
    }

    // Legacy alongside; deleted in Task 13.
    pub fn has_track(&self) -> bool {
        matches!(self.mime, crate::file::MediaMime::Track(_))
    }
    pub fn has_exif(&self) -> bool {
        matches!(self.mime, crate::file::MediaMime::Image(_))
    }
}

impl<R: Read + Seek> MediaSource<R> {
    pub fn seekable(reader: R) -> crate::Result<Self> {
        Self::build(reader, |r, n| {
            let signed: i64 = n.try_into().map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
            r.seek_relative(signed)?;
            Ok(true)
        })
    }
}

impl<R: Read> MediaSource<R> {
    pub fn unseekable(reader: R) -> crate::Result<Self> {
        Self::build(reader, |_, _| Ok(false))
    }
}

impl MediaSource<File> {
    pub fn open<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::seekable(File::open(path)?)
    }
    pub fn from_file(file: File) -> crate::Result<Self> {
        Self::seekable(file)
    }
    // Legacy aliases; deleted in Task 13.
    pub fn file_path<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::open(path)
    }
    pub fn file(file: File) -> crate::Result<Self> {
        Self::from_file(file)
    }
}

impl MediaSource<TcpStream> {
    // Deleted in Task 13.
    pub fn tcp_stream(stream: TcpStream) -> crate::Result<Self> {
        Self::unseekable(stream)
    }
}
```

Notes:
- Two `impl<R: Read>` blocks coexist: one for the always-available accessors and `unseekable`, one only with the unseekable constructor. Rust allows multiple impl blocks on the same type with the same bounds — they merge.
- `seek_relative` is the same `Read+Seek` method used by `Skip<R>` for `Seekable`.

- [ ] **Step 6.3: Strip `S` from `BufParser` / `ParseOutput` / `MediaParser::parse`**

Edit `src/parser.rs` further. Find the `BufParser` trait and remove all `S: Skip<R>` parameters from its methods. Replace `S::skip_by_seek(reader, n)` calls inside `clear_and_skip` with a direct call to a fn-pointer parameter:

```rust
pub(crate) trait BufParser: Buf + Debug {
    fn fill_buf<R: Read>(&mut self, reader: &mut R, size: usize) -> io::Result<usize>;

    fn load_and_parse<R: Read, P, O>(
        &mut self,
        reader: &mut R,
        skip_by_seek: SkipBySeekFn<R>,
        mut parse: P,
    ) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8], Option<ParsingState>) -> Result<O, ParsingErrorState>,
    {
        self.load_and_parse_with_offset(reader, skip_by_seek, |data, _, state| parse(data, state), 0)
    }

    #[tracing::instrument(skip_all)]
    fn load_and_parse_with_offset<R: Read, P, O>(
        &mut self,
        reader: &mut R,
        skip_by_seek: SkipBySeekFn<R>,
        mut parse: P,
        offset: usize,
    ) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8], usize, Option<ParsingState>) -> Result<O, ParsingErrorState>,
    {
        if offset >= self.buffer().len() {
            self.fill_buf(reader, MIN_GROW_SIZE)?;
        }
        let mut parsing_state: Option<ParsingState> = None;
        loop {
            match parse_loop_step(self.buffer(), offset, &mut parsing_state, &mut parse) {
                LoopAction::Done(o) => return Ok(o),
                LoopAction::NeedFill(needed) => {
                    let to_read = max(needed, MIN_GROW_SIZE);
                    let n = self.fill_buf(reader, to_read)?;
                    if n == 0 {
                        return Err(ParsedError::NoEnoughBytes);
                    }
                }
                LoopAction::Skip(n) => {
                    self.clear_and_skip(reader, skip_by_seek, n)?;
                }
                LoopAction::Failed(s) => return Err(ParsedError::Failed(s)),
            }
        }
    }

    #[tracing::instrument(skip(reader, skip_by_seek))]
    fn clear_and_skip<R: Read>(
        &mut self,
        reader: &mut R,
        skip_by_seek: SkipBySeekFn<R>,
        n: usize,
    ) -> Result<(), ParsedError> {
        match clear_and_skip_decide(self.buffer().len(), n) {
            SkipPlan::AdvanceOnly => {
                self.set_position(self.position() + n);
                Ok(())
            }
            SkipPlan::ClearAndSkip { extra: skip_n } => {
                self.clear();
                let done = (skip_by_seek)(
                    reader,
                    skip_n.try_into()
                        .map_err(|_| ParsedError::Failed("skip too many bytes".into()))?,
                )?;
                if !done {
                    let mut skipped = 0;
                    while skipped < skip_n {
                        let mut to_skip = skip_n - skipped;
                        to_skip = min(to_skip, MAX_ALLOC_SIZE);
                        let n = self.fill_buf(reader, to_skip)?;
                        skipped += n;
                        if skipped <= skip_n {
                            self.clear();
                        } else {
                            let remain = skipped - skip_n;
                            self.set_position(self.buffer().len() - remain);
                            break;
                        }
                    }
                }
                if self.buffer().is_empty() {
                    self.fill_buf(reader, MIN_GROW_SIZE)?;
                }
                Ok(())
            }
        }
    }
}
```

Then update `ParseOutput` and the two impls:

```rust
pub trait ParseOutput<R>: Sized {
    fn parse(parser: &mut MediaParser, ms: MediaSource<R>) -> crate::Result<Self>;
}

impl<R: Read> ParseOutput<R> for ExifIter {
    fn parse(parser: &mut MediaParser, mut ms: MediaSource<R>) -> crate::Result<Self> {
        if !ms.has_exif() {
            return Err(crate::Error::ExifNotFound);
        }
        parse_exif_iter(parser, ms.mime.unwrap_image(), &mut ms.reader, ms.skip_by_seek)
    }
}

impl<R: Read> ParseOutput<R> for TrackInfo {
    fn parse(parser: &mut MediaParser, mut ms: MediaSource<R>) -> crate::Result<Self> {
        if !ms.has_track() {
            return Err(crate::Error::TrackNotFound);
        }
        let mime_track = ms.mime.unwrap_track();
        let skip_by_seek = ms.skip_by_seek;
        let out = parser.load_and_parse(ms.reader.by_ref(), skip_by_seek, |data, _| {
            parse_track_info(data, mime_track).map_err(|e| ParsingErrorState::new(e, None))
        })?;
        Ok(out)
    }
}
```

And `MediaParser::parse`:
```rust
pub fn parse<R: Read, O: ParseOutput<R>>(
    &mut self,
    mut ms: MediaSource<R>,
) -> crate::Result<O> {
    self.reset();
    self.acquire_buf();
    self.buf_mut().append(&mut ms.buf);
    let res = self.do_parse(ms);
    self.reset();
    res
}

fn do_parse<R: Read, O: ParseOutput<R>>(
    &mut self,
    mut ms: MediaSource<R>,
) -> Result<O, crate::Error> {
    self.fill_buf(&mut ms.reader, INIT_BUF_SIZE)?;
    let res = ParseOutput::parse(self, ms)?;
    Ok(res)
}
```

- [ ] **Step 6.4: Update `parse_exif_iter` in `src/exif.rs`**

Replace:
```rust
pub(crate) fn parse_exif_iter<R: Read, S: Skip<R>>(
    parser: &mut MediaParser,
    mime_img: MimeImage,
    reader: &mut R,
) -> Result<ExifIter, crate::Error> {
    if mime_img == MimeImage::Cr3 {
        return parse_cr3_exif_iter::<R, S>(parser, reader);
    }
    let out = parser.load_and_parse::<R, S, _, _>(reader, |buf, state| {
        extract_exif_range(mime_img, buf, state)
    })?;
    range_to_iter(parser, out)
}
```
with:
```rust
pub(crate) fn parse_exif_iter<R: Read>(
    parser: &mut MediaParser,
    mime_img: MediaMimeImage,
    reader: &mut R,
    skip_by_seek: crate::parser::SkipBySeekFn<R>,
) -> Result<ExifIter, crate::Error> {
    if mime_img == MediaMimeImage::Cr3 {
        return parse_cr3_exif_iter(parser, reader, skip_by_seek);
    }
    let out = parser.load_and_parse(reader, skip_by_seek, |buf, state| {
        extract_exif_range(mime_img, buf, state)
    })?;
    range_to_iter(parser, out)
}

fn parse_cr3_exif_iter<R: Read>(
    parser: &mut MediaParser,
    reader: &mut R,
    skip_by_seek: crate::parser::SkipBySeekFn<R>,
) -> Result<ExifIter, crate::Error> {
    use crate::parser::Buf;
    let cmt_ranges = parser
        .load_and_parse(reader, skip_by_seek, |buf, _state| cr3::extract_all_cmt_ranges(buf))?;
    // ... rest unchanged ...
}
```

Drop the `use crate::skip::Skip;` import in `src/exif.rs`.

- [ ] **Step 6.5: Trim `src/skip.rs` — delete the `Skip<R>` trait + `Seekable`/`Unseekable` types**

After Task 6.4 there are no remaining `S: Skip<R>` consumers in the sync path. Delete the sync portion of `src/skip.rs`:
- Delete `pub struct Seekable(())`.
- Delete `pub struct Unseekable(())`.
- Delete `pub trait Skip<R>` + its two impls.
- Delete the sync `tests::skip` test (replaced by Task 6.1's test).
- **Keep** the async portion (`AsyncSkip`, async impls) — Task 7 handles those.

Update `src/lib.rs:339`:
```rust
pub(crate) use skip::{Seekable, Unseekable};
```
→ delete this line. (`Seekable`/`Unseekable` no longer exist.)

Update `src/parser.rs` imports near the top:
```rust
ExifIter, Seekable, TrackInfo, Unseekable,
```
→ delete `Seekable` / `Unseekable`. Also drop `use crate::skip::Skip;`.

- [ ] **Step 6.6: Verify**

Run:
```bash
cargo build --no-default-features 2>&1 | tail -20
```
Expected: clean. The async path won't compile yet because Task 7 still has `S: AsyncSkip<R>` references in `parser_async.rs`. That's why we build with `--no-default-features` here (no async code compiled).

Run:
```bash
cargo test --lib --no-default-features 2>&1 | tail -3
```
Expected: most tests pass. (The `tokio`-feature tests aren't compiled.)

Run:
```bash
cargo build --all-features 2>&1 | tail -20
```
Expected: **errors** — the async `parser_async.rs` still references `Seekable` / `Unseekable` / `Skip<R>` which we just deleted on the sync side, AND the async Skip-trait machinery still depends on the now-deleted `Seekable`/`Unseekable` ZSTs.

This is fine — Task 7 fixes it. **Do not commit yet** if you broke async; instead, at this point either:
  - **(a)** keep the `Seekable` / `Unseekable` ZSTs alive in `src/skip.rs` (don't delete them in this task — they're still referenced by `AsyncSkip<R>` impls), and only delete the sync `Skip<R>` trait. Then `cargo build --all-features` builds clean.
  - **(b)** stash this task and do Task 7 in the same commit (drop S from async at the same time).

**Recommended: (a).** Keep the two ZST types in `src/skip.rs`; they get deleted in Task 13 once async also stops referring to them. Adjust this step accordingly: in Step 6.5, delete only the sync `Skip<R>` trait + its two impls + the sync test, NOT the ZST types. Re-export in `lib.rs:339` stays.

Re-run:
```bash
cargo build --all-features 2>&1 | tail -10
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: clean build, all tests pass.

- [ ] **Step 6.7: Commit**

```bash
git add -A
git commit -m "refactor(parser)!: drop S type parameter from MediaSource<R>

Per v3 spec §3.3: MediaSource<R> no longer carries the Seekable/Unseekable
phantom. Skip-by-seek capability is captured as fn(&mut R, u64) -> io::Result<bool>
at construction time and stored on the source. ParseOutput<R, S> becomes
ParseOutput<R>; BufParser methods take the skip fn explicitly.

Async path still has S parameter (parser_async.rs) — Task 7 of P3 will
mirror this change."
```

---

## Task 7 — Drop `S` type parameter from `AsyncMediaSource<R>` (async internal refactor)

Spec: §3.3 (async parity).

**Approach:** mirror Task 6 for the async path. Async fn pointers can't be plain `fn` types, so we use `for<'a> fn(&'a mut R, u64) -> Pin<Box<dyn Future<...> + Send + 'a>>`. The `Box::pin` per skip is acceptable overhead — async I/O dwarfs it.

**Files:**
- Modify: `src/parser_async.rs`.
- Modify: `src/skip.rs` — finally delete `AsyncSkip<R>` + the `Seekable` / `Unseekable` ZSTs.
- Modify: `src/exif.rs` — `parse_exif_iter_async::<R, S>` → `parse_exif_iter_async<R: AsyncRead>`.
- Modify: `src/lib.rs` — drop the `pub(crate) use skip::{Seekable, Unseekable};` re-export (after this task there are no consumers).

- [ ] **Step 7.1: Define `AsyncSkipBySeekFn<R>` and rewrite `AsyncMediaSource<R>`**

Edit `src/parser_async.rs`. Near the top (after imports), add:

```rust
pub(crate) type AsyncSkipBySeekFn<R> = for<'a> fn(
    &'a mut R,
    u64,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<bool>> + Send + 'a>>;
```

Replace the `AsyncMediaSource` struct (lines 32-37):
```rust
pub struct AsyncMediaSource<R> {
    pub(crate) reader: R,
    pub(crate) buf: Vec<u8>,
    pub(crate) mime: crate::file::MediaMime,
    pub(crate) skip_by_seek: AsyncSkipBySeekFn<R>,
}
```

Drop the `phantom: PhantomData<S>` field.

- [ ] **Step 7.2: Rewrite the async impl blocks**

Replace lines 39-91 (the four impl blocks) with:

```rust
impl<R> Debug for AsyncMediaSource<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncMediaSource")
            .field("mime", &self.mime)
            .finish_non_exhaustive()
    }
}

impl<R: AsyncRead + Unpin> AsyncMediaSource<R> {
    async fn build(mut reader: R, skip_by_seek: AsyncSkipBySeekFn<R>) -> crate::Result<Self> {
        let mut buf = Vec::with_capacity(HEADER_PARSE_BUF_SIZE);
        (&mut reader)
            .take(HEADER_PARSE_BUF_SIZE as u64)
            .read_to_end(&mut buf)
            .await?;
        let mime: crate::file::MediaMime = buf.as_slice().try_into()?;
        Ok(Self { reader, buf, mime, skip_by_seek })
    }

    pub fn kind(&self) -> crate::MediaKind {
        match self.mime {
            crate::file::MediaMime::Image(_) => crate::MediaKind::Image,
            crate::file::MediaMime::Track(_) => crate::MediaKind::Track,
        }
    }

    // Legacy alongside; deleted in Task 13.
    pub fn has_track(&self) -> bool {
        matches!(self.mime, crate::file::MediaMime::Track(_))
    }
    pub fn has_exif(&self) -> bool {
        matches!(self.mime, crate::file::MediaMime::Image(_))
    }
}

impl<R: AsyncRead + AsyncSeek + Unpin + Send> AsyncMediaSource<R> {
    pub async fn seekable(reader: R) -> crate::Result<Self> {
        let f: AsyncSkipBySeekFn<R> = |r, n| {
            Box::pin(async move {
                use std::io::SeekFrom;
                use tokio::io::AsyncSeekExt;
                let signed: i64 = n.try_into().map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
                r.seek(SeekFrom::Current(signed)).await?;
                Ok(true)
            })
        };
        Self::build(reader, f).await
    }
}

impl<R: AsyncRead + Unpin + Send> AsyncMediaSource<R> {
    pub async fn unseekable(reader: R) -> crate::Result<Self> {
        let f: AsyncSkipBySeekFn<R> = |_, _| Box::pin(async move { Ok(false) });
        Self::build(reader, f).await
    }
}

impl AsyncMediaSource<File> {
    pub async fn open<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::seekable(File::open(path).await?).await
    }
    pub async fn from_file(file: File) -> crate::Result<Self> {
        Self::seekable(file).await
    }
    // Legacy aliases; deleted in Task 13.
    pub async fn file_path<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::open(path).await
    }
    pub async fn file(file: File) -> crate::Result<Self> {
        Self::from_file(file).await
    }
}
```

Drop `marker::PhantomData` import. Drop `crate::skip::AsyncSkip` import. Drop `Seekable`, `Unseekable` from the imports list.

- [ ] **Step 7.3: Update `AsyncBufParser`, `AsyncParseOutput`, `AsyncMediaParser::parse`**

Mirror Step 6.3 for the async trait:

```rust
pub(crate) trait AsyncBufParser: Buf + Debug {
    async fn fill_buf<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        size: usize,
    ) -> io::Result<usize>;

    async fn load_and_parse<R: AsyncRead + Unpin, P, O>(
        &mut self,
        reader: &mut R,
        skip_by_seek: AsyncSkipBySeekFn<R>,
        parse: P,
    ) -> Result<O, ParsedError>
    where
        P: Fn(&[u8], Option<ParsingState>) -> Result<O, ParsingErrorState>,
    {
        self.load_and_parse_with_offset(reader, skip_by_seek, |data, _, state| parse(data, state), 0)
            .await
    }

    #[tracing::instrument(skip_all)]
    async fn load_and_parse_with_offset<R: AsyncRead + Unpin, P, O>(
        &mut self,
        reader: &mut R,
        skip_by_seek: AsyncSkipBySeekFn<R>,
        parse: P,
        offset: usize,
    ) -> Result<O, ParsedError>
    where
        P: Fn(&[u8], usize, Option<ParsingState>) -> Result<O, ParsingErrorState>,
    {
        if offset >= self.buffer().len() {
            self.fill_buf(reader, MIN_GROW_SIZE).await?;
        }
        let mut parsing_state: Option<ParsingState> = None;
        let mut parse = parse;
        loop {
            match parse_loop_step(self.buffer(), offset, &mut parsing_state, &mut parse) {
                LoopAction::Done(o) => return Ok(o),
                LoopAction::NeedFill(needed) => {
                    let to_read = max(needed, MIN_GROW_SIZE);
                    let n = self.fill_buf(reader, to_read).await?;
                    if n == 0 {
                        return Err(ParsedError::NoEnoughBytes);
                    }
                }
                LoopAction::Skip(n) => {
                    self.clear_and_skip(reader, skip_by_seek, n).await?;
                }
                LoopAction::Failed(s) => return Err(ParsedError::Failed(s)),
            }
        }
    }

    #[tracing::instrument(skip(reader, skip_by_seek))]
    async fn clear_and_skip<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        skip_by_seek: AsyncSkipBySeekFn<R>,
        n: usize,
    ) -> Result<(), ParsedError> {
        match clear_and_skip_decide(self.buffer().len(), n) {
            SkipPlan::AdvanceOnly => {
                self.set_position(self.position() + n);
                return Ok(());
            }
            SkipPlan::ClearAndSkip { extra: skip_n } => {
                self.clear();
                let done = (skip_by_seek)(
                    reader,
                    skip_n.try_into()
                        .map_err(|_| ParsedError::Failed("skip too many bytes".into()))?,
                )
                .await?;
                if !done {
                    let mut skipped = 0;
                    while skipped < skip_n {
                        let mut to_skip = skip_n - skipped;
                        to_skip = min(to_skip, MAX_ALLOC_SIZE);
                        let n = self.fill_buf(reader, to_skip).await?;
                        skipped += n;
                        if skipped <= skip_n {
                            self.clear();
                        } else {
                            let remain = skipped - skip_n;
                            self.set_position(self.buffer().len() - remain);
                            break;
                        }
                    }
                }
                if self.buffer().is_empty() {
                    self.fill_buf(reader, MIN_GROW_SIZE).await?;
                }
                Ok(())
            }
        }
    }
}
```

Update `AsyncParseOutput`:
```rust
pub trait AsyncParseOutput<R>: Sized {
    fn parse(
        parser: &mut AsyncMediaParser,
        ms: AsyncMediaSource<R>,
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send;
}

impl<R: AsyncRead + Unpin + Send> AsyncParseOutput<R> for ExifIter {
    async fn parse(
        parser: &mut AsyncMediaParser,
        mut ms: AsyncMediaSource<R>,
    ) -> crate::Result<Self> {
        if !ms.has_exif() {
            return Err(crate::Error::ExifNotFound);
        }
        parse_exif_iter_async(parser, ms.mime.unwrap_image(), &mut ms.reader, ms.skip_by_seek).await
    }
}

impl<R: AsyncRead + Unpin + Send> AsyncParseOutput<R> for TrackInfo {
    async fn parse(
        parser: &mut AsyncMediaParser,
        ms: AsyncMediaSource<R>,
    ) -> crate::Result<Self> {
        let mut ms = ms;
        let mime_track = match ms.mime {
            crate::file::MediaMime::Image(_) => return Err(crate::Error::TrackNotFound),
            crate::file::MediaMime::Track(t) => t,
        };
        let skip = ms.skip_by_seek;
        let out = parser
            .load_and_parse(&mut ms.reader, skip, |data, _| {
                parse_track_info(data, mime_track).map_err(|e| ParsingErrorState::new(e, None))
            })
            .await?;
        Ok(out)
    }
}
```

Update `AsyncMediaParser::parse`:
```rust
pub async fn parse<R: AsyncRead + Unpin, O: AsyncParseOutput<R>>(
    &mut self,
    mut ms: AsyncMediaSource<R>,
) -> crate::Result<O> { /* body unchanged except for the type signature */ }
```

- [ ] **Step 7.4: Update `parse_exif_iter_async` in `src/exif.rs`**

Generic over the parser type `P` so that Task 9 can call this with `&mut MediaParser` (which will also implement `AsyncBufParser` once tokio is enabled). The two calling sites — `AsyncMediaParser` now, `MediaParser` later — both satisfy the bound:

```rust
#[cfg(feature = "tokio")]
#[tracing::instrument(skip(parser, reader, skip_by_seek))]
pub(crate) async fn parse_exif_iter_async<P, R: AsyncRead + Unpin + Send>(
    parser: &mut P,
    mime_img: MediaMimeImage,
    reader: &mut R,
    skip_by_seek: crate::parser_async::AsyncSkipBySeekFn<R>,
) -> Result<ExifIter, crate::Error>
where
    P: crate::parser_async::AsyncBufParser + crate::parser::ShareBuf,
{
    let out = parser
        .load_and_parse(reader, skip_by_seek, |buf, state| {
            extract_exif_range(mime_img, buf, state)
        })
        .await?;
    range_to_iter(parser, out)
}
```

- [ ] **Step 7.5: Delete `Skip` / `AsyncSkip` traits and `Seekable` / `Unseekable` ZSTs from `src/skip.rs`**

After Task 6 deleted the sync `Skip<R>` trait, this task deletes the rest. Replace the whole `src/skip.rs` file with: delete it entirely (the module is no longer used).

Update `src/lib.rs`:
- Delete line 339: `pub(crate) use skip::{Seekable, Unseekable};`
- Delete line 356: `mod skip;`

- [ ] **Step 7.6: Verify**

Run:
```bash
cargo build --all-features 2>&1 | tail -20
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: clean build, all tests pass (205+ from earlier tasks).

If `parse_async` test functions in the (now-deleted) `src/skip.rs` were referenced anywhere, fix those references — they were only used by tests internal to `skip.rs` itself, so deletion of the file deletes them too.

- [ ] **Step 7.7: Commit**

```bash
git add -A
git commit -m "refactor(parser_async)!: drop S type parameter from AsyncMediaSource<R>

Mirror of Task 6 for the async path. AsyncSkipBySeekFn<R> is a fn pointer
to a Pin<Box<dyn Future>>-returning closure; the Box-per-skip overhead is
trivial against actual async I/O.

Removes the entire skip.rs module — its Skip<R> / AsyncSkip<R> traits and
Seekable/Unseekable phantom types are no longer used."
```

---

## Task 8 — Add `MediaParser::parse_exif` and `MediaParser::parse_track`

Spec: §3.3 (MediaParser section).

**Files:**
- Modify: `src/parser.rs` — add the two methods alongside the existing `parse<O>`.

- [ ] **Step 8.1: Write the failing tests**

Add to `src/parser.rs` test module:

```rust
#[test]
fn parse_exif_returns_exif_iter() {
    let mut parser = parser();
    let ms = MediaSource::open("testdata/exif.jpg").unwrap();
    let _: ExifIter = parser.parse_exif(ms).unwrap();
}

#[test]
fn parse_track_returns_track_info() {
    let mut parser = parser();
    let ms = MediaSource::open("testdata/meta.mov").unwrap();
    let _: TrackInfo = parser.parse_track(ms).unwrap();
}

#[test]
fn parse_exif_on_track_returns_exif_not_found_v3() {
    let mut parser = parser();
    let ms = MediaSource::open("testdata/meta.mov").unwrap();
    let res = parser.parse_exif(ms);
    assert!(matches!(res, Err(crate::Error::ExifNotFound)));
}

#[test]
fn parse_track_on_image_returns_track_not_found_v3() {
    let mut parser = parser();
    let ms = MediaSource::open("testdata/exif.jpg").unwrap();
    let res = parser.parse_track(ms);
    assert!(matches!(res, Err(crate::Error::TrackNotFound)));
}
```

Run:
```bash
cargo test --lib parse_exif_returns_exif_iter parse_track_returns_track_info parse_exif_on_track_returns_exif_not_found_v3 parse_track_on_image_returns_track_not_found_v3 2>&1 | tail -10
```
Expected: FAIL — `parse_exif` / `parse_track` methods not found.

- [ ] **Step 8.2: Implement**

Add to `impl MediaParser` block in `src/parser.rs`:

```rust
/// Parse Exif metadata from an image source. Returns `Error::ExifNotFound`
/// if the source is a `Track` (use [`Self::parse_track`] instead).
pub fn parse_exif<R: Read>(&mut self, ms: MediaSource<R>) -> crate::Result<ExifIter> {
    self.parse(ms)
}

/// Parse track info from a video/audio source. Returns `Error::TrackNotFound`
/// if the source is an `Image` (use [`Self::parse_exif`] instead).
pub fn parse_track<R: Read>(&mut self, ms: MediaSource<R>) -> crate::Result<TrackInfo> {
    self.parse(ms)
}
```

These delegate to the existing generic `parse<O>` method. After Task 13 deletes `parse<O>`, the bodies inline (one `ParseOutput::parse` call each).

- [ ] **Step 8.3: Verify**

Run:
```bash
cargo test --lib parse_exif_returns_exif_iter parse_track_returns_track_info parse_exif_on_track_returns_exif_not_found_v3 parse_track_on_image_returns_track_not_found_v3 2>&1 | tail -10
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: PASS, total 209.

- [ ] **Step 8.4: Commit**

```bash
git add -A
git commit -m "feat(parser): MediaParser::parse_exif / parse_track

Named methods replace the v2 generic parse<O>(ms) per spec §3.3. The
generic form remains as a private delegate until end of P3."
```

---

## Task 9 — Add `MediaParser::parse_exif_async` and `MediaParser::parse_track_async`

Spec: §3.3 (MediaParser async section), §3.4. Per spec, **async methods belong on `MediaParser`, not on a separate `AsyncMediaParser`**. We add them now; `AsyncMediaParser` is deleted in Task 13.

**Files:**
- Modify: `src/parser.rs` — add async methods on `MediaParser` gated by `feature = "tokio"`.

**Strategy:** the new methods on `MediaParser` need access to async I/O machinery. Easiest approach: have `MediaParser`'s async methods internally construct an `AsyncMediaParser` and delegate. After Task 13 the `AsyncMediaParser` struct disappears and the method bodies inline directly.

Actually simpler — `MediaParser` and `AsyncMediaParser` already share `BufferedParserState`, so `MediaParser` can implement `AsyncBufParser` itself. Add the two impls and the two methods:

- [ ] **Step 9.1: Make `MediaParser` implement `AsyncBufParser` and `Buf` (it already does)**

In `src/parser.rs`, add at the bottom (gated):
```rust
#[cfg(feature = "tokio")]
mod tokio_impl {
    use super::*;
    use crate::parser_async::{AsyncBufParser, AsyncSkipBySeekFn, AsyncMediaSource};
    use tokio::io::{AsyncRead, AsyncReadExt};

    impl AsyncBufParser for MediaParser {
        async fn fill_buf<R: AsyncRead + Unpin>(
            &mut self,
            reader: &mut R,
            size: usize,
        ) -> std::io::Result<usize> {
            check_fill_size(self.state.buf().len(), size)?;
            let n = reader.take(size as u64).read_to_end(self.state.buf_mut()).await?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            Ok(n)
        }
    }

    impl MediaParser {
        pub async fn parse_exif_async<R: AsyncRead + Unpin + Send>(
            &mut self,
            mut ms: AsyncMediaSource<R>,
        ) -> crate::Result<ExifIter> {
            self.reset();
            self.acquire_buf();
            self.buf_mut().append(&mut ms.buf);
            let res: crate::Result<ExifIter> = async {
                self.fill_buf(&mut ms.reader, INIT_BUF_SIZE).await?;
                if !ms.has_exif() {
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
            .await;
            self.reset();
            res
        }

        pub async fn parse_track_async<R: AsyncRead + Unpin + Send>(
            &mut self,
            mut ms: AsyncMediaSource<R>,
        ) -> crate::Result<TrackInfo> {
            self.reset();
            self.acquire_buf();
            self.buf_mut().append(&mut ms.buf);
            let res: crate::Result<TrackInfo> = async {
                self.fill_buf(&mut ms.reader, INIT_BUF_SIZE).await?;
                let mime_track = match ms.mime {
                    crate::file::MediaMime::Image(_) => return Err(crate::Error::TrackNotFound),
                    crate::file::MediaMime::Track(t) => t,
                };
                let skip = ms.skip_by_seek;
                let out = self
                    .load_and_parse(&mut ms.reader, skip, |data, _| {
                        crate::video::parse_track_info(data, mime_track)
                            .map_err(|e| ParsingErrorState::new(e, None))
                    })
                    .await?;
                Ok(out)
            }
            .await;
            self.reset();
            res
        }
    }

    impl crate::parser_async::ShareBufAsync for MediaParser {
        // If parser_async has its own ShareBuf trait, mirror it here. If it
        // shares the sync ShareBuf trait (P2), no extra impl needed.
    }
}
```

**Note:** the `ShareBufAsync` impl may not be needed if `ShareBuf` is shared between sync/async (which P2 unified via `BufferedParserState`). Verify by reading `src/parser_async.rs:312-316` — if `AsyncMediaParser`'s `ShareBuf` impl uses the same trait as sync, then `MediaParser`'s existing `ShareBuf` impl is sufficient and no extra impl is needed.

Also, `parse_exif_iter_async` needs to be callable with `&mut MediaParser` (not `&mut AsyncMediaParser`). Since both implement `AsyncBufParser`, we may need to make `parse_exif_iter_async` generic over `P: AsyncBufParser + ShareBuf`:

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
    let out = parser
        .load_and_parse(reader, skip_by_seek, |buf, state| {
            extract_exif_range(mime_img, buf, state)
        })
        .await?;
    range_to_iter(parser, out)
}
```

(`range_to_iter` already takes `&mut impl ShareBuf`, so it's compatible.)

- [ ] **Step 9.2: Smoke test**

Add to `src/parser.rs` (cfg=tokio) test module — or to the existing `parser_async.rs` tests:

```rust
#[cfg(feature = "tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn media_parser_parse_exif_async() {
    use crate::parser_async::AsyncMediaSource;
    let mut parser = MediaParser::new();
    let ms = AsyncMediaSource::open("testdata/exif.jpg").await.unwrap();
    let _: ExifIter = parser.parse_exif_async(ms).await.unwrap();
}

#[cfg(feature = "tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn media_parser_parse_track_async() {
    use crate::parser_async::AsyncMediaSource;
    let mut parser = MediaParser::new();
    let ms = AsyncMediaSource::open("testdata/meta.mov").await.unwrap();
    let _: TrackInfo = parser.parse_track_async(ms).await.unwrap();
}
```

Run:
```bash
cargo test --lib --all-features media_parser_parse_exif_async media_parser_parse_track_async 2>&1 | tail -10
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: PASS, total 211.

- [ ] **Step 9.3: Commit**

```bash
git add -A
git commit -m "feat(parser): MediaParser::parse_exif_async / parse_track_async

Per v3 spec §3.4: a single MediaParser type carries both sync and async
parse methods. AsyncMediaParser remains alongside until end of P3."
```

---

## Task 10 — Add `Metadata` enum + top-level convenience functions

Spec: §3.11.

**Files:**
- Modify: `src/lib.rs` — add `Metadata`, `read_exif`, `read_exif_iter`, `read_track`, `read_metadata`, and async variants.

- [ ] **Step 10.1: Define `Metadata`**

In `src/lib.rs`, after the existing re-exports (around line 339 before the `mod` declarations), add:

```rust
/// One-shot result of [`read_metadata`]: either Exif (image) or TrackInfo
/// (video/audio). Closed enum — see spec §8.6 for why there's no `Both`
/// variant.
#[derive(Debug, Clone)]
pub enum Metadata {
    Exif(Exif),
    Track(TrackInfo),
}
```

- [ ] **Step 10.2: Implement the sync convenience functions**

Append to `src/lib.rs` (after re-exports, before `mod` declarations):

```rust
use std::io::BufReader;
use std::path::Path;

/// Read EXIF metadata from a file in a single call. Wraps the `File` in a
/// `BufReader` internally so the hot path (`for path in paths { read_exif(path)? }`)
/// is immune to per-syscall overhead.
///
/// For batch processing, prefer constructing a [`MediaParser`] once and
/// reusing its parse buffer via [`MediaParser::parse_exif`].
pub fn read_exif(path: impl AsRef<Path>) -> Result<Exif> {
    let iter = read_exif_iter(path)?;
    Ok(iter.into())
}

pub fn read_exif_iter(path: impl AsRef<Path>) -> Result<ExifIter> {
    let file = std::fs::File::open(path)?;
    let ms = MediaSource::seekable(BufReader::new(file))?;
    let mut parser = MediaParser::new();
    parser.parse_exif(ms)
}

pub fn read_track(path: impl AsRef<Path>) -> Result<TrackInfo> {
    let file = std::fs::File::open(path)?;
    let ms = MediaSource::seekable(BufReader::new(file))?;
    let mut parser = MediaParser::new();
    parser.parse_track(ms)
}

pub fn read_metadata(path: impl AsRef<Path>) -> Result<Metadata> {
    let file = std::fs::File::open(path)?;
    let ms = MediaSource::seekable(BufReader::new(file))?;
    let mut parser = MediaParser::new();
    match ms.kind() {
        MediaKind::Image => parser.parse_exif(ms).map(|i| Metadata::Exif(i.into())),
        MediaKind::Track => parser.parse_track(ms).map(Metadata::Track),
    }
}
```

- [ ] **Step 10.3: Implement the async convenience functions**

Append (gated):
```rust
#[cfg(feature = "tokio")]
mod tokio_top_level {
    use super::*;
    use tokio::io::BufReader as TokioBufReader;

    pub async fn read_exif_async(path: impl AsRef<std::path::Path>) -> Result<Exif> {
        let iter = read_exif_iter_async(path).await?;
        Ok(iter.into())
    }

    pub async fn read_exif_iter_async(path: impl AsRef<std::path::Path>) -> Result<ExifIter> {
        let file = tokio::fs::File::open(path).await?;
        let ms = parser_async::AsyncMediaSource::seekable(TokioBufReader::new(file)).await?;
        let mut parser = MediaParser::new();
        parser.parse_exif_async(ms).await
    }

    pub async fn read_track_async(path: impl AsRef<std::path::Path>) -> Result<TrackInfo> {
        let file = tokio::fs::File::open(path).await?;
        let ms = parser_async::AsyncMediaSource::seekable(TokioBufReader::new(file)).await?;
        let mut parser = MediaParser::new();
        parser.parse_track_async(ms).await
    }

    pub async fn read_metadata_async(path: impl AsRef<std::path::Path>) -> Result<Metadata> {
        let file = tokio::fs::File::open(path).await?;
        let ms = parser_async::AsyncMediaSource::seekable(TokioBufReader::new(file)).await?;
        let mut parser = MediaParser::new();
        match ms.kind() {
            MediaKind::Image => parser.parse_exif_async(ms).await.map(|i| Metadata::Exif(i.into())),
            MediaKind::Track => parser.parse_track_async(ms).await.map(Metadata::Track),
        }
    }
}

#[cfg(feature = "tokio")]
pub use tokio_top_level::{read_exif_async, read_exif_iter_async, read_track_async, read_metadata_async};
```

Note `BufReader<tokio::fs::File>` implements both `AsyncRead` and `AsyncSeek`, so `seekable` works.

- [ ] **Step 10.4: Tests**

Add to `src/lib.rs` (or a new `tests/v3_top_level.rs` if you prefer integration tests):

```rust
#[cfg(test)]
mod v3_top_level_tests {
    use super::*;

    #[test]
    fn read_exif_jpg() {
        let exif = read_exif("testdata/exif.jpg").unwrap();
        assert!(exif.get(ExifTag::Make).is_some());
    }

    #[test]
    fn read_track_mov() {
        let info = read_track("testdata/meta.mov").unwrap();
        assert!(info.get(TrackInfoTag::Make).is_some());
    }

    #[test]
    fn read_metadata_dispatches_image() {
        match read_metadata("testdata/exif.jpg").unwrap() {
            Metadata::Exif(_) => {}
            Metadata::Track(_) => panic!("expected Exif variant"),
        }
    }

    #[test]
    fn read_metadata_dispatches_track() {
        match read_metadata("testdata/meta.mov").unwrap() {
            Metadata::Track(_) => {}
            Metadata::Exif(_) => panic!("expected Track variant"),
        }
    }

    #[cfg(feature = "tokio")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn read_exif_async_jpg() {
        let exif = read_exif_async("testdata/exif.jpg").await.unwrap();
        assert!(exif.get(ExifTag::Make).is_some());
    }

    #[cfg(feature = "tokio")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn read_track_async_mov() {
        let info = read_track_async("testdata/meta.mov").await.unwrap();
        assert!(info.get(TrackInfoTag::Make).is_some());
    }
}
```

- [ ] **Step 10.5: Re-export the new top-level fns + `Metadata`**

Confirm `src/lib.rs` re-exports include:
```rust
pub use parser::{MediaKind, MediaParser, MediaSource};
// ... and these are visible from the `pub fn` declarations directly above
```
The `pub fn read_exif` etc. are already at module root, no separate re-export needed. Just confirm `pub enum Metadata` is at module root.

- [ ] **Step 10.6: Verify**

Run:
```bash
cargo test --lib --all-features v3_top_level_tests 2>&1 | tail -10
cargo test --lib --all-features 2>&1 | tail -3
```
Expected: PASS, total 217.

- [ ] **Step 10.7: Commit**

```bash
git add -A
git commit -m "feat(lib): top-level read_exif / read_exif_iter / read_track / read_metadata

Per v3 spec §3.11. One-shot helpers wrap File in BufReader internally so
the common hot path (script iterating over paths) doesn't suffer per-read
syscall overhead. Batch users still reach for MediaParser directly.

Adds Metadata enum (closed, Image | Track per spec §8.6)."
```

---

## Task 11 — Migrate internal callers to v3 surface

Spec: §5.1 / §5.8 migration tables (applied to our own tests + examples).

**Files:**
- Modify: `src/parser.rs` test module — `MediaSource::file_path` → `MediaSource::open`, `parser.parse(ms)` → `parser.parse_exif(ms)` / `parser.parse_track(ms)`, `ms.has_exif()` → `ms.kind() == MediaKind::Image`, `ms.has_track()` → `ms.kind() == MediaKind::Track`. (Tests using legacy methods stay until Task 13 — only update the `parse` -> `parse_exif/parse_track` migration here so we exercise the new methods.)

  Actually a more pragmatic plan: leave the v2-style test methods intact (they verify the legacy surface still works); add **new** v3-style tests that exercise the v3 surface end-to-end. Both test sets coexist until Task 13 deletes legacy.

- Modify: `src/parser_async.rs` test module — same dual-coverage approach.
- Modify: `src/cr3.rs:128-129` — update to v3 surface.
- Modify: `src/exif/exif_exif.rs:50,84` — update doc-comment examples to v3 surface.
- Modify: `src/video.rs:131-153` — update doc-comment example to v3 surface.
- Modify: `src/parser.rs:478-503` — update `MediaParser` doc-comment example to v3 surface.
- Modify: `src/parser_async.rs:241-279` — update `AsyncMediaParser` doc-comment example to v3 surface.
- Modify: `examples/rexiftool.rs` — fully migrate to v3:
  - `MediaSource::file_path` → `MediaSource::open`.
  - `ms.has_exif()` → `ms.kind() == MediaKind::Image`.
  - `parser.parse(ms)` → `parser.parse_exif(ms)` / `parser.parse_track(ms)`.
- Modify: `fuzz/fuzz_targets/media_parser.rs` — `parser.parse(ms)` → `parser.parse_exif(ms)` / `parser.parse_track(ms)`.

- [ ] **Step 11.1: Migrate `examples/rexiftool.rs`**

Edit the diff (rough sketch — the executor confirms exact lines):
```rust
// L104
let ms = MediaSource::open(path).inspect_err(handle_parsing_error)?;

// L105-129
let values = match ms.kind() {
    MediaKind::Image => {
        let iter: ExifIter = parser.parse_exif(ms).inspect_err(handle_parsing_error)?;
        iter.into_iter()
            .filter_map(/* unchanged body */)
            .collect::<Vec<_>>()
    }
    MediaKind::Track => {
        let info: TrackInfo = parser.parse_track(ms)?;
        info.into_iter()
            .map(|x| (x.0.to_string(), x.1))
            .collect::<Vec<_>>()
    }
};
```

Add `MediaKind` to the `use nom_exif::{...}` import.

- [ ] **Step 11.2: Migrate `fuzz/fuzz_targets/media_parser.rs`**

```rust
fuzz_target!(|data: &[u8]| {
    let mut parser = MediaParser::new();

    if let Ok(ms) = MediaSource::seekable(Cursor::new(data)) {
        let iter: Result<ExifIter, _> = parser.parse_exif(ms);
        if let Ok(iter) = iter {
            let _ = iter.parse_gps_info();
            let _: Exif = iter.into();
        }
    }
    if let Ok(ms) = MediaSource::seekable(Cursor::new(data)) {
        let _: Result<TrackInfo, _> = parser.parse_track(ms);
    }
    if let Ok(ms) = MediaSource::unseekable(Cursor::new(data)) {
        let iter: Result<ExifIter, _> = parser.parse_exif(ms);
        if let Ok(iter) = iter {
            let _ = iter.parse_gps_info();
            let _: Exif = iter.into();
        }
    }
    if let Ok(ms) = MediaSource::unseekable(Cursor::new(data)) {
        let _: Result<TrackInfo, _> = parser.parse_track(ms);
    }
});
```

- [ ] **Step 11.3: Migrate doc-comment examples**

For `src/cr3.rs:128`, `src/exif/exif_exif.rs:50,84`, `src/video.rs:131`, `src/parser.rs:478-503`, `src/parser_async.rs:252-279`: globally replace `MediaSource::file_path` → `MediaSource::open`, `AsyncMediaSource::file_path` → `AsyncMediaSource::open`, `parser.parse(ms)` → `parser.parse_exif(ms)` (or `parse_track` based on context — disambiguate by what the doc-comment is doing).

Run:
```bash
grep -n 'file_path\|parser\.parse(\|has_exif\|has_track' src/cr3.rs src/exif/exif_exif.rs src/video.rs src/parser.rs src/parser_async.rs
```
After edits, only test-mod lines should remain. Tests still use legacy until Task 13.

- [ ] **Step 11.4: Verify**

Run:
```bash
cargo test --all-features 2>&1 | tail -3
cargo test --doc --all-features 2>&1 | tail -3
cargo build --examples --all-features 2>&1 | tail -3
```
Expected: clean. Doc tests in particular will catch any remaining v2 surface in doc-comments.

- [ ] **Step 11.5: Commit**

```bash
git add -A
git commit -m "refactor: migrate examples, fuzz, and doc-comments to v3 entry surface

Tests in parser.rs / parser_async.rs still call legacy methods; deleted
in Task 13 of P3. Doc-comment examples are user-facing and switch now."
```

---

## Task 12 — Stub `lib.rs` doc-comment for P3 (full rewrite is P6)

Spec / master plan §"Cross-phase rules": "in P3, gate the v2 doc-comment behind `#[cfg(any())]` (i.e. dead-code-comment it) and write a *minimal* placeholder; full rewrite is P6."

**Why this stays even after Task 11:** the doc-comment in `lib.rs` is a multi-page tutorial with v2-style examples (`MediaSource::file_path`, `as_time_components`, `get_gps_info`). Some of those (e.g. `as_time_components`) still exist in v2 form and will be reshaped in P4/P5 — so we cannot meaningfully rewrite the tutorial in P3 without front-running future phases. Stub it.

**Files:**
- Modify: `src/lib.rs:1-326` (replace the entire `//!` doc-comment block).

- [ ] **Step 12.1: Replace the `lib.rs` doc-comment**

Replace lines 1-326 (the entire `//!` block, from `//! \`nom-exif\` is...` through the rexiftool examples and ending at `//!`) with:

```rust
//! `nom-exif` — Exif and track metadata parser for image, video, and audio
//! files.
//!
//! **v3 (in progress):** the API is being reshaped; this top-level docstring
//! is a placeholder until P6 lands the full v3 tutorial. The public symbols
//! you most likely want:
//!
//! - One-shot helpers: [`read_exif`], [`read_exif_iter`], [`read_track`],
//!   [`read_metadata`].
//! - Reusable parser: [`MediaParser`] + [`MediaSource`] + [`MediaKind`].
//! - Async variants under `feature = "tokio"`.
//!
//! See the v3 design document at `docs/V3_API_DESIGN.md` for the full
//! migration story.
```

Roughly 12 lines, no doctests (so nothing breaks during P4/P5).

- [ ] **Step 12.2: Verify `cargo doc` is clean**

Run:
```bash
cargo doc --no-deps --all-features 2>&1 | tail -10
cargo test --doc --all-features 2>&1 | tail -3
```
Expected: clean — no broken intra-doc links, no failing examples.

- [ ] **Step 12.3: Commit**

```bash
git add -A
git commit -m "docs(lib)!: stub crate-level docstring; full v3 tutorial in P6

Per master plan: lib.rs //! examples used v2 API surface that is being
removed in P3-P5. Stub now to keep doc-tests green; full rewrite happens
in P6 after the value/iter API has stabilized."
```

---

## Task 13 — Delete v2 entry surface

Per master plan: deletions are the **last** task of each phase, so the build stays green between earlier tasks.

**Targets:**

1. `src/parser.rs`:
   - `MediaSource::file_path`, `MediaSource::file` (the v2 forwarders added in Task 4).
   - `MediaSource::has_exif`, `MediaSource::has_track`.
   - `MediaSource::tcp_stream` (entire `impl MediaSource<TcpStream>` block).
   - `pub trait ParseOutput<R>` + its two impls.
   - `MediaParser::parse<R, O>` (the generic delegate).
   - `MediaParser::do_parse` if it's no longer needed by anything other than `parse<O>`. (The new `parse_exif` / `parse_track` should inline its body.)
   - `use std::net::TcpStream` import.
2. `src/parser_async.rs`:
   - `AsyncMediaSource::file_path`, `AsyncMediaSource::file`.
   - `AsyncMediaSource::has_exif`, `AsyncMediaSource::has_track`.
   - `pub trait AsyncParseOutput<R>` + its two impls.
   - `pub struct AsyncMediaParser` + all its methods (the file shrinks to: `AsyncMediaSource` + `AsyncSkipBySeekFn` + `AsyncBufParser` trait).
   - `pub use parser_async::AsyncMediaParser` in `src/lib.rs`.
3. `src/parser.rs` test module:
   - The four legacy tests `parse_exif_on_track_returns_exif_not_found` / `parse_track_on_image_returns_track_not_found` (use `_v3` versions added in Task 8).
   - Update remaining tests that still call `MediaSource::file_path` / `parser.parse(ms)` / `ms.has_exif()` to v3 surface (`MediaSource::open`, `parser.parse_exif(ms)`, `ms.kind() == MediaKind::Image`).
4. `src/parser_async.rs` test module: same migration as above for async tests.
5. `src/cr3.rs:128-129` — `MediaSource::file_path` → `MediaSource::open` (was deferred from Task 11 because tests were not in the "doc-comments" group); `parser.parse(ms)` → `parser.parse_exif(ms)`; `ms.has_exif()` → `ms.kind() == MediaKind::Image`.

- [ ] **Step 13.1: Delete legacy methods/types in `src/parser.rs`**

Delete:
- The `impl MediaSource<TcpStream>` block (the whole impl).
- `has_track` / `has_exif` from `impl<R: Read> MediaSource<R>`.
- `file_path` / `file` from `impl MediaSource<File>`.
- `pub trait ParseOutput<R>` and its two `impl` blocks.
- `MediaParser::parse<R, O>` method.
- `do_parse` method (unused after `parse` deletion).
- `use std::net::TcpStream;` import.

Inline `parse_exif` / `parse_track` bodies (no longer delegating to a deleted method):
```rust
pub fn parse_exif<R: Read>(&mut self, mut ms: MediaSource<R>) -> crate::Result<ExifIter> {
    self.reset();
    self.acquire_buf();
    self.buf_mut().append(&mut ms.buf);
    let res: crate::Result<ExifIter> = (|| {
        self.fill_buf(&mut ms.reader, INIT_BUF_SIZE)?;
        if !matches!(ms.mime, crate::file::MediaMime::Image(_)) {
            return Err(crate::Error::ExifNotFound);
        }
        crate::exif::parse_exif_iter(self, ms.mime.unwrap_image(), &mut ms.reader, ms.skip_by_seek)
    })();
    self.reset();
    res
}

pub fn parse_track<R: Read>(&mut self, mut ms: MediaSource<R>) -> crate::Result<TrackInfo> {
    self.reset();
    self.acquire_buf();
    self.buf_mut().append(&mut ms.buf);
    let res: crate::Result<TrackInfo> = (|| {
        self.fill_buf(&mut ms.reader, INIT_BUF_SIZE)?;
        let mime_track = match ms.mime {
            crate::file::MediaMime::Image(_) => return Err(crate::Error::TrackNotFound),
            crate::file::MediaMime::Track(t) => t,
        };
        let skip = ms.skip_by_seek;
        self.load_and_parse(ms.reader.by_ref(), skip, |data, _| {
            crate::video::parse_track_info(data, mime_track)
                .map_err(|e| ParsingErrorState::new(e, None))
        })
    })();
    self.reset();
    res
}
```

- [ ] **Step 13.2: Delete legacy methods/types in `src/parser_async.rs`**

Delete:
- `has_track` / `has_exif` from `impl<R: AsyncRead + Unpin> AsyncMediaSource<R>`.
- `file_path` / `file` from `impl AsyncMediaSource<File>`.
- `pub trait AsyncParseOutput<R>` and its two impls.
- `pub struct AsyncMediaParser` + `Debug`/`Default`/`ShareBuf`/`AsyncBufParser` impls + all methods.
- `pub fn new`, `pub async fn parse`, etc.

`src/parser_async.rs` after deletion contains only:
- `AsyncSkipBySeekFn<R>` type alias.
- `AsyncMediaSource<R>` struct + impls (Debug, build, kind, seekable, unseekable, open, from_file).
- `AsyncBufParser` trait (used by `MediaParser`'s tokio-gated impl in `src/parser.rs`).

- [ ] **Step 13.3: Update `src/lib.rs` re-exports**

```rust
#[cfg(feature = "tokio")]
pub use parser_async::{AsyncMediaParser, AsyncMediaSource};
```
→
```rust
#[cfg(feature = "tokio")]
pub use parser_async::AsyncMediaSource;
```

- [ ] **Step 13.4: Migrate remaining test sites to v3 surface**

Files: `src/parser.rs` (test mod), `src/parser_async.rs` (test mod), `src/cr3.rs` (test mod).

Run:
```bash
grep -n 'file_path\|\.has_exif()\|\.has_track()\|parser\.parse(' src/parser.rs src/parser_async.rs src/cr3.rs
```

For each remaining site, apply the migration:
- `MediaSource::file_path(p)` → `MediaSource::open(p)`.
- `MediaSource::file(f)` → `MediaSource::from_file(f)`.
- `AsyncMediaSource::file_path(p).await` → `AsyncMediaSource::open(p).await`.
- `AsyncMediaSource::file(f).await` → `AsyncMediaSource::from_file(f).await`.
- `ms.has_exif()` (assertion in test setup) → `assert_eq!(ms.kind(), MediaKind::Image);`.
- `ms.has_track()` (assertion) → `assert_eq!(ms.kind(), MediaKind::Track);`.
- `ms.has_exif()` (branch condition) → `ms.kind() == MediaKind::Image`.
- `parser.parse(ms)` (Exif context) → `parser.parse_exif(ms)`.
- `parser.parse(ms)` (Track context) → `parser.parse_track(ms)`.
- `parser.parse(ms).await` (Exif context) → `parser.parse_exif_async(ms).await`.
- `parser.parse(ms).await` (Track context) → `parser.parse_track_async(ms).await`.

The `parse_media` parameterized test in `src/parser.rs` and `src/parser_async.rs` has a generic flow — be careful with the `Exif` / `Track` arms. Same in `parse_track_crash`.

- [ ] **Step 13.5: Verify the deletion was exhaustive**

Run:
```bash
grep -nE 'file_path|tcp_stream|has_exif|has_track|ParseOutput|AsyncParseOutput|AsyncMediaParser|MediaParser::parse[^_]' src tests examples fuzz 2>&1 | grep -v '/\*' | grep -v '^$'
```
Expected: no output, or only `Mime::Image` matches (the false-positive on `has_exif` shouldn't occur because the symbol is gone).

Run the master verification chain:
```bash
cargo build --no-default-features 2>&1 | tail -3
cargo build --all-features 2>&1 | tail -3
cargo build --examples --all-features 2>&1 | tail -3
cargo test --lib --no-default-features 2>&1 | tail -3
cargo test --lib --all-features 2>&1 | tail -3
cargo test --doc --all-features 2>&1 | tail -3
cargo doc --no-deps --all-features 2>&1 | tail -10
```
Expected: all green, no warnings.

- [ ] **Step 13.6: Sanity-check fuzz still builds**

Run:
```bash
( cd fuzz && cargo +nightly check 2>&1 | tail -5 ) || echo "Skipping if nightly fuzz toolchain unavailable"
```
Expected: clean check, or graceful skip if nightly is missing locally.

- [ ] **Step 13.7: Commit**

```bash
git add -A
git commit -m "refactor!(api)!: delete v2 entry surface

Removes per v3 spec §3.3 / §5.1 / §5.8:

- MediaSource: tcp_stream, file_path, file, has_exif, has_track
- AsyncMediaSource: file_path, file, has_exif, has_track
- MediaParser::parse<O> + ParseOutput trait
- AsyncMediaParser (entire struct) + AsyncParseOutput trait

Internal callers (tests, examples, fuzz) migrated to v3 surface in
preceding tasks; this commit only deletes."
```

---

## Task 14 — Final verification and tag

- [ ] **Step 14.1: Full test matrix**

Run all of these in sequence; each must pass:
```bash
cargo build --no-default-features 2>&1 | tail -3
cargo build --features tokio 2>&1 | tail -3
cargo build --features serde 2>&1 | tail -3
cargo build --all-features 2>&1 | tail -3
cargo build --examples --all-features 2>&1 | tail -3
cargo test --lib --no-default-features 2>&1 | tail -3
cargo test --lib --all-features 2>&1 | tail -3
cargo test --doc --all-features 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings 2>&1 | tail -10
cargo doc --no-deps --all-features 2>&1 | tail -10
```

If clippy reports any warnings, fix them in a follow-up commit before tagging.

- [ ] **Step 14.2: Confirm spec coverage**

Use the §4.1 table from `docs/V3_API_DESIGN.md` — verify each P3-owned symbol exists with the documented signature:

```bash
grep -nE '^pub (fn|struct|enum|trait|use|type) ' src/lib.rs
```

Manual checklist (cross out as confirmed):
- [ ] `MediaParser` (struct, sync + async methods)
- [ ] `MediaSource<R>` (struct, no `S`)
- [ ] `AsyncMediaSource<R>` (struct, no `S`, gated)
- [ ] `MediaKind` (enum, closed `Image | Track`)
- [ ] `Metadata` (enum, closed `Exif | Track`)
- [ ] `read_exif`, `read_exif_iter`, `read_track`, `read_metadata` (sync fns)
- [ ] `read_*_async` (gated fns)
- [ ] Cargo features: `tokio`, `serde` (no `async` / `json_dump`)

- [ ] **Step 14.3: Tag the phase**

```bash
git tag v3-p3-done
git log --oneline -1
```

Confirm the tag points at the deletion commit, and report to the user with a one-liner on what changed and what's next (P4 — values).

---

## Risks / open questions

| Risk | Mitigation |
|------|------------|
| **Async fn-pointer ergonomics**: `for<'a> fn(&'a mut R, u64) -> Pin<Box<dyn Future + Send + 'a>>` is verbose and may surface lifetime errors during construction. | Closures with no captures should coerce. If the executor hits HRTB inference issues, fall back to a small enum + match (cost: one extra branch per skip; trivial vs I/O). The user-visible API is identical in either implementation. |
| **`MediaSource<R>` doesn't constrain `R: Read`** at the struct level — users could write `MediaSource::<MyType> { ... }` if they could find the field accessors (they can't; fields are `pub(crate)`), but the type lifts the bound to its constructors only. | This is a feature, not a bug — the v2 design also relied on construction-site bounds. Documented in the rustdoc on `seekable`/`unseekable`. |
| **Doc-tests in `src/parser.rs`'s `MediaParser` doc-comment block** become broken between Task 8 (where `parse_exif` exists) and Task 13 (where `parse` is deleted). | Task 11 rewrites the doc-comment to v3 surface, so by Task 13 the doc-comment is already migrated. |
| **`AsyncMediaParser` deletion** ripples through anything that names it externally. | `pub use parser_async::AsyncMediaParser` is the only external reference; deleting it in Task 13 is a single-line edit. Downstream is already broken by every other change in P3 — `AsyncMediaParser` doesn't add to the migration burden. |
| **Adding `BufReader` inside `read_exif`** changes the buffering model. The parser's own buffer pool already handles large reads, so wrapping in `BufReader` adds a small (usually 8 KiB) header-stage buffer. Net effect on hot path: slightly fewer syscalls during MIME detection (the first 128 bytes), no slowdown after the parser takes over. | Document the rationale in the `read_exif` rustdoc (as already drafted in §3.11). |

## Done definition (P3)

1. Phase tag `v3-p3-done` exists at HEAD.
2. `cargo test --lib --all-features` shows ≥ 217 passing tests.
3. `cargo test --doc --all-features` is clean.
4. `cargo clippy --all-features --all-targets -- -D warnings` is clean.
5. `cargo doc --no-deps --all-features` is clean.
6. The v2 symbols listed in Task 13 are absent from the codebase (`grep` returns empty).
7. Cargo features `async` and `json_dump` are absent from `Cargo.toml`; `tokio` and `serde` exist with the documented `dep:` syntax.

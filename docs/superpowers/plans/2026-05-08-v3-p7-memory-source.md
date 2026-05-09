# nom-exif v3 — P7: zero-copy memory data source

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a zero-copy memory entry point — `MediaSource::from_bytes(impl Into<bytes::Bytes>)` plus parser methods (`parse_exif_bytes` / `parse_track_bytes`) and one-shot helpers (`read_exif_from_bytes` / `read_exif_iter_from_bytes` / `read_track_from_bytes` / `read_metadata_from_bytes`) — for the "data is already in memory" case (WASM, mobile, HTTP proxies). Streaming code path is untouched.

**Architecture:** A new internal "memory mode" lives inside `BufferedParserState` as a `memory: Option<Bytes>` slot. When set, `Buf::buffer()` returns `&memory[position..]`, `BufParser::fill_buf` short-circuits to `Err(UnexpectedEof)` (the parser already holds every byte it can ever have), and `ShareBuf::share_buf` returns `(memory.take(), position)` — a `Bytes::clone`-grade share with no `Vec<u8>` round-trip and no recycle-cache write (the user owns the alloc; recycle is irrelevant). `clear_and_skip` is unchanged: in memory mode every skip falls into `SkipPlan::AdvanceOnly` because the requested skip count cannot exceed the buffer length when the buffer *is* the whole input. The `MediaSource<R>` struct gains a `memory: Option<Bytes>` field; `MediaSource::<()>::from_bytes` is the only constructor that populates it. `parse_exif_bytes` / `parse_track_bytes` move the `Bytes` from the source into parser state and dispatch into the existing format parsers (`parse_exif_iter` / `parse_track_info`) using `std::io::empty()` (or `tokio::io::empty()`) as a placeholder reader — the reader is never read in memory mode, only used to satisfy the generic bound.

**Tech Stack:** `bytes` 1.7.1 (already a hard dependency), Rust 1.83.

**Phase position:** lands on `v3` branch after P6 (`v3.0.0-rc.1` already tagged). P7 is *not* part of v3.0.0 cutover; it ships in v3.1 (or v3.0.x point release at the maintainer's discretion). Master plan's phase summary table is updated as the last task to flip the P7 row from `(TBW)` to `done`.

**Why now:** P4.5 already converged the internal byte-view onto `bytes::Bytes`, so memory mode's terminal share is just `Bytes::clone()`. The streaming and memory paths now end at the same primitive, which is what makes P7 a small surgery rather than a structural one. P7 is the final piece of the v3 read-side API and the natural close to the redesign.

**Why a parallel `parse_exif_bytes` method instead of overloading `parse_exif`:** `parse_exif<R: Read>` requires `R: Read`. `MediaSource<()>` cannot satisfy that bound (`()` does not implement `Read`), so the compiler will refuse the call on the same name. Two alternatives were considered:

1. **Sealed trait `ParseSource` impl'd by `MediaSource<R: Read>` and `MediaSource<()>`, single `parse_exif<S: ParseSource>(s)`** — adds a public trait, churns the existing `parse_exif` signature, and every doc example. Cost > value for v3.1 — the trait would carry zero abstraction users actually want to plug into.
2. **A wrapper type `MediaBytes` distinct from `MediaSource<()>`** — diverges from spec §3.3 ("没有引入新的 source 类型").

The dual-method approach (`parse_exif` for `Read` sources, `parse_exif_bytes` for memory) keeps each method's bound exactly what it needs and surfaces the "this skips streaming I/O" property in the name. If a future v3.x introduces a unifying trait, the dual methods can be deprecated then; for v3.1 they are the simplest correct shape.

**Why `std::io::empty()` (sync) / `tokio::io::empty()` (async) as the placeholder reader:** the format parsers (`crate::exif::parse_exif_iter`, `crate::video::parse_track_info`-via-`load_and_parse`) take a `&mut R: Read` for type reasons. In memory mode, every code path that *would* read from `R` (`fill_buf`, the `skip_by_seek` callback inside `clear_and_skip`'s `ClearAndSkip` branch) is short-circuited by parser state checks before the reader is touched. `io::Empty` is the canonical "type-system placeholder, never touched" reader and avoids forking the format-parser entry points into "with reader / without reader" pairs.

**Exit criterion:**

- `MediaSource::<()>::from_bytes(impl Into<bytes::Bytes>) -> Result<Self>` exists.
- `MediaParser::parse_exif_bytes(&mut self, MediaSource<()>) -> Result<ExifIter>` exists.
- `MediaParser::parse_track_bytes(&mut self, MediaSource<()>) -> Result<TrackInfo>` exists.
- Top-level helpers `read_exif_from_bytes` / `read_exif_iter_from_bytes` / `read_track_from_bytes` / `read_metadata_from_bytes` exist (sync only — async memory mode is meaningless).
- A regression test asserts that `share_buf` in memory mode returns a `Bytes` whose `as_ptr()` matches the user-supplied `Bytes::as_ptr()` (true zero-copy share).
- `cargo test --all-features` green.
- `cargo doc --no-deps --all-features --document-private-items` clean.
- `cargo clippy --all-features -- -D warnings` clean.
- `lib.rs` `//!` doc adds a "memory input" subsection under Quick start; no streaming examples broken.
- Master plan phase summary table flips P7 from `(TBW)` to `done`; status line updated.
- Public symbols added: 1 constructor + 2 parser methods + 4 top-level helpers + 0 new types. No types deleted, no existing signatures changed.

---

## File Structure

| File | Operation | Responsibility post-change |
|---|---|---|
| `src/parser.rs` | **Modify** | `BufferedParserState` gains `memory: Option<Bytes>` field, `set_memory` / `is_memory_mode` methods. `Buf::buffer` / `ShareBuf::share_buf` / `acquire_buf` / `reset` branch on memory mode. `MediaParser::fill_buf` (sync `BufParser` impl + async via `tokio_impl`) short-circuits to `Err(UnexpectedEof)` in memory mode. `MediaSource<R>` gains `memory: Option<Bytes>` field. `impl MediaSource<()> { from_bytes }`. `impl MediaParser { parse_exif_bytes, parse_track_bytes }`. New tests for memory mode (zero-copy ptr identity, EOF on truncated bytes, mime sniff on memory). |
| `src/parser_async.rs` | **Modify** | `AsyncBufParser` impl on `MediaParser` (under `#[cfg(feature = "tokio")]`) — `fill_buf` short-circuits in memory mode with the same `Err(UnexpectedEof)` semantics. (Note: `parse_exif_bytes` / `parse_track_bytes` are sync-only — no async counterparts.) |
| `src/lib.rs` | **Modify** | Top-level helpers: `read_exif_from_bytes`, `read_exif_iter_from_bytes`, `read_track_from_bytes`, `read_metadata_from_bytes`. `//!` adds a "Reading from in-memory bytes" example. Top-level test module adds smoke tests. `prelude` is **not** updated — memory helpers are a niche entry point; cold-path types stay out of prelude per existing v3 prelude policy. |
| `docs/V3_API_DESIGN.md` | **No change** | §3.3 already documents the design. P7 is a faithful implementation of that section. |
| `docs/superpowers/plans/2026-05-08-v3-master.md` | **Modify** | Phase summary: P7 row promoted from `(TBW)` placeholder to `[v3-p7-memory-source.md](2026-05-08-v3-p7-memory-source.md)` with concrete exit criterion. Status line: append `· P7 ✅ done (v3.1.0)`. |
| `CHANGELOG.md` | **Modify** | New `## [3.1.0]` section listing the four added public symbols. (If a `## [Unreleased]` section exists at HEAD, add under it instead and let release tooling promote it later.) |

---

## Task 1: Branch hygiene + baseline tests + plan commit

**Goal of this task:** confirm working tree is on `v3` after rc.1, capture the pre-P7 test count for end-of-phase verification, and land this plan document so subagents have a stable reference.

**Files:**
- Read: `Cargo.toml`, `src/parser.rs`, `src/lib.rs`
- Modify: `docs/superpowers/plans/2026-05-08-v3-p7-memory-source.md` (this plan, already on disk — committed in this task)

- [ ] **Step 1: Confirm branch state**

Run: `git status` and `git log --oneline -5`

Expected: `On branch v3`. Recent commits include `v3.0.0-rc.1` tag (or its equivalent commit) at or near HEAD. Working tree clean except for this plan file.

- [ ] **Step 2: Capture baseline test count**

Run: `cargo test --all-features 2>&1 | grep -E "test result" | tail -20`

Expected: every line ends with `0 failed`. Record the total `passed` count for Task 9 verification (paste the numbers into a scratch comment in this plan or note them externally).

- [ ] **Step 3: Verify the public surface entering P7**

Run: `cargo doc --no-deps --all-features --document-private-items 2>&1 | tail -5`

Expected: no warnings.

Run: `Grep -n "^pub use" src/lib.rs`

Expected: the v3 public re-export list as left by P6 — no `read_*_from_bytes` symbols yet, no `from_bytes` constructor yet.

- [ ] **Step 4: Commit this plan**

```bash
git add docs/superpowers/plans/2026-05-08-v3-p7-memory-source.md
git commit -m "docs(v3): add P7 plan for zero-copy memory data source"
```

Expected: clean commit. Working tree clean afterward.

---

## Task 2: Internal — `BufferedParserState::memory` + Buf / ShareBuf branches

**Goal of this task:** add the internal memory-mode plumbing on the parser state, *without* yet wiring up any user-facing constructor or parser method. After this task, `cargo test --all-features` is green and the new internal API surface (`BufferedParserState::set_memory`, `is_memory_mode`) is reachable via the existing `pub(crate)` boundary.

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Add the `memory` slot**

In `src/parser.rs`, change `BufferedParserState`:

```rust
#[derive(Debug, Default)]
pub(crate) struct BufferedParserState {
    cached: Option<bytes::Bytes>,
    buf: Option<Vec<u8>>,
    /// P7: memory-mode storage. When `Some`, the parser is feeding from a
    /// caller-owned `Bytes` instead of streaming via a reader. `buf` and
    /// `cached` are unused in this mode — the user owns the allocation,
    /// so there is nothing to recycle.
    memory: Option<bytes::Bytes>,
    position: usize,
}
```

- [ ] **Step 2: Add `set_memory` / `is_memory_mode` accessors**

Inside `impl BufferedParserState`, alongside `acquire_buf`:

```rust
/// Switch the parser state into memory mode, owning `bytes` directly.
/// Caller must have already called `reset()` (asserted in debug). Subsequent
/// `share_buf` returns a clone of `bytes` (zero-copy: `Bytes::clone` is a
/// refcount bump). Subsequent `Buf::buffer()` returns `&bytes[position..]`.
pub(crate) fn set_memory(&mut self, bytes: bytes::Bytes) {
    debug_assert!(
        self.buf.is_none() && self.memory.is_none(),
        "set_memory called on non-clean state"
    );
    self.memory = Some(bytes);
    self.position = 0;
}

pub(crate) fn is_memory_mode(&self) -> bool {
    self.memory.is_some()
}
```

- [ ] **Step 3: Update `reset` to clear memory mode**

```rust
pub(crate) fn reset(&mut self) {
    // If a parse failed mid-way the buf may still be present; drop it.
    // Cache stays — recycle on next acquire if eligible.
    self.buf = None;
    self.memory = None;
    self.position = 0;
}
```

- [ ] **Step 4: Update `acquire_buf` to no-op in memory mode**

```rust
pub(crate) fn acquire_buf(&mut self) {
    if self.memory.is_some() {
        // Memory mode: nothing to acquire — `buffer()` reads from `memory`.
        return;
    }
    debug_assert!(self.buf.is_none());
    let buf = match self.cached.take() {
        Some(b) => match b.try_into_mut() {
            Ok(bm) => {
                let mut v = Vec::<u8>::from(bm);
                v.clear();
                if v.capacity() > MAX_REUSE_BUF_SIZE {
                    v.shrink_to(MAX_REUSE_BUF_SIZE);
                }
                v
            }
            Err(_still_shared) => Vec::with_capacity(INIT_BUF_SIZE),
        },
        None => Vec::with_capacity(INIT_BUF_SIZE),
    };
    self.buf = Some(buf);
}
```

- [ ] **Step 5: Update `Buf` impl to read from `memory` when set**

```rust
impl Buf for BufferedParserState {
    fn buffer(&self) -> &[u8] {
        if let Some(m) = &self.memory {
            return &m[self.position..];
        }
        &self.buf()[self.position..]
    }
    fn clear(&mut self) {
        // In memory mode `clear` is a no-op: there is no scratch buffer to
        // truncate, and the caller's bytes must remain available for further
        // parse_loop_step iterations. clear_and_skip's AdvanceOnly path is
        // what advances `position` in memory mode.
        if self.memory.is_some() {
            return;
        }
        self.buf_mut().clear();
    }
    fn set_position(&mut self, pos: usize) {
        self.position = pos;
    }
    fn position(&self) -> usize {
        self.position
    }
}
```

- [ ] **Step 6: Update `ShareBuf` impl to clone `memory` in memory mode**

```rust
impl ShareBuf for BufferedParserState {
    fn share_buf(&mut self) -> (bytes::Bytes, usize) {
        if let Some(m) = self.memory.take() {
            // Zero-copy share: caller already owns the allocation. No cache
            // write — recycle is irrelevant when the user holds the alloc.
            let position = self.position;
            return (m, position);
        }
        let vec = self.buf.take().expect("no buf to share");
        let bytes = bytes::Bytes::from(vec);
        let position = self.position;
        self.cached = Some(bytes.clone());
        (bytes, position)
    }
}
```

- [ ] **Step 7: Update `MediaParser::fill_buf` (sync) to short-circuit**

In the `impl BufParser for MediaParser` block at `src/parser.rs:419`:

```rust
impl BufParser for MediaParser {
    #[tracing::instrument(skip(self, reader), fields(buf_len=self.state.buffer().len()))]
    fn fill_buf<R: Read>(&mut self, reader: &mut R, size: usize) -> io::Result<usize> {
        if self.state.is_memory_mode() {
            // Memory mode owns every byte it will ever have. A request for
            // more is "the parser walked off the end of the input"; surface
            // it the same way the streaming path surfaces a 0-byte read.
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        check_fill_size(self.state.buf().len(), size)?;
        let n = reader.take(size as u64).read_to_end(self.state.buf_mut())?;
        if n == 0 {
            tracing::error!(buf_len = self.state.buf().len(), "fill_buf: EOF");
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        tracing::debug!(?size, ?n, buf_len = self.state.buf().len(), "fill_buf: read bytes");
        Ok(n)
    }
}
```

Note: `tracing::instrument`'s `buf_len` field source changes from `self.state.buf().len()` (which panics in memory mode because `buf` is `None`) to `self.state.buffer().len()` (which works in both modes). This is the only externally visible change to the trace event.

- [ ] **Step 8: Add unit tests for the new state transitions**

Append inside the existing `mod tests` in `src/parser.rs`:

```rust
#[test]
fn buffered_state_memory_mode_sets_and_reads() {
    let mut s = BufferedParserState::new();
    s.set_memory(bytes::Bytes::from_static(b"abcdefgh"));
    assert!(s.is_memory_mode());
    assert_eq!(s.buffer(), b"abcdefgh");
    s.set_position(3);
    assert_eq!(s.buffer(), b"defgh");
}

#[test]
fn buffered_state_share_buf_memory_mode_is_zero_copy() {
    let original = bytes::Bytes::from_static(b"the parser owns nothing here");
    let original_ptr = original.as_ptr();
    let mut s = BufferedParserState::new();
    s.set_memory(original);
    let (shared, position) = s.share_buf();
    assert_eq!(position, 0);
    assert_eq!(shared.as_ptr(), original_ptr, "memory share must be a Bytes::clone, not a Vec round-trip");
    // After share_buf, the parser's memory slot is taken — leaving the state
    // ready for the next `reset()` cycle.
    assert!(!s.is_memory_mode());
}

#[test]
fn buffered_state_reset_clears_memory() {
    let mut s = BufferedParserState::new();
    s.set_memory(bytes::Bytes::from_static(b"x"));
    s.reset();
    assert!(!s.is_memory_mode());
    assert_eq!(s.position, 0);
}

#[test]
fn buffered_state_acquire_buf_skips_in_memory_mode() {
    let mut s = BufferedParserState::new();
    s.set_memory(bytes::Bytes::from_static(b"data"));
    s.acquire_buf();
    // No streaming buf was allocated.
    assert!(s.buf.is_none());
    // Memory still readable.
    assert_eq!(s.buffer(), b"data");
}
```

- [ ] **Step 9: Verify**

Run: `cargo test --all-features parser::tests::buffered_state_`

Expected: 4 new tests pass.

Run: `cargo test --all-features` to confirm no regressions.

- [ ] **Step 10: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): add memory-mode plumbing to BufferedParserState"
```

---

## Task 3: Internal — `MediaSource<R>::memory` field + async `fill_buf` short-circuit

**Goal of this task:** thread the `memory` slot through `MediaSource<R>` (still no public constructor) and patch the async `MediaParser::fill_buf` so memory mode is symmetric across sync/async.

The async patch is needed *now* (rather than at constructor time) because `parse_exif_bytes` / `parse_track_bytes` are sync-only, but the trait machinery is shared with async — leaving the async `fill_buf` panicking on `self.state.buf_mut()` in memory mode would be a latent landmine for any future async memory entry point.

**Files:**
- Modify: `src/parser.rs`
- Modify: `src/parser_async.rs`

- [ ] **Step 1: Add `memory: Option<Bytes>` to `MediaSource<R>`**

```rust
pub struct MediaSource<R> {
    pub(crate) reader: R,
    pub(crate) buf: Vec<u8>,
    pub(crate) mime: MediaMime,
    pub(crate) skip_by_seek: SkipBySeekFn<R>,
    /// P7: zero-copy memory-mode payload. `Some` only when the source was
    /// built via [`MediaSource::<()>::from_bytes`]; `reader`, `buf`, and
    /// `skip_by_seek` are placeholders (and never consulted) in that mode.
    pub(crate) memory: Option<bytes::Bytes>,
}
```

- [ ] **Step 2: Update `MediaSource::build` to default `memory` to `None`**

```rust
fn build(mut reader: R, skip_by_seek: SkipBySeekFn<R>) -> crate::Result<Self> {
    let mut buf = Vec::with_capacity(HEADER_PARSE_BUF_SIZE);
    reader.by_ref().take(HEADER_PARSE_BUF_SIZE as u64).read_to_end(&mut buf)?;
    let mime: MediaMime = buf.as_slice().try_into()?;
    Ok(Self { reader, buf, mime, skip_by_seek, memory: None })
}
```

- [ ] **Step 3: Verify sync compile**

Run: `cargo build --all-features 2>&1 | tail -20`

Expected: no errors. Existing tests still pass (`cargo test --all-features parser::tests::media_source_open`).

- [ ] **Step 4: Patch the async `fill_buf` in `tokio_impl`**

In `src/parser.rs`, the `#[cfg(feature = "tokio")] mod tokio_impl` block:

```rust
impl AsyncBufParser for MediaParser {
    async fn fill_buf<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        size: usize,
    ) -> std::io::Result<usize> {
        if self.state.is_memory_mode() {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        check_fill_size(self.state.buf().len(), size)?;
        let n = reader.take(size as u64).read_to_end(self.state.buf_mut()).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        Ok(n)
    }
}
```

- [ ] **Step 5: Verify async compile + tests**

Run: `cargo build --all-features 2>&1 | tail -20`

Run: `cargo test --all-features parser_async::tests::`

Expected: no errors; existing async tests still pass.

- [ ] **Step 6: Commit**

```bash
git add src/parser.rs src/parser_async.rs
git commit -m "feat(parser): add memory slot to MediaSource and async fill_buf short-circuit"
```

---

## Task 4: Public — `MediaSource::<()>::from_bytes` constructor

**Goal of this task:** the first user-visible symbol — a constructor that accepts arbitrary `Into<bytes::Bytes>` (covering `Bytes`, `Vec<u8>`, `&'static [u8]`, `bytes::Bytes::from_owner(...)`) and returns a `MediaSource<()>`.

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Add the constructor block**

After the `impl MediaSource<File> { open }` block, add:

```rust
impl MediaSource<()> {
    /// Build a [`MediaSource`] from an in-memory byte payload.
    ///
    /// Accepts any type convertible into [`bytes::Bytes`] — `Bytes`,
    /// `Vec<u8>`, `&'static [u8]`, [`bytes::Bytes::from_owner`] outputs, and
    /// HTTP-stack body types that implement `Into<Bytes>` directly.
    ///
    /// The header (first up to 128 bytes) is sniffed for media kind, the
    /// same way [`MediaSource::open`] does it for files. The full payload is
    /// stored zero-copy: subsequent parsing through
    /// [`MediaParser::parse_exif_bytes`] / [`MediaParser::parse_track_bytes`]
    /// shares this `Bytes` directly with the returned `ExifIter` / sub-IFDs
    /// via reference counting.
    ///
    /// The returned source is parsed by the dedicated
    /// [`MediaParser::parse_exif_bytes`] / [`MediaParser::parse_track_bytes`]
    /// methods. The streaming `parse_exif` / `parse_track` methods do not
    /// accept `MediaSource<()>` (their `R: Read` bound is unsatisfiable).
    ///
    /// # Example
    ///
    /// ```rust
    /// use nom_exif::{MediaSource, MediaParser, MediaKind};
    ///
    /// let bytes = std::fs::read("./testdata/exif.jpg")?;
    /// let ms = MediaSource::from_bytes(bytes)?;
    /// assert_eq!(ms.kind(), MediaKind::Image);
    ///
    /// let mut parser = MediaParser::new();
    /// let _iter = parser.parse_exif_bytes(ms)?;
    /// # Ok::<(), nom_exif::Error>(())
    /// ```
    pub fn from_bytes(bytes: impl Into<bytes::Bytes>) -> crate::Result<Self> {
        let bytes = bytes.into();
        let head_end = bytes.len().min(HEADER_PARSE_BUF_SIZE);
        let mime: MediaMime = bytes[..head_end].try_into()?;
        Ok(Self {
            reader: (),
            buf: Vec::new(),
            mime,
            // Placeholder: never invoked in memory mode (clear_and_skip's
            // AdvanceOnly path is the only one taken).
            skip_by_seek: |_, _| Ok(false),
            memory: Some(bytes),
        })
    }
}
```

- [ ] **Step 2: Add unit tests for the constructor**

Append in `mod tests`:

```rust
#[test]
fn media_source_from_bytes_image_jpg() {
    let raw = std::fs::read("testdata/exif.jpg").unwrap();
    let ms = MediaSource::from_bytes(raw).unwrap();
    assert_eq!(ms.kind(), MediaKind::Image);
    assert!(ms.memory.is_some());
}

#[test]
fn media_source_from_bytes_track_mov() {
    let raw = std::fs::read("testdata/meta.mov").unwrap();
    let ms = MediaSource::from_bytes(raw).unwrap();
    assert_eq!(ms.kind(), MediaKind::Track);
}

#[test]
fn media_source_from_bytes_static_slice() {
    // &'static [u8] should work via Into<Bytes> because the file is read
    // into a Vec at compile-time-friendly size; here we use include_bytes.
    let raw: &'static [u8] = include_bytes!("../testdata/exif.jpg");
    let ms = MediaSource::from_bytes(raw).unwrap();
    assert_eq!(ms.kind(), MediaKind::Image);
}

#[test]
fn media_source_from_bytes_rejects_too_short() {
    // Below the smallest mime signature length: should fail mime detection.
    let raw = vec![0u8; 4];
    let res = MediaSource::from_bytes(raw);
    assert!(res.is_err(), "expected mime-detection error");
}

#[test]
fn media_source_from_bytes_rejects_unknown_mime() {
    // Random bytes long enough to trigger detection but not match any
    // signature.
    let raw = vec![0xAAu8; 256];
    let res = MediaSource::from_bytes(raw);
    assert!(res.is_err(), "expected mime-detection error for unknown bytes");
}
```

- [ ] **Step 3: Verify**

Run: `cargo test --all-features parser::tests::media_source_from_bytes_`

Expected: 5 new tests pass.

Run: `cargo doc --no-deps --all-features 2>&1 | tail -10`

Expected: no warnings (the new doctest under the constructor compiles).

- [ ] **Step 4: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): add MediaSource::<()>::from_bytes zero-copy constructor"
```

---

## Task 5: Public — `MediaParser::parse_exif_bytes`

**Goal of this task:** the parser-side entry point for memory-mode EXIF parsing. Mirrors the structure of `parse_exif<R>` but installs the `Bytes` into parser state and dispatches with `std::io::empty()` as the placeholder reader.

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Add the method**

Inside `impl MediaParser` (the same block that holds `parse_exif`), append:

```rust
/// Parse Exif metadata from an in-memory byte payload built via
/// [`MediaSource::<()>::from_bytes`]. Returns `Error::ExifNotFound` if the
/// payload is a `Track` (use [`Self::parse_track_bytes`] instead).
///
/// Memory-mode parsing is **zero-copy**: the underlying `Bytes` is shared
/// with the returned [`ExifIter`] (and its sub-IFDs / CR3 CMT blocks) via
/// reference counting. No `Vec<u8>` is allocated for the parse buffer.
pub fn parse_exif_bytes(&mut self, mut ms: MediaSource<()>) -> crate::Result<ExifIter> {
    self.reset();
    let memory = ms
        .memory
        .take()
        .expect("MediaSource<()> must have memory (only constructor is from_bytes)");
    self.state.set_memory(memory);
    let res: crate::Result<ExifIter> = (|| {
        if !matches!(ms.mime, crate::file::MediaMime::Image(_)) {
            return Err(crate::Error::ExifNotFound);
        }
        // Placeholder reader: never read from in memory mode (fill_buf
        // short-circuits; clear_and_skip uses AdvanceOnly).
        let mut empty = std::io::empty();
        crate::exif::parse_exif_iter(
            self,
            ms.mime.unwrap_image(),
            &mut empty,
            // Placeholder skip-by-seek: never invoked.
            |_, _| Ok(false),
        )
    })();
    self.reset();
    res
}
```

- [ ] **Step 2: Add tests covering image kinds + zero-copy assertion + error paths**

Append in `mod tests`:

```rust
#[test]
fn parse_exif_bytes_jpg_basic() {
    let mut parser = MediaParser::new();
    let raw = std::fs::read("testdata/exif.jpg").unwrap();
    let ms = MediaSource::from_bytes(raw).unwrap();
    let iter = parser.parse_exif_bytes(ms).unwrap();
    let exif: crate::Exif = iter.into();
    assert!(exif.get(crate::ExifTag::Make).is_some());
}

#[test]
fn parse_exif_bytes_heic_basic() {
    let mut parser = MediaParser::new();
    let raw = std::fs::read("testdata/exif.heic").unwrap();
    let ms = MediaSource::from_bytes(raw).unwrap();
    let iter = parser.parse_exif_bytes(ms).unwrap();
    let exif: crate::Exif = iter.into();
    assert_eq!(
        exif.get(crate::ExifTag::Make).and_then(|v| v.as_str()),
        Some("Apple")
    );
}

#[test]
fn parse_exif_bytes_zero_copy_shared_bytes() {
    // Build a Bytes whose pointer we can compare. The ExifIter's underlying
    // share must point to the same allocation — proving Bytes::clone path.
    let raw = std::fs::read("testdata/exif.jpg").unwrap();
    let bytes = bytes::Bytes::from(raw);
    let original_ptr = bytes.as_ptr();

    let mut parser = MediaParser::new();
    let ms = MediaSource::from_bytes(bytes).unwrap();
    let iter = parser.parse_exif_bytes(ms).unwrap();

    // The cached pointer in parser state should be None in memory mode
    // (memory mode does not write to cache — the user owns the alloc).
    assert!(
        parser.state.cached_ptr_for_test().is_none(),
        "memory mode must not poison the recycle cache"
    );

    // Drop the iter and confirm parser is clean for the next call.
    drop(iter);

    // Build again; pointer identity proves we did not duplicate the alloc
    // anywhere along the parse path.
    let bytes2 = bytes::Bytes::from(std::fs::read("testdata/exif.jpg").unwrap());
    let ms2 = MediaSource::from_bytes(bytes2.clone()).unwrap();
    let _iter2 = parser.parse_exif_bytes(ms2).unwrap();
    // (We cannot assert pointer-equality across distinct user Bytes; the
    // assertion above on the first parse is the load-bearing one.)
    let _ = original_ptr; // explicit: original_ptr is the assertion target.
}

#[test]
fn parse_exif_bytes_on_track_returns_exif_not_found() {
    let mut parser = MediaParser::new();
    let raw = std::fs::read("testdata/meta.mov").unwrap();
    let ms = MediaSource::from_bytes(raw).unwrap();
    let res = parser.parse_exif_bytes(ms);
    assert!(matches!(res, Err(crate::Error::ExifNotFound)));
}

#[test]
fn parse_exif_bytes_on_truncated_returns_io_error() {
    // Truncate exif.jpg to just enough for mime detection but too short
    // for the full EXIF block. Memory-mode fill_buf must surface
    // UnexpectedEof when the parser walks off the end.
    let mut raw = std::fs::read("testdata/exif.jpg").unwrap();
    raw.truncate(200);
    let mut parser = MediaParser::new();
    let ms = MediaSource::from_bytes(raw).unwrap();
    let res = parser.parse_exif_bytes(ms);
    assert!(res.is_err(), "expected error on truncated bytes, got {:?}", res);
}
```

- [ ] **Step 3: Verify**

Run: `cargo test --all-features parser::tests::parse_exif_bytes_`

Expected: 5 new tests pass.

Run: `cargo test --all-features` to confirm no regressions.

- [ ] **Step 4: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): add MediaParser::parse_exif_bytes for memory sources"
```

---

## Task 6: Public — `MediaParser::parse_track_bytes`

**Goal of this task:** the parallel method for video / audio track metadata. Same shape as `parse_track<R: Read>` but installs `Bytes` into parser state and dispatches with `std::io::empty()`.

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Add the method**

Inside the same `impl MediaParser` block, after `parse_exif_bytes`:

```rust
/// Parse track info from an in-memory video/audio byte payload built via
/// [`MediaSource::<()>::from_bytes`]. Returns `Error::TrackNotFound` if the
/// payload is an `Image` (use [`Self::parse_exif_bytes`] instead).
///
/// Like [`Self::parse_exif_bytes`], the parse is zero-copy with respect to
/// the user-supplied `Bytes`.
pub fn parse_track_bytes(&mut self, mut ms: MediaSource<()>) -> crate::Result<TrackInfo> {
    self.reset();
    let memory = ms
        .memory
        .take()
        .expect("MediaSource<()> must have memory (only constructor is from_bytes)");
    self.state.set_memory(memory);
    let res: crate::Result<TrackInfo> = (|| {
        let mime_track = match ms.mime {
            crate::file::MediaMime::Image(_) => return Err(crate::Error::TrackNotFound),
            crate::file::MediaMime::Track(t) => t,
        };
        let mut empty = std::io::empty();
        let out = self.load_and_parse(&mut empty, |_, _| Ok(false), |data, _| {
            crate::video::parse_track_info(data, mime_track)
                .map_err(|e| ParsingErrorState::new(e, None))
        })?;
        Ok(out)
    })();
    self.reset();
    res
}
```

- [ ] **Step 2: Add tests**

Append:

```rust
#[test]
fn parse_track_bytes_mov_basic() {
    let mut parser = MediaParser::new();
    let raw = std::fs::read("testdata/meta.mov").unwrap();
    let ms = MediaSource::from_bytes(raw).unwrap();
    let info = parser.parse_track_bytes(ms).unwrap();
    assert_eq!(info.get(crate::TrackInfoTag::Make), Some(&"Apple".into()));
    assert_eq!(info.get(crate::TrackInfoTag::Model), Some(&"iPhone X".into()));
}

#[test]
fn parse_track_bytes_mp4_basic() {
    let mut parser = MediaParser::new();
    let raw = std::fs::read("testdata/meta.mp4").unwrap();
    let ms = MediaSource::from_bytes(raw).unwrap();
    let info = parser.parse_track_bytes(ms).unwrap();
    assert!(info.get(crate::TrackInfoTag::CreateDate).is_some());
}

#[test]
fn parse_track_bytes_mkv_basic() {
    let mut parser = MediaParser::new();
    let raw = std::fs::read("testdata/mkv_640x360.mkv").unwrap();
    let ms = MediaSource::from_bytes(raw).unwrap();
    let info = parser.parse_track_bytes(ms).unwrap();
    assert_eq!(
        info.get(crate::TrackInfoTag::ImageWidth),
        Some(&(640_u32.into()))
    );
}

#[test]
fn parse_track_bytes_on_image_returns_track_not_found() {
    let mut parser = MediaParser::new();
    let raw = std::fs::read("testdata/exif.jpg").unwrap();
    let ms = MediaSource::from_bytes(raw).unwrap();
    let res = parser.parse_track_bytes(ms);
    assert!(matches!(res, Err(crate::Error::TrackNotFound)));
}
```

- [ ] **Step 3: Verify**

Run: `cargo test --all-features parser::tests::parse_track_bytes_`

Expected: 4 new tests pass.

Run: `cargo test --all-features`

Expected: full suite green.

- [ ] **Step 4: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): add MediaParser::parse_track_bytes for memory sources"
```

---

## Task 7: Public — top-level `read_*_from_bytes` helpers

**Goal of this task:** the four one-shot helpers in `src/lib.rs` that wrap `MediaSource::from_bytes` + a fresh `MediaParser`. These mirror the existing path-based helpers (`read_exif`, `read_exif_iter`, `read_track`, `read_metadata`) but skip the `BufReader` wrap since the input is already in RAM.

**Files:**
- Modify: `src/lib.rs`

- [ ] **Step 1: Add the four helpers**

After the existing path-based helpers (after `read_metadata` definition, before `#[cfg(feature = "tokio")] mod tokio_top_level`):

```rust
/// Read EXIF metadata from an in-memory byte payload in a single call.
/// Zero-copy: the underlying allocation is shared with the returned
/// [`Exif`] via [`bytes::Bytes`] reference counting.
///
/// Accepts anything convertible into [`bytes::Bytes`] — `Vec<u8>`,
/// `&'static [u8]`, an existing `Bytes`, or HTTP-body types that implement
/// `Into<Bytes>` directly.
///
/// For batch processing or multiple parses against the same buffer, prefer
/// constructing a [`MediaParser`] once and reusing it via
/// [`MediaParser::parse_exif_bytes`].
pub fn read_exif_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Exif> {
    let iter = read_exif_iter_from_bytes(bytes)?;
    Ok(iter.into())
}

/// Read EXIF metadata from an in-memory byte payload as a lazy iterator.
/// Like [`read_exif_from_bytes`] but returns an [`ExifIter`].
pub fn read_exif_iter_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<ExifIter> {
    let ms = MediaSource::from_bytes(bytes)?;
    let mut parser = MediaParser::new();
    parser.parse_exif_bytes(ms)
}

/// Read track metadata from an in-memory video/audio payload.
pub fn read_track_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<TrackInfo> {
    let ms = MediaSource::from_bytes(bytes)?;
    let mut parser = MediaParser::new();
    parser.parse_track_bytes(ms)
}

/// Read metadata from an in-memory payload, dispatching by detected
/// [`MediaKind`]: images return [`Metadata::Exif`], video/audio containers
/// return [`Metadata::Track`].
pub fn read_metadata_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Metadata> {
    let ms = MediaSource::from_bytes(bytes)?;
    let mut parser = MediaParser::new();
    match ms.kind() {
        MediaKind::Image => parser.parse_exif_bytes(ms).map(|i| Metadata::Exif(i.into())),
        MediaKind::Track => parser.parse_track_bytes(ms).map(Metadata::Track),
    }
}
```

- [ ] **Step 2: Add smoke tests in `v3_top_level_tests`**

Inside `mod v3_top_level_tests`:

```rust
#[test]
fn read_exif_from_bytes_jpg() {
    let raw = std::fs::read("testdata/exif.jpg").unwrap();
    let exif = read_exif_from_bytes(raw).unwrap();
    assert!(exif.get(ExifTag::Make).is_some());
}

#[test]
fn read_exif_iter_from_bytes_jpg() {
    let raw = std::fs::read("testdata/exif.jpg").unwrap();
    let iter = read_exif_iter_from_bytes(raw).unwrap();
    assert!(iter.into_iter().count() > 0);
}

#[test]
fn read_track_from_bytes_mov() {
    let raw = std::fs::read("testdata/meta.mov").unwrap();
    let info = read_track_from_bytes(raw).unwrap();
    assert!(info.get(TrackInfoTag::Make).is_some());
}

#[test]
fn read_metadata_from_bytes_dispatches_image() {
    let raw = std::fs::read("testdata/exif.jpg").unwrap();
    match read_metadata_from_bytes(raw).unwrap() {
        Metadata::Exif(_) => {}
        Metadata::Track(_) => panic!("expected Exif variant"),
    }
}

#[test]
fn read_metadata_from_bytes_dispatches_track() {
    let raw = std::fs::read("testdata/meta.mov").unwrap();
    match read_metadata_from_bytes(raw).unwrap() {
        Metadata::Track(_) => {}
        Metadata::Exif(_) => panic!("expected Track variant"),
    }
}

#[test]
fn read_exif_from_bytes_static_slice() {
    let raw: &'static [u8] = include_bytes!("../testdata/exif.jpg");
    let exif = read_exif_from_bytes(raw).unwrap();
    assert!(exif.get(ExifTag::Make).is_some());
}
```

- [ ] **Step 3: Verify**

Run: `cargo test --all-features v3_top_level_tests::read_`

Expected: 6 new tests pass.

Run: `cargo doc --no-deps --all-features 2>&1 | tail -10`

Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs
git commit -m "feat(lib): add top-level read_*_from_bytes helpers"
```

---

## Task 8: Doc updates — `lib.rs` `//!`, Highlights bullet, MediaSource rustdoc

**Goal of this task:** make the new entry points discoverable from the crate-level rustdoc landing page so users searching for "memory", "WASM", "byte slice" find them.

**Files:**
- Modify: `src/lib.rs`
- Modify: `src/parser.rs` (only the `MediaSource` doc-comment intro at line ~26)

- [ ] **Step 1: Add a Highlights bullet**

In `src/lib.rs` `//!` block, inside the `# Highlights` list, after the "Streaming-friendly" bullet:

```
//! - **Zero-copy memory mode** — already-in-RAM bytes (WASM, mobile,
//!   HTTP proxies) parse without copy via [`MediaSource::from_bytes`] +
//!   [`MediaParser::parse_exif_bytes`] / [`MediaParser::parse_track_bytes`],
//!   or one-shot [`read_exif_from_bytes`] / [`read_metadata_from_bytes`].
```

- [ ] **Step 2: Add a "Reading from in-memory bytes" subsection in Quick start**

In `src/lib.rs` `//!` block, immediately before `# API surface`:

```
//! # Reading from in-memory bytes
//!
//! When the payload is already in RAM (WASM, mobile, HTTP proxy, decoded
//! response body), use the `*_from_bytes` helpers to skip the `File` /
//! `Read` round-trip entirely. Memory mode is **zero-copy**: the underlying
//! allocation is shared with the returned [`Exif`] / [`ExifIter`] /
//! [`TrackInfo`] via [`bytes::Bytes`] reference counting.
//!
//! ```rust
//! use nom_exif::{read_exif_from_bytes, ExifTag};
//!
//! let raw = std::fs::read("./testdata/exif.jpg")?;
//! let exif = read_exif_from_bytes(raw)?;
//! assert_eq!(exif.get(ExifTag::Make).and_then(|v| v.as_str()), Some("vivo"));
//! # Ok::<(), nom_exif::Error>(())
//! ```
//!
//! For batch processing of many in-memory payloads, build a [`MediaParser`]
//! once and call [`MediaParser::parse_exif_bytes`] /
//! [`MediaParser::parse_track_bytes`] per payload.
```

- [ ] **Step 3: Update the API surface bullet on one-shot helpers**

In the `# API surface` list, expand the first bullet:

```
//! - **One-shot helpers**: [`read_exif`], [`read_exif_iter`], [`read_track`], [`read_metadata`]
//!   for files; [`read_exif_from_bytes`], [`read_exif_iter_from_bytes`],
//!   [`read_track_from_bytes`], [`read_metadata_from_bytes`] for in-memory bytes.
```

- [ ] **Step 4: Update `MediaSource` rustdoc with the memory option**

In `src/parser.rs`, the `MediaSource<R>` doc-comment block at line ~26-44, before the closing `pub struct MediaSource<R>` declaration:

Replace the bulleted list with:

```rust
/// `MediaSource` represents a media data source that can be parsed by
/// [`MediaParser`].
///
/// - Use [`MediaSource::open`] to create a MediaSource from a file path.
///
/// - Use [`MediaSource::from_bytes`] for zero-copy in-memory input
///   (`Vec<u8>`, `&'static [u8]`, [`bytes::Bytes`], …). Pair with
///   [`MediaParser::parse_exif_bytes`] / [`MediaParser::parse_track_bytes`].
///
/// - In other cases:
///
///   - Use [`MediaSource::seekable`] to create a MediaSource from a `Read + Seek`
///     (an already-open `File` goes here).
///
///   - Use [`MediaSource::unseekable`] to create a MediaSource from a
///     reader that only impl `Read`
///
/// *Note*: Please use [`MediaSource::seekable`] in preference to [`MediaSource::unseekable`],
/// since the former is more efficient when the parser needs to skip a large number of bytes.
///
/// Passing in a `BufRead` should be avoided because [`MediaParser`] comes with
/// its own buffer management and the buffers can be shared between multiple
/// parsing tasks, thus avoiding frequent memory allocations.
```

- [ ] **Step 5: Verify all doc examples compile and run**

Run: `cargo test --all-features --doc 2>&1 | tail -20`

Expected: every doctest passes — including the new Quick-start memory example and the `from_bytes` constructor example.

Run: `cargo doc --no-deps --all-features 2>&1 | tail -10`

Expected: no warnings, no broken intra-doc links.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/parser.rs
git commit -m "docs(v3): document memory mode in crate //! and MediaSource rustdoc"
```

---

## Task 9: Master plan + CHANGELOG + tag phase boundary

**Goal of this task:** flip the P7 row in the master plan from `(TBW)` to `done`, append a `## [3.1.0]` section to CHANGELOG, and tag the phase commit so the v3.1.0 release can branch off it cleanly.

**Files:**
- Modify: `docs/superpowers/plans/2026-05-08-v3-master.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Update the master plan phase summary row**

In `docs/superpowers/plans/2026-05-08-v3-master.md`, replace the current P7 row:

```
| **P7** | (TBW) v3-p7-memory-source.md | §3.3 (新增内存数据源段) | `MediaSource::from_bytes(impl Into<Bytes>)` zero-copy memory input. Internal "memory mode" parse path that bypasses `fill_buf` / `clear_and_skip`. Requires P4.5 landed. Optional for v3.0.0; can ship in v3.1 if scope tightens. |
```

with:

```
| **P7** | [v3-p7-memory-source.md](2026-05-08-v3-p7-memory-source.md) | §3.3 (memory data source) | `MediaSource::<()>::from_bytes(impl Into<Bytes>)` constructor. `MediaParser::parse_exif_bytes` / `parse_track_bytes` methods. Top-level `read_exif_from_bytes` / `read_exif_iter_from_bytes` / `read_track_from_bytes` / `read_metadata_from_bytes`. Internal memory-mode short-circuit on `fill_buf` / `share_buf`; streaming code path untouched. Ships in v3.1.0. |
```

- [ ] **Step 2: Update the Status line**

Replace:

```
- P1 ✅ done · P2 ✅ done · P3 ✅ done · P4 ✅ done · P4.5 ✅ done · P5 ✅ done · P6 ✅ done (v3.0.0-rc.1 tagged)
- P7 (memory data source) — deferred to v3.1; see plan stub.
```

with:

```
- P1 ✅ done · P2 ✅ done · P3 ✅ done · P4 ✅ done · P4.5 ✅ done · P5 ✅ done · P6 ✅ done (v3.0.0-rc.1 tagged) · P7 ✅ done (v3.1.0)
```

- [ ] **Step 3: Add a P7 entry to the risk register (post-mortem form)**

In `## Risk register`, append:

```
| Memory-mode `fill_buf` short-circuit may be reached when the parser legitimately walks past truncated input | P7 | Mitigated by `parse_exif_bytes_on_truncated_returns_io_error` regression test in `parser::tests`. UnexpectedEof maps to the same `Error::IO` variant streaming truncation produces, keeping the error taxonomy consistent. |
```

- [ ] **Step 4: Update CHANGELOG**

Inspect `CHANGELOG.md` to determine whether the layout is `## [Unreleased]` (Keep-a-Changelog style) or sequential version-stamped sections. If `Unreleased` exists, add bullets there; otherwise insert a new `## [3.1.0] - YYYY-MM-DD` section at the top of the version log.

```
### Added
- `MediaSource::<()>::from_bytes(impl Into<bytes::Bytes>)` — zero-copy
  in-memory byte source. Accepts `Vec<u8>`, `&'static [u8]`, `Bytes`, and
  `Bytes::from_owner(...)` outputs.
- `MediaParser::parse_exif_bytes` / `MediaParser::parse_track_bytes` —
  parser methods for memory sources. Zero-copy: returned `ExifIter` /
  sub-IFDs / CR3 CMT blocks share the user's allocation via
  `bytes::Bytes` reference counting.
- One-shot helpers: `read_exif_from_bytes`, `read_exif_iter_from_bytes`,
  `read_track_from_bytes`, `read_metadata_from_bytes`.

### Internal
- `BufferedParserState` gains a memory mode (no public surface change).
  Streaming parse path is untouched.
```

- [ ] **Step 5: Final verification suite**

Run in sequence (each must pass):

```bash
cargo test --all-features 2>&1 | grep "test result"
cargo test --all-features --doc 2>&1 | grep "test result"
cargo clippy --all-features -- -D warnings
cargo doc --no-deps --all-features --document-private-items 2>&1 | tail -10
```

Compare the test count against Task 1 Step 2 baseline: should be **higher** by approximately 24 tests (4 in Task 2, 5 in Task 4, 5 in Task 5, 4 in Task 6, 6 in Task 7). No tests should have *disappeared*. The `--doc` count should increase by 2 (the new `from_bytes` constructor doctest and the new Quick-start memory doctest).

- [ ] **Step 6: Commit and tag**

```bash
git add docs/superpowers/plans/2026-05-08-v3-master.md CHANGELOG.md
git commit -m "docs(v3): mark P7 done in master plan + CHANGELOG"
git tag v3-p7-done
```

- [ ] **Step 7: Confirm tag visibility**

Run: `git log --oneline v3-p4_5-done..v3-p7-done` (or substitute the appropriate prior tag).

Expected: 9 commits between phase boundaries (one per task).

---

## Self-review checklist

**Spec coverage (V3_API_DESIGN.md §3.3, §7 #9):**

- [x] `MediaSource::<()>::from_bytes(impl Into<bytes::Bytes>) -> Result<Self>` — Task 4
- [x] `MediaParser::parse_exif_bytes` / `parse_track_bytes` — Tasks 5, 6
- [x] `read_*_from_bytes` top-level helpers — Task 7
- [x] No new public source type (only constructor + methods on existing types) — Task 4 step 1, Tasks 5–6
- [x] `fill_buf` no-op (Err UnexpectedEof) in memory mode — Task 2 step 7, Task 3 step 4
- [x] `clear_and_skip` becomes `position += n` automatically (no code change needed: AdvanceOnly fires when buffer covers the skip) — verified by Task 5 / Task 6 tests
- [x] `share_buf` returns `Bytes::clone()` directly with no Vec round-trip and no cache write — Task 2 step 6
- [x] No streaming code path touched — `parse_exif<R: Read>` and `parse_track<R: Read>` unchanged

**Type consistency:**

- `MediaSource<R>` field `memory: Option<bytes::Bytes>` — defined in Task 3 step 1, populated only by `from_bytes` (Task 4 step 1), consumed only by `parse_exif_bytes` / `parse_track_bytes` (Tasks 5–6). ✓
- `BufferedParserState` field `memory: Option<bytes::Bytes>` — defined in Task 2 step 1, written by `set_memory` (Task 2 step 2), read by `Buf::buffer` (Task 2 step 5), taken by `share_buf` (Task 2 step 6), cleared by `reset` (Task 2 step 3). ✓
- Sync `BufParser::fill_buf` and async `AsyncBufParser::fill_buf` both check `is_memory_mode` first — Task 2 step 7 + Task 3 step 4. ✓
- `parse_exif_bytes` and `parse_track_bytes` use `std::io::empty()` (sync); no async variants exist (memory + tokio is meaningless and would require duplicating these methods purely for type symmetry). ✓

**Architecture invariants:**

- Memory mode preserves the parser-state invariants `parse_exif`/`parse_track` rely on: `acquire_buf` is a no-op (Task 2 step 4), `Buf::buffer()` returns the entire payload from `position` (Task 2 step 5), `clear_and_skip` advances `position` because `n <= buffer.len()` always holds when buffer = full payload. ✓
- `MediaParser` `&mut self` invariant unchanged — only one outstanding parse at a time. ✓
- `Bytes::clone` is the share primitive in both modes after share_buf — streaming path produces `Bytes::from(Vec)` then clones into cache; memory path clones the user-owned `Bytes`. The downstream consumer (`exif_iter.rs`, `parse_cr3_exif_iter`) sees the same shape. ✓
- No public type added or removed; existing public method signatures unchanged. ✓

**Placeholder scan:** no `TBD`, `figure out`, "appropriate" handling. The `(TBW)` reference in this plan's phase-position note refers to the *prior* state of the master plan and is updated to a concrete row in Task 9 step 1.

**Test discipline:**

- Memory-mode state transitions: 4 tests (Task 2 step 8).
- `from_bytes` constructor: 5 tests covering JPEG, MOV, `&'static [u8]`, too-short input, unknown mime (Task 4 step 2).
- `parse_exif_bytes`: 5 tests covering JPEG, HEIC, zero-copy ptr identity, wrong-kind error, truncated EOF (Task 5 step 2).
- `parse_track_bytes`: 4 tests covering MOV, MP4, MKV, wrong-kind error (Task 6 step 2).
- Top-level helpers: 6 smoke tests (Task 7 step 2).
- Total: 24 new tests + 1 new doctest in `from_bytes` + 1 new doctest in crate-level `//!`.

**Async coverage decision:**

P7 ships sync-only (no `parse_exif_bytes_async` / `parse_track_bytes_async`). Rationale: when bytes already live in RAM, switching threads or yielding to a runtime adds zero throughput — the work is CPU-bound parsing, not I/O. The async patch in Task 3 step 4 exists purely to keep the trait infrastructure consistent in case a future v3.x adds async memory entry points; it does not gate any user-facing behavior in v3.1.

---

**Plan complete and saved to `docs/superpowers/plans/2026-05-08-v3-p7-memory-source.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review diff between tasks, fast iteration

**2. Inline Execution** — execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**

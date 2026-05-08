# v3 Phase 2 — Internal sync/async unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the algorithmic duplication between `BufParser` (sync, in `parser.rs`) and `AsyncBufParser` (async, in `parser_async.rs`) without changing the public API. After P2, the parse-loop control flow lives in exactly one place; the only thing that's duplicated is the I/O dispatch (which is unavoidable in stable Rust without `maybe_async` macros — and the master plan has rejected that approach).

**Architecture:** Extract the buffer-management state (`Buffers`, `Vec<u8>`, `position`) into a shared `BufferedParserState` struct that both `MediaParser` and `AsyncMediaParser` compose. Extract the parse-loop body into a pure state-machine helper `parse_loop_step(buffer, offset, state, parse_fn) -> LoopAction` whose return value tells the (sync or async) caller what I/O to perform next. Same approach for `clear_and_skip`'s skip-decision logic. The result: ~150 lines of duplicated trait-method bodies in `parser_async.rs` shrink to thin (~15-line) drivers around shared helpers.

**Tech Stack:** No new dependencies. Reuses existing `tokio` (feature-gated), `tracing`, `bytes`. Async trait methods (stable since Rust 1.75) keep the `pub(crate)` async API shape, just with shared internals.

**Spec sections covered:** §3.4 (the sync/async story — the *internal* half; the *public* half is P3) + §6.1 (the "去重 sync/async 解析逻辑" line item under "Internal architecture impact").

**Out of scope:**
- Public API changes (P3 will rename the feature `async → tokio`, drop `MediaSource`'s `S` type parameter, merge `AsyncMediaParser` into `MediaParser`'s methods).
- `MediaSource` / `AsyncMediaSource` consolidation — P3.
- `ParseOutput` / `AsyncParseOutput` trait merge — P3.
- Removal of `Skip` / `AsyncSkip` traits (kept; they're invoked by the shared helpers).
- Any changes to `parse_exif_iter` / `parse_exif_iter_async` (already thin wrappers over `parser.load_and_parse(...)`; nothing to extract there).

---

## File map

- **Modify:**
  - `src/parser.rs` — replace `MediaParser`'s state fields with a `BufferedParserState`. Trim `BufParser::load_and_parse_with_offset` body to call the shared `parse_loop_step` helper. Trim `BufParser::clear_and_skip` body to call shared `clear_and_skip_decide` helper. Trim `BufParser::fill_buf` body to share size-clamp logic.
  - `src/parser_async.rs` — same trimming for `AsyncMediaParser`. After P2 this file is ~250 lines instead of ~565 (45% reduction).
  - `src/buffer.rs` — pre-existing `Buffers` type stays. May add `BufferedParserState` here, OR put it in `src/parser.rs` next to the trait definitions. Plan picks `parser.rs` to keep all I/O-coordination types together.
  - `src/error.rs` — delete the commented-out `From<nom::Err<...>> for ParsingErrorState` block (lines ~191–203, flagged by the P1 phase reviewer).
  - `src/values.rs` — delete the commented-out `parse_time_with_local_tz` block (~lines 415–422, P1-reviewer flagged).
  - `src/exif/exif_iter.rs` — delete the commented-out `make_err` block (~lines 361–367, P1-reviewer flagged).

- **Create:** none. (All new types go into existing files.)

---

## Task 0 — Pre-flight

- [ ] **Step 0.1: Confirm we're at `v3-p1-done`**

```bash
git describe --tags --exact-match HEAD 2>/dev/null || git log --oneline HEAD..v3-p1-done
```

Expected: `v3-p1-done` (or empty if HEAD == v3-p1-done). If the user has done other work since P1, abort and ask whether to rebase.

- [ ] **Step 0.2: Baseline build green**

```bash
cargo build --all-features --examples 2>&1 | tail -3
cargo test --lib --all-features 2>&1 | tail -3
```

Expected: 201 lib tests pass.

- [ ] **Step 0.3: Snapshot the duplication baseline**

```bash
wc -l src/parser.rs src/parser_async.rs
```

Expected (approximate): 681 + 565 = 1246 lines. Note this; the P2 exit criterion compares against it.

---

## Task 1 — Extract `BufferedParserState` struct

Both `MediaParser` and `AsyncMediaParser` carry the same three fields: `bb: Buffers`, `buf: Option<Vec<u8>>`, `position: usize`. Both implement `Buf` and `ShareBuf` with byte-identical bodies. Extract.

**Files:**
- Modify: `src/parser.rs` (add `BufferedParserState`, refactor `MediaParser` to compose it, move `Buf` and `ShareBuf` impls)
- Modify: `src/parser_async.rs` (refactor `AsyncMediaParser` to compose it, delete duplicate `Buf` and `ShareBuf` impls)

- [ ] **Step 1.1: Add `BufferedParserState` in `parser.rs`**

After the `pub(crate) trait Buf { ... }` block (~line 143), add:

```rust
/// Buffer-management state shared between `MediaParser` and `AsyncMediaParser`.
///
/// Holds the buffer pool, the currently-acquired buffer, and the read position
/// within it. The `Buf` and `ShareBuf` impls live on this type so both parsers
/// inherit them by composition.
#[derive(Debug, Default)]
pub(crate) struct BufferedParserState {
    bb: Buffers,
    buf: Option<Vec<u8>>,
    position: usize,
}

impl BufferedParserState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn reset(&mut self) {
        if let Some(buf) = self.buf.take() {
            self.bb.release(buf);
        }
        self.position = 0;
    }

    pub(crate) fn acquire_buf(&mut self) {
        debug_assert!(self.buf.is_none());
        self.buf = Some(self.bb.acquire());
    }

    pub(crate) fn buf(&self) -> &Vec<u8> {
        self.buf.as_ref().expect("no buf here")
    }

    pub(crate) fn buf_mut(&mut self) -> &mut Vec<u8> {
        self.buf.as_mut().expect("no buf here")
    }
}

impl Buf for BufferedParserState {
    fn buffer(&self) -> &[u8] {
        &self.buf()[self.position..]
    }
    fn clear(&mut self) {
        self.buf_mut().clear();
    }
    fn set_position(&mut self, pos: usize) {
        self.position = pos;
    }
    fn position(&self) -> usize {
        self.position
    }
}

impl ShareBuf for BufferedParserState {
    fn share_buf(&mut self, mut range: Range<usize>) -> PartialVec {
        let buf = self.buf.take().expect("no buf to share");
        let vec = self.bb.release_to_share(buf);
        range.start += self.position;
        range.end += self.position;
        PartialVec::new(vec, range)
    }
}
```

- [ ] **Step 1.2: Refactor `MediaParser` to compose**

Replace the `MediaParser` struct definition and impls in `src/parser.rs`:

```rust
pub struct MediaParser {
    state: BufferedParserState,
}

impl Debug for MediaParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaParser")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Default for MediaParser {
    fn default() -> Self {
        Self { state: BufferedParserState::new() }
    }
}

impl Buf for MediaParser {
    fn buffer(&self) -> &[u8] { self.state.buffer() }
    fn clear(&mut self) { self.state.clear() }
    fn set_position(&mut self, pos: usize) { self.state.set_position(pos) }
    fn position(&self) -> usize { self.state.position() }
}

impl ShareBuf for MediaParser {
    fn share_buf(&mut self, range: Range<usize>) -> PartialVec {
        self.state.share_buf(range)
    }
}
```

The four-line forwarding `Buf` impl is unfortunate but unavoidable: `Buf` is part of the `BufParser: Buf + Debug` super-trait bound, and `MediaParser` (not `BufferedParserState`) is the type that implements `BufParser`. Same shape for `AsyncMediaParser` in step 1.3.

Update the inherent methods on `MediaParser`:

```rust
impl MediaParser {
    pub fn new() -> Self { Self::default() }

    // ... existing parse() method body, but replace every
    //   `self.buf.take()` → `self.state.buf.take()`         (won't compile — buf is private; use the helpers)
    //   `self.bb.release(...)` → `self.state.bb.release(...)`  (same)
    // ... use `self.state.reset()`, `self.state.acquire_buf()`, `self.state.buf()`, `self.state.buf_mut()` instead.

    fn reset(&mut self) { self.state.reset() }
    fn acquire_buf(&mut self) { self.state.acquire_buf() }
    fn buf(&self) -> &Vec<u8> { self.state.buf() }
    fn buf_mut(&mut self) -> &mut Vec<u8> { self.state.buf_mut() }

    pub fn parse<R, S, O>(...) -> ... { /* unchanged body */ }
    fn do_parse<R, S, O>(...) -> ... { /* unchanged body */ }
}
```

The `pub(crate) fn buf(&self) -> &Vec<u8>` in the original is referenced by `BufParser::fill_buf`. Keep it as a private helper that forwards to `self.state.buf()`.

- [ ] **Step 1.3: Refactor `AsyncMediaParser` symmetrically**

In `src/parser_async.rs`, replace `AsyncMediaParser`'s struct + impls:

```rust
pub struct AsyncMediaParser {
    state: BufferedParserState,
}

// Same Debug, Default, Buf, ShareBuf impls as MediaParser (forwarding)
```

Use `pub(crate) use crate::parser::BufferedParserState;` at the top of the file.

Delete the duplicate `Buf for AsyncMediaParser` and `ShareBuf for AsyncMediaParser` impls — they now live on `BufferedParserState`, accessed via the forwarding impls above.

- [ ] **Step 1.4: Build and test**

```bash
cargo build --all-features 2>&1 | tail -3
cargo test --lib --all-features 2>&1 | tail -3
```

Expected: 201 tests pass.

- [ ] **Step 1.5: Commit**

```bash
git add src/parser.rs src/parser_async.rs
git commit -m "refactor(parser): extract BufferedParserState shared by sync/async"
```

---

## Task 2 — Extract `parse_loop_step` state machine

Both `BufParser::load_and_parse_with_offset` and `AsyncBufParser::load_and_parse_with_offset` have a `loop { match parse(...) { Ok => return; Err => match err { ... } } }` whose body differs only in `.await`. Extract the loop body into a pure helper.

**Files:**
- Modify: `src/parser.rs` (add helper, simplify `BufParser::load_and_parse_with_offset` body)
- Modify: `src/parser_async.rs` (simplify `AsyncBufParser::load_and_parse_with_offset` body)

- [ ] **Step 2.1: Add `LoopAction` enum and `parse_loop_step` helper in `parser.rs`**

Below the `BufferedParserState` block:

```rust
/// What the (sync or async) driver of `parse_loop_step` should do next.
pub(crate) enum LoopAction<O> {
    /// Parse succeeded; return this value to the caller.
    Done(O),
    /// Need more bytes — call `fill_buf(reader, n)` then re-step.
    NeedFill(usize),
    /// Need to skip bytes — call `clear_and_skip(reader, n)` then re-step.
    Skip(usize),
    /// Parse failed permanently. Driver returns `Err(ParsedError::Failed(s))`.
    Failed(String),
}

/// Drives one iteration of the parse-loop algorithm. Pure (no I/O).
///
/// `parsing_state` is `&mut Option<ParsingState>` so the caller threads state
/// across iterations: a `parse` closure may consume the previous state and
/// emit a new one, both via `ParsingErrorState`.
pub(crate) fn parse_loop_step<O>(
    buffer: &[u8],
    offset: usize,
    parsing_state: &mut Option<ParsingState>,
    parse: &mut dyn FnMut(&[u8], usize, Option<ParsingState>) -> Result<O, ParsingErrorState>,
) -> LoopAction<O> {
    match parse(buffer, offset, parsing_state.take()) {
        Ok(o) => LoopAction::Done(o),
        Err(es) => {
            *parsing_state = es.state;
            match es.err {
                ParsingError::Need(n) => LoopAction::NeedFill(n),
                ParsingError::ClearAndSkip(n) => LoopAction::Skip(n),
                ParsingError::Failed(s) => LoopAction::Failed(s),
            }
        }
    }
}
```

The `&mut dyn FnMut` parameter is the wrinkle: sync uses `FnMut`, async uses `Fn` (because the closure may be re-invoked across `.await` points). We accept `FnMut` and let the async caller pass an `Fn` (an `Fn` is automatically a `FnMut` — it just doesn't require `&mut`). This eliminates the v2 inconsistency where the two trait methods diverged on closure bound.

- [ ] **Step 2.2: Simplify `BufParser::load_and_parse_with_offset`**

In `src/parser.rs`, the body becomes:

```rust
fn load_and_parse_with_offset<R: Read, S: Skip<R>, P, O>(
    &mut self,
    reader: &mut R,
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
                self.clear_and_skip::<R, S>(reader, n)?;
            }
            LoopAction::Failed(s) => return Err(ParsedError::Failed(s)),
        }
    }
}
```

- [ ] **Step 2.3: Simplify `AsyncBufParser::load_and_parse_with_offset` symmetrically**

In `src/parser_async.rs` add `use crate::parser::{parse_loop_step, LoopAction};` and rewrite the body to mirror Step 2.2 with `.await` on `fill_buf` and `clear_and_skip`:

```rust
async fn load_and_parse_with_offset<R: AsyncRead + Unpin, S: AsyncSkip<R>, P, O>(
    &mut self,
    reader: &mut R,
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
    let mut parse = parse; // bind so it can be passed as &mut dyn FnMut
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
                self.clear_and_skip::<R, S>(reader, n).await?;
            }
            LoopAction::Failed(s) => return Err(ParsedError::Failed(s)),
        }
    }
}
```

- [ ] **Step 2.4: Build and test**

```bash
cargo build --all-features 2>&1 | tail -3
cargo test --lib --all-features 2>&1 | tail -10
```

Expected: 201 tests pass. If a test fails because the closure-bound change broke an inference, the symptom will be a borrow-checker error at a `parser.load_and_parse(...)` call site. Look at `src/exif.rs:40-42` and `src/parser_async.rs:233-235` — both pass closures of the right shape; should not require changes.

- [ ] **Step 2.5: Commit**

```bash
git add src/parser.rs src/parser_async.rs
git commit -m "refactor(parser): share parse loop algorithm via LoopAction state machine"
```

---

## Task 3 — Extract `clear_and_skip_decide` helper

`BufParser::clear_and_skip` and `AsyncBufParser::clear_and_skip` differ only in `.await` on `fill_buf` and `S::skip_by_seek`. Extract the **decision** (does the requested skip fit in the existing buffer? do we try seek? do we read-and-discard?) into a pure helper, leaving I/O dispatch in the trait method.

**Files:**
- Modify: `src/parser.rs`
- Modify: `src/parser_async.rs`

- [ ] **Step 3.1: Add `SkipPlan` enum and `clear_and_skip_decide` helper in `parser.rs`**

```rust
/// What `clear_and_skip` should do, given the current buffer state and
/// the requested skip count.
pub(crate) enum SkipPlan {
    /// Skip is fully within the current buffer; just advance position.
    AdvanceOnly,
    /// Buffer must be cleared and `extra` bytes skipped from the reader.
    ClearAndSkip { extra: usize },
}

pub(crate) fn clear_and_skip_decide(buffer_len: usize, n: usize) -> SkipPlan {
    if n <= buffer_len {
        SkipPlan::AdvanceOnly
    } else {
        SkipPlan::ClearAndSkip { extra: n - buffer_len }
    }
}
```

- [ ] **Step 3.2: Simplify `BufParser::clear_and_skip`**

```rust
#[tracing::instrument(skip(reader))]
fn clear_and_skip<R: Read, S: Skip<R>>(
    &mut self,
    reader: &mut R,
    n: usize,
) -> Result<(), ParsedError> {
    match clear_and_skip_decide(self.buffer().len(), n) {
        SkipPlan::AdvanceOnly => {
            self.set_position(n);
            return Ok(());
        }
        SkipPlan::ClearAndSkip { extra: skip_n } => {
            self.clear();
            let done = S::skip_by_seek(
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
```

This is shorter than the original by one `if` (the early return is now a match arm). Doesn't fully unify the read-and-discard loop with the async version — that loop has identical structure but `.await`s. Leave the loop duplicated; extracting it would create a yield-point primitive that's awkward to express cleanly.

- [ ] **Step 3.3: Simplify `AsyncBufParser::clear_and_skip` symmetrically**

Mirror Step 3.2 with `.await` on `fill_buf` and `S::skip_by_seek(...).await`.

- [ ] **Step 3.4: Build and test**

```bash
cargo build --all-features 2>&1 | tail -3
cargo test --lib --all-features 2>&1 | tail -3
```

Expected: 201 tests pass.

- [ ] **Step 3.5: Commit**

```bash
git add src/parser.rs src/parser_async.rs
git commit -m "refactor(parser): share clear_and_skip decision via SkipPlan helper"
```

---

## Task 4 — Extract `fill_buf` size-clamp logic

`BufParser::fill_buf` and `AsyncBufParser::fill_buf` differ in:
1. `.await` on `read_to_end`
2. The sync version checks `size.saturating_add(self.buf().len()) > MAX_ALLOC_SIZE`; the async version checks `size > MAX_ALLOC_SIZE` only (a bug — async should also account for the existing buffer length). Spec doesn't mention this; we'll **fix the async bug to match sync**.
3. Both check `n == 0` and return `UnexpectedEof`.

**Files:**
- Modify: `src/parser.rs`
- Modify: `src/parser_async.rs`

- [ ] **Step 4.1: Write a test for the async size-clamp bug**

In `src/parser_async.rs` `tests` mod:

```rust
#[tokio::test(flavor = "current_thread")]
async fn fill_buf_rejects_oversize_when_combined_with_existing() {
    // Given an AsyncMediaParser whose buffer is already 1 GiB - 1024 bytes,
    // requesting 2 KiB more must be rejected (would exceed MAX_ALLOC_SIZE),
    // not silently allowed.
    use tokio::io::repeat;
    let mut parser = AsyncMediaParser::new();
    parser.acquire_buf();
    parser.buf_mut().resize(MAX_ALLOC_SIZE - 1024, 0);
    let mut r = repeat(0);
    let res = parser.fill_buf(&mut r, 2 * 1024).await;
    assert!(matches!(res, Err(ref e) if e.kind() == std::io::ErrorKind::Unsupported),
            "expected Unsupported, got {res:?}");
}
```

Note: this test allocates a near-1GiB Vec, which is slow. If it times out under `cargo test`, mark it `#[ignore]` and run manually. Acceptable since the goal is to prove the fix exists, not to run on every CI build.

Run:

```bash
cargo test --lib --all-features fill_buf_rejects_oversize 2>&1 | tail -10
```

Expected: FAIL — current async code only checks `size > MAX_ALLOC_SIZE`, not the combined size. Bug confirmed.

- [ ] **Step 4.2: Add shared helper in `parser.rs`**

```rust
pub(crate) fn check_fill_size(existing_len: usize, requested: usize) -> io::Result<()> {
    if requested.saturating_add(existing_len) > MAX_ALLOC_SIZE {
        tracing::error!(?requested, "the requested buffer size is too big");
        return Err(io::ErrorKind::Unsupported.into());
    }
    Ok(())
}
```

- [ ] **Step 4.3: Update both `fill_buf` impls to use the helper**

`BufParser::fill_buf` body becomes:

```rust
fn fill_buf<R: Read>(&mut self, reader: &mut R, size: usize) -> io::Result<usize> {
    check_fill_size(self.buf().len(), size)?;
    let n = reader.take(size as u64).read_to_end(self.buf_mut())?;
    if n == 0 {
        tracing::error!(buf_len = self.buf().len(), "fill_buf: EOF");
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    tracing::debug!(?size, ?n, buf_len = self.buf().len(), "fill_buf: read bytes");
    Ok(n)
}
```

`AsyncBufParser::fill_buf` body becomes the same with `.await` on `read_to_end`. The async version also gains the buffer-length-aware clamp (the bug fix).

- [ ] **Step 4.4: Confirm test now passes**

```bash
cargo test --lib --all-features fill_buf_rejects_oversize 2>&1 | tail -3
```

Expected: PASS.

- [ ] **Step 4.5: Full test sweep**

```bash
cargo test --lib --all-features 2>&1 | tail -3
```

Expected: 202 tests pass (+1 from new test).

- [ ] **Step 4.6: Commit**

```bash
git add src/parser.rs src/parser_async.rs
git commit -m "refactor(parser): share fill_buf size-clamp; fix async size-check bug"
```

---

## Task 5 — Cleanup dead-code comments (P1 reviewer follow-ups)

The P1 phase reviewer flagged three commented-out blocks that should be deleted opportunistically. Do them here so they're gone before P3 starts a fresh round of edits in those files.

**Files:**
- Modify: `src/error.rs`
- Modify: `src/values.rs`
- Modify: `src/exif/exif_iter.rs`

- [ ] **Step 5.1: Delete the commented `From<nom::Err<...>> for ParsingErrorState` block in `src/error.rs`**

Find:

```bash
grep -n '// impl From<nom' src/error.rs
```

Delete the block (typically ~13 lines of `//`-prefixed lines).

- [ ] **Step 5.2: Delete the commented `parse_time_with_local_tz` block in `src/values.rs`**

```bash
grep -n '// fn parse_time_with_local_tz' src/values.rs
```

Delete the block.

- [ ] **Step 5.3: Delete the commented `make_err` block in `src/exif/exif_iter.rs`**

```bash
grep -n '// fn make_err' src/exif/exif_iter.rs
```

Delete the block (typically 5 lines).

- [ ] **Step 5.4: Build and test**

```bash
cargo build --all-features --examples 2>&1 | tail -3
cargo test --lib --all-features 2>&1 | tail -3
```

Expected: 202 tests still pass.

- [ ] **Step 5.5: Commit**

```bash
git add src/error.rs src/values.rs src/exif/exif_iter.rs
git commit -m "chore: delete stale commented-out code blocks"
```

---

## Task 6 — Final verification + tag

- [ ] **Step 6.1: Confirm the duplication delta**

```bash
wc -l src/parser.rs src/parser_async.rs
```

Compare against the Task 0.3 baseline. `src/parser_async.rs` should have shrunk by ~150–200 lines. `src/parser.rs` may have grown by ~50 (the new shared helpers). Net deduplication: ~100–150 lines.

If the numbers don't move meaningfully, something went wrong — revisit.

- [ ] **Step 6.2: Run full test suite (sync + async)**

```bash
cargo test --all-features 2>&1 | tail -3
cargo test --no-default-features 2>&1 | tail -3
```

Expected: both green.

- [ ] **Step 6.3: Doc + clippy**

```bash
cargo doc --no-deps --all-features 2>&1 | tail -3
cargo clippy --all-features -- -D warnings 2>&1 | tail -10
```

Expected: no new warnings.

- [ ] **Step 6.4: Public API surface check**

The whole point of P2 was internal changes only. Confirm no public symbol changed:

```bash
cargo +stable doc --no-deps --all-features 2>&1
diff <(git show v3-p1-done:src/lib.rs | grep -E '^pub use') <(grep -E '^pub use' src/lib.rs)
```

Expected: empty diff (lib.rs `pub use` lines unchanged from end of P1).

- [ ] **Step 6.5: Tag**

```bash
git tag v3-p2-done
```

---

## Self-review checklist

- [ ] `BufferedParserState` exists in `src/parser.rs` with fields `bb`, `buf`, `position`?
- [ ] `MediaParser` and `AsyncMediaParser` both compose `BufferedParserState` (no duplicate fields)?
- [ ] `Buf` and `ShareBuf` impls for `BufferedParserState` exist; the trait impls on `MediaParser`/`AsyncMediaParser` are pure forwarding?
- [ ] `parse_loop_step` is called from BOTH `BufParser::load_and_parse_with_offset` AND `AsyncBufParser::load_and_parse_with_offset`?
- [ ] `clear_and_skip_decide` is called from BOTH `clear_and_skip` impls?
- [ ] `check_fill_size` is called from BOTH `fill_buf` impls; the async bug is fixed?
- [ ] Public API in `src/lib.rs` `pub use` block unchanged?
- [ ] All commits compile + test green?
- [ ] Three commented-out dead blocks gone?
- [ ] `cargo test --all-features` and `cargo test --no-default-features` both green?

---

## Known follow-ups deferred to later phases

1. The read-and-discard loop in `clear_and_skip` (after `S::skip_by_seek` returns `false`) is still duplicated between sync/async. Extracting it cleanly requires a yield-aware abstraction; not worth it without `maybe_async` macros. P3 may revisit if `MediaSource`'s `S` parameter removal makes the seek path unconditional.
2. `MediaSource` and `AsyncMediaSource` are still separate types with duplicated `build()` / `has_exif()` / `has_track()` / constructors. P3 unifies on a single `MediaSource<R>` (no `S`) plus the v3 entry-point methods, at which point the duplication shrinks naturally.
3. The `MAX_ALLOC_SIZE` constant lives in `parser.rs`. It's also referenced from `parser_async.rs`. After P3's reorg this may move into a shared `internal/limits.rs` or similar. Not in scope here.
4. P3 will rename the `async` feature → `tokio`, which moves all `#[cfg(feature = "async")]` gates throughout `src/`. Expected churn: modest; the gates today are at module boundaries (`mod parser_async;`) and a few helper definitions in `skip.rs` / `error.rs`.

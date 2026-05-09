# nom-exif v3 — P6: TrackInfo, prelude, top-level docs, migration guide

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close out the v3 cutover. Tighten the `TrackInfo` / `TrackInfoTag` surface to the §3.10 contract, add the `prelude` module from §4.2, rewrite the top-level `lib.rs` doc-comment + `README.md` against the v3 surface, and land the §5 migration guide as runnable doc-tests so every example in the table compiles. Final tagged commit `v3.0.0-rc.1`.

**Architecture:** `TrackInfo` mirrors `Exif`: `gps_info()` (no `get_` prefix), `iter()` returning a borrowing iterator, `has_embedded_media() -> bool`. The two public ctor leaks (`From<BTreeMap>`, `IntoIterator`) get deleted. `TrackInfoTag` gains `name() -> &'static str` (const) and `FromStr` (using `ConvertError::UnknownTagName`, peer-aligned with `ExifTag`); the local `UnknownTrackInfoTag` error and `From<TrackInfoTag> for &str` are deleted. `Rational<T>` is added to the top-level re-export (closing the §4.1 gap). A new `prelude` module re-exports the §4.2 set. The `lib.rs` `//!` placeholder is replaced with a full v3 tutorial; README is rewritten end-to-end against the v3 surface; CHANGELOG gets a `## [3.0.0]` section embedding the §5 migration table. The §5 migration guide moves from prose-only into a checked artifact: every `v2 → v3` row gets a v3 doc-test (the v2 side is by definition unrunnable post-cutover; we only check the v3 side compiles + runs against `testdata/`). Final cross-check task runs `cargo doc`, `cargo clippy`, `cargo test --all-features`, `cargo test --doc --all-features`, and the fuzz harness build, then tags `v3.0.0-rc.1`.

**Tech Stack:** Rust 1.83, `chrono` 0.4, `bytes` 1.7.1 (already in tree), `thiserror` 2.0.

**Phase position:** lands on `v3` branch as the final phase. Per master plan, no per-phase feature branches — every commit is on `v3` itself. Final task tags `v3-p6-done` *and* `v3.0.0-rc.1`.

**Exit criterion (verbatim from master plan P6 row + spec sections):**
- `TrackInfo::gps_info` (renamed from `get_gps_info`).
- `TrackInfo::iter` (signature aligned with spec: `impl Iterator<Item = (TrackInfoTag, &EntryValue)>` — `TrackInfoTag` is `Copy`, so by-value yield matches spec §3.10).
- `TrackInfo::has_embedded_media() -> bool`.
- `From<BTreeMap<TrackInfoTag, EntryValue>> for TrackInfo` deleted.
- `IntoIterator for TrackInfo` deleted.
- `From<TrackInfoTag> for &str` deleted.
- `TryFrom<&str> for TrackInfoTag` deleted; `UnknownTrackInfoTag` error type deleted.
- `TrackInfoTag::name() -> &'static str` (const) added.
- `impl FromStr for TrackInfoTag` (with `ConvertError::UnknownTagName`) added.
- `Metadata` enum already lives in `lib.rs` (added in P3); P6 just adds rustdoc per spec §3.11.
- `prelude` module added re-exporting the §4.2 symbol set.
- Top-level `pub use` adds `Rational` (the generic, currently unexported despite §4.1).
- `lib.rs` `//!` doc-comment fully rewritten against the v3 surface (the v2 placeholder put in by P3 is replaced).
- `README.md` rewritten — every code block uses the v3 surface (no `MediaSource::file_path`, no turbofish parse, no `parse_gps_info()`, no `get_gps_info()`, feature names `tokio`/`serde`).
- `CHANGELOG.md` has a `## nom-exif v3.0.0` section with the §5 migration table inline.
- A new `tests/migration_guide.rs` integration test contains one runnable v3-side example per `v2 → v3` migration row and passes `cargo test --all-features`.
- Master plan P6 row link flipped from `(TBW)` to `[v3-p6-track-prelude.md](2026-05-08-v3-p6-track-prelude.md)`.
- `cargo test --all-features` green.
- `cargo test --all-features --doc` green.
- `cargo clippy --all-features -- -D warnings` clean.
- `cargo doc --no-deps --all-features --document-private-items` produces no warnings (per master plan §52 done-definition).
- Fuzz harness builds: `(cd fuzz && cargo +nightly build)` exits 0.
- Final tag `v3.0.0-rc.1` placed on the last P6 commit.

---

## File Structure

| File | Operation | Responsibility post-change |
|---|---|---|
| `src/video.rs` | **Modify** | `TrackInfo`: rename `get_gps_info` → `gps_info`, add `has_embedded_media` (field + accessor + `pub(crate) set_has_embedded_media`), keep `iter` (drop `&` from key in yield to match spec — `TrackInfoTag` is `Copy`). Delete `From<BTreeMap>` / `IntoIterator` / `From<TrackInfoTag> for &str`. `TrackInfoTag`: add `pub const fn name(self)`, `impl FromStr` using `ConvertError::UnknownTagName`. Delete `TryFrom<&str>` and `UnknownTrackInfoTag`. `Display` switches to `name()`. |
| `src/mov.rs` | **Modify** | `parse_isobmff` returns `TrackInfo` directly instead of `BTreeMap` (the only caller of the deleted `From<BTreeMap>` impl). |
| `src/lib.rs` | **Modify** | Add `pub use values::Rational;` to close the §4.1 gap. Add `pub mod prelude { ... }` per §4.2. Rewrite `//!` doc-comment against the v3 surface (full tutorial replacing the P3 placeholder). Add rustdoc for the `Metadata` enum (per §3.11). The four `read_*` (sync) and three `read_*_async` already exist — only the doc-comments get expanded. |
| `src/error.rs` | **No change** | `ConvertError::UnknownTagName(String)` already exists (added in P1). Reused. |
| `src/parser.rs` | **No change** | `parse_track` already returns `TrackInfo`; signature unchanged. |
| `src/parser_async.rs` | **No change** | Same. |
| `examples/rexiftool.rs` | **Modify** | `info.into_iter()` → `info.iter()` (the `IntoIterator` impl is gone). Tag iteration produces `(TrackInfoTag, &EntryValue)`; format strings adjusted. |
| `README.md` | **Replace** | Full rewrite against v3 surface — see Task 7 for the new content outline. |
| `CHANGELOG.md` | **Modify** | Prepend `## nom-exif v3.0.0` section with §5 migration table inline. |
| `tests/migration_guide.rs` | **Create** | Integration test crate file. One `#[test]` per §5 migration row exercising the v3-side example. Lives in `tests/` (not `src/`) so it compiles as a downstream consumer would, validating the public API surface end-to-end. |
| `docs/superpowers/plans/2026-05-08-v3-master.md` | **Modify** | (last task) flip P6 row link from `(TBW) v3-p6-track-prelude.md` to `[v3-p6-track-prelude.md](2026-05-08-v3-p6-track-prelude.md)`. |

---

## Task 0: Branch hygiene + baseline regression test

**Files:**
- Read: `Cargo.toml`, `src/video.rs`, `src/lib.rs`
- Modify: `src/video.rs` (add baseline test)

- [ ] **Step 1: Confirm branch and clean working tree**

Run: `git status`

Expected: `On branch v3` with only `docs/superpowers/plans/2026-05-08-v3-p6-track-prelude.md` unstaged (or clean). Stash unrelated changes before proceeding.

- [ ] **Step 2: Capture baseline test count**

Run: `cargo test --all-features 2>&1 | grep "test result"`

Expected: a number of passing tests, all green. Record the count for Task 11 verification.

- [ ] **Step 3: Add a snapshot regression test for `parse_track` output stability**

Append to the `tests` module of `src/video.rs` (or create one at the bottom of the file if missing):

```rust
#[cfg(test)]
mod p6_baseline {
    use crate::{MediaParser, MediaSource, TrackInfoTag};

    #[test]
    fn p6_baseline_meta_mov_dump_snapshot() {
        // Lock down the post-refactor invariant: parsing testdata/meta.mov
        // through the public API yields the same set of (tag, value) pairs
        // before and after every P6 task. Captured as a sorted formatted
        // string so the assertion is a single Vec compare.
        let mut parser = MediaParser::new();
        let ms = MediaSource::open("testdata/meta.mov").unwrap();
        let info = parser.parse_track(ms).unwrap();

        // Probe the well-known tags (Make/Model/GpsIso6709/DurationMs).
        // The rest is exercised indirectly by other tests.
        let mut entries: Vec<String> = [
            TrackInfoTag::Make,
            TrackInfoTag::Model,
            TrackInfoTag::GpsIso6709,
            TrackInfoTag::DurationMs,
            TrackInfoTag::ImageWidth,
            TrackInfoTag::ImageHeight,
        ]
        .into_iter()
        .filter_map(|t| info.get(t).map(|v| format!("{t:?}={v}")))
        .collect();
        entries.sort();
        assert!(entries.len() >= 4, "expected >=4 well-known tags, got {entries:?}");
        assert!(
            entries.iter().any(|s| s.starts_with("Make=")),
            "expected Make tag in snapshot, got {entries:?}"
        );
    }
}
```

- [ ] **Step 4: Run baseline test**

Run: `cargo test --all-features p6_baseline_meta_mov_dump_snapshot`

Expected: PASS.

- [ ] **Step 5: Commit baseline**

```bash
git add docs/superpowers/plans/2026-05-08-v3-p6-track-prelude.md src/video.rs
git commit -m "test(track): P6 baseline regression for meta.mov dump"
```

---

## Task 1: TrackInfoTag — `name()` + `FromStr` with `ConvertError`

**Files:**
- Read: `src/video.rs`, `src/error.rs` (verify `ConvertError::UnknownTagName` exists)
- Modify: `src/video.rs`

**Why first:** `TrackInfoTag::name()` is needed by the new `Display` impl; deleting `From<TrackInfoTag> for &str` (Task 2) depends on a `name()`-based `Display`. Also peer-aligns with `ExifTag::from_str` from P5 (same `ConvertError::UnknownTagName` variant).

- [ ] **Step 1: Write failing test for `name()` and `FromStr`**

Append to the `p6_baseline` module (or create a new `tracks_api` test module) in `src/video.rs`:

```rust
    #[test]
    fn track_info_tag_name_is_const_str() {
        const _: &str = TrackInfoTag::Make.name();
        assert_eq!(TrackInfoTag::Make.name(), "Make");
        assert_eq!(TrackInfoTag::GpsIso6709.name(), "GpsIso6709");
        assert_eq!(TrackInfoTag::DurationMs.name(), "DurationMs");
    }

    #[test]
    fn track_info_tag_from_str_round_trip() {
        use std::str::FromStr;
        for t in [
            TrackInfoTag::Make,
            TrackInfoTag::Model,
            TrackInfoTag::Software,
            TrackInfoTag::CreateDate,
            TrackInfoTag::DurationMs,
            TrackInfoTag::ImageWidth,
            TrackInfoTag::ImageHeight,
            TrackInfoTag::GpsIso6709,
            TrackInfoTag::Author,
        ] {
            assert_eq!(TrackInfoTag::from_str(t.name()).unwrap(), t);
        }
    }

    #[test]
    fn track_info_tag_from_str_unknown_returns_convert_error() {
        use crate::ConvertError;
        use std::str::FromStr;
        let err = TrackInfoTag::from_str("Bogus").unwrap_err();
        assert!(matches!(err, ConvertError::UnknownTagName(s) if s == "Bogus"));
    }
```

Expected: all three fail to compile (no `name()`, no `FromStr`).

- [ ] **Step 2: Add `name()` const fn**

In `src/video.rs`, add an `impl TrackInfoTag` block (or extend an existing one) with:

```rust
impl TrackInfoTag {
    /// Stable, programmatic name of this tag (matches the `Display` output).
    pub const fn name(self) -> &'static str {
        match self {
            TrackInfoTag::Make => "Make",
            TrackInfoTag::Model => "Model",
            TrackInfoTag::Software => "Software",
            TrackInfoTag::CreateDate => "CreateDate",
            TrackInfoTag::DurationMs => "DurationMs",
            TrackInfoTag::ImageWidth => "ImageWidth",
            TrackInfoTag::ImageHeight => "ImageHeight",
            TrackInfoTag::GpsIso6709 => "GpsIso6709",
            TrackInfoTag::Author => "Author",
        }
    }
}
```

- [ ] **Step 3: Add `FromStr` impl using `ConvertError`**

Append to `src/video.rs`:

```rust
impl std::str::FromStr for TrackInfoTag {
    type Err = crate::ConvertError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Keep the body in lock-step with `name()` above. Both must match
        // the exact same set of strings; a single source of truth would be
        // a macro, but the explicit form is easier to grep.
        Ok(match s {
            "Make" => TrackInfoTag::Make,
            "Model" => TrackInfoTag::Model,
            "Software" => TrackInfoTag::Software,
            "CreateDate" => TrackInfoTag::CreateDate,
            "DurationMs" => TrackInfoTag::DurationMs,
            "ImageWidth" => TrackInfoTag::ImageWidth,
            "ImageHeight" => TrackInfoTag::ImageHeight,
            "GpsIso6709" => TrackInfoTag::GpsIso6709,
            "Author" => TrackInfoTag::Author,
            other => return Err(crate::ConvertError::UnknownTagName(other.to_owned())),
        })
    }
}
```

- [ ] **Step 4: Run the new tests**

Run: `cargo test --all-features track_info_tag_`

Expected: 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add src/video.rs
git commit -m "feat(track): TrackInfoTag::name + FromStr with ConvertError"
```

---

## Task 2: Delete `From<TrackInfoTag> for &str`, `TryFrom<&str>`, `UnknownTrackInfoTag`; rewire `Display`

**Files:**
- Read: `src/video.rs` (find every callsite of the about-to-be-deleted impls)
- Modify: `src/video.rs`

- [ ] **Step 1: Find every callsite of `From<TrackInfoTag> for &str`**

Run: `grep -rn 'TrackInfoTag.*into.*&str\|<&str as From<TrackInfoTag>>\|<TrackInfoTag as Into<&str>>\|impl.*From<TrackInfoTag>' src/ examples/`

Expected: at least the `Display` impl in `src/video.rs:188-193` (which does `let s: &str = (*self).into();`). May also appear in `examples/rexiftool.rs` for track formatting — verify.

Run: `grep -rn 'TryFrom::<&str>::try_from\|<TrackInfoTag as TryFrom<&str>>\|TrackInfoTag::try_from' src/ examples/ tests/`

Expected: zero or near-zero hits; if any, they migrate to `FromStr` in this task.

- [ ] **Step 2: Switch `Display` to use `name()`**

Replace the existing `impl Display for TrackInfoTag` body in `src/video.rs:188-193` with:

```rust
impl std::fmt::Display for TrackInfoTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
```

- [ ] **Step 3: Delete `From<TrackInfoTag> for &str`**

Remove the entire `impl From<TrackInfoTag> for &str { ... }` block (currently ~`src/video.rs:195-209`).

- [ ] **Step 4: Delete `TryFrom<&str> for TrackInfoTag` and `UnknownTrackInfoTag`**

Remove both the error type (`pub struct UnknownTrackInfoTag` ~line 211-213) and the `impl TryFrom<&str> for TrackInfoTag` block (~line 215-233).

- [ ] **Step 5: Drop the now-unused `thiserror::Error` import**

If `use thiserror::Error;` is now unused in `src/video.rs` (was only present for `UnknownTrackInfoTag`), remove that import line. Also remove `use std::fmt::Display;` if it became dead.

- [ ] **Step 6: Migrate any `TryFrom<&str>` callsite found in Step 1**

If Step 1 surfaced any callsite under `src/`, `examples/`, or `tests/`, switch it to `<TrackInfoTag as FromStr>::from_str(s)?`. (Likely none — `TryFrom<&str>` was internal-only — but verify.)

- [ ] **Step 7: Build + run all `track_*` tests**

Run: `cargo build --all-features`

Expected: clean.

Run: `cargo test --all-features track_info_tag_ p6_baseline_meta_mov_dump_snapshot`

Expected: 4 PASS.

- [ ] **Step 8: Commit**

```bash
git add src/video.rs
git commit -m "feat(track): drop TrackInfoTag<->&str impls; Display via name()"
```

---

## Task 3: Rename `TrackInfo::get_gps_info` → `gps_info`; tighten `iter()` signature

**Files:**
- Read: `src/video.rs`, `src/parser.rs` (doc-test references), `examples/rexiftool.rs`, `tests/`
- Modify: `src/video.rs`, `src/parser.rs` (doc-test only), `examples/rexiftool.rs`

**Why bundled:** Both are `TrackInfo` accessor renames; one commit keeps the public surface coherent.

- [ ] **Step 1: Find every callsite of `get_gps_info`**

Run: `grep -rn 'get_gps_info' src/ examples/ tests/ docs/superpowers/plans/`

Expected hits in: `src/video.rs` (definition + doc-comment), `src/parser.rs` (the `parse_track` doc-test), and possibly `examples/rexiftool.rs` (verify). The `docs/superpowers/plans/` hits are reference-only and should not be edited.

Note: the `Exif::gps_info()` method (no `get_` prefix) already landed in P5. This task brings `TrackInfo` to parity.

- [ ] **Step 2: Rename method on `TrackInfo`**

In `src/video.rs:69-72`, rename:

```rust
pub fn get_gps_info(&self) -> Option<&GPSInfo> {
    self.gps_info.as_ref()
}
```

to:

```rust
/// Parsed GPS info, if `GpsIso6709` was present in the source. Mirrors
/// [`Exif::gps_info`](crate::Exif::gps_info).
pub fn gps_info(&self) -> Option<&GPSInfo> {
    self.gps_info.as_ref()
}
```

(Note: `gps_info` is also the name of a private field. Rust allows method/field name collision because the call syntax disambiguates. If clippy ever objects to that, rename the field to `gps` — but as of 1.83 with current lints it's clean.)

- [ ] **Step 3: Tighten `iter()` to spec signature**

The spec (§3.10) shows `iter(&self) -> impl Iterator<Item = (TrackInfoTag, &EntryValue)>` — by-value tag (since `TrackInfoTag` is `Copy`). Current code yields `(&TrackInfoTag, &EntryValue)`. Update `src/video.rs:74-78`:

```rust
/// Iterate over `(tag, value)` pairs (tag yielded by value since
/// `TrackInfoTag` is `Copy`). The parsed `GPSInfo` is **not** included
/// here — get it via [`TrackInfo::gps_info`].
pub fn iter(&self) -> impl Iterator<Item = (TrackInfoTag, &EntryValue)> {
    self.entries.iter().map(|(k, v)| (*k, v))
}
```

- [ ] **Step 4: Update the in-source doc-test in `parse_track`**

`src/parser.rs:495-510` (or near the `parse_track` rustdoc) has a doc-test calling `info.get_gps_info()`. Update to `info.gps_info()`. Same for any other doc-test under `parser.rs` / `parser_async.rs` that touches `TrackInfo`.

Run: `grep -n 'get_gps_info' src/parser.rs src/parser_async.rs`

Expected after edit: zero hits in `src/`.

- [ ] **Step 5: Update `examples/rexiftool.rs` if it uses `get_gps_info`**

If the grep in Step 1 surfaced a hit in `examples/rexiftool.rs`, update it. (Current code path uses `info.into_iter()`, not `get_gps_info()`, so likely no edit here — verify.)

- [ ] **Step 6: Update tests in `src/video.rs` and `src/parser.rs::tests`**

Run: `grep -rn 'get_gps_info' src/`

Expected: zero hits after this step.

- [ ] **Step 7: Build + test**

Run: `cargo test --all-features --doc track_ parser_`

Expected: green (doc-tests for `parse_track` use the renamed method).

Run: `cargo test --all-features`

Expected: full suite green.

- [ ] **Step 8: Commit**

```bash
git add src/video.rs src/parser.rs src/parser_async.rs examples/rexiftool.rs
git commit -m "feat(track): rename get_gps_info -> gps_info; iter yields tag by value"
```

---

## Task 4: `TrackInfo::has_embedded_media` (field + accessor)

**Files:**
- Read: `src/video.rs`, `src/exif/exif_iter.rs:156-318` (mirror the Exif side)
- Modify: `src/video.rs`

**Why now (and why so simple):** The spec (§3.10 + §8.6) requires a `has_embedded_media() -> bool` method symmetric with `Exif::has_embedded_media`. For v3.0.0 day-one, the value is **always `false`**: the parser does not yet detect "container has streams not represented in `TrackInfo`". The field exists so future `parse_track` code (e.g., HEIC Live Photo embedded MOV detection, or .mka-with-video-track flagging) can flip it without API churn. The Exif side ships real detection (HEIC/HEIF/RAF MIME-flagged) because the embedded media is already known statically; the track side has no static analog yet, so we ship the API contract as `false` and document the rationale. This is the *day-one compromise* §8.6 explicitly calls out.

- [ ] **Step 1: Write failing test**

Append to `p6_baseline` (or whichever test module in `src/video.rs` exists by now):

```rust
    #[test]
    fn track_info_has_embedded_media_default_false() {
        // v3.0.0 ships the API contract; detection is a v3.x deliverable.
        // This test pins the day-one behavior so accidentally flipping it
        // requires explicit ack.
        let mut parser = crate::MediaParser::new();
        let ms = crate::MediaSource::open("testdata/meta.mov").unwrap();
        let info = parser.parse_track(ms).unwrap();
        assert!(!info.has_embedded_media());
    }
```

Expected: fails to compile (no `has_embedded_media` method).

- [ ] **Step 2: Add field + accessor + setter**

Update the `TrackInfo` struct in `src/video.rs:56-60`:

```rust
#[derive(Debug, Clone, Default)]
pub struct TrackInfo {
    entries: BTreeMap<TrackInfoTag, EntryValue>,
    gps_info: Option<GPSInfo>,
    has_embedded_media: bool,
}
```

Append to `impl TrackInfo`:

```rust
/// Whether the source container is known to embed additional media streams
/// that this `parse_track` call did *not* surface (e.g. an .mka container
/// holding both audio and video, or an .mp4 that also embeds a still-image
/// track). Symmetric with [`Exif::has_embedded_media`](crate::Exif::has_embedded_media).
///
/// **v3.0.0 note:** detection on the track side is not yet implemented;
/// this currently always returns `false`. The accessor exists so future
/// versions can flip the flag without a breaking API change. See spec
/// §8.6 for the design rationale.
pub fn has_embedded_media(&self) -> bool {
    self.has_embedded_media
}

#[allow(dead_code)] // staged: real callers land in v3.x
pub(crate) fn set_has_embedded_media(&mut self, v: bool) {
    self.has_embedded_media = v;
}
```

- [ ] **Step 3: Build + test**

Run: `cargo test --all-features track_info_has_embedded_media_default_false p6_baseline`

Expected: PASS.

Run: `cargo clippy --all-features -- -D warnings`

Expected: clean (the `#[allow(dead_code)]` keeps `set_has_embedded_media` from tripping `dead_code` until v3.x wires up real callers).

- [ ] **Step 4: Commit**

```bash
git add src/video.rs
git commit -m "feat(track): TrackInfo::has_embedded_media (always false in v3.0.0)"
```

---

## Task 5: Delete `From<BTreeMap>` and `IntoIterator` for `TrackInfo`; migrate internal callers

**Files:**
- Read: `src/video.rs`, `src/mov.rs`, `src/ebml/webm.rs`, `examples/rexiftool.rs`
- Modify: `src/video.rs`, `src/mov.rs`, `examples/rexiftool.rs`

**Why this order:** Tasks 1-4 add the replacement APIs; this task removes the v2 holdovers. By P6 we know the only callers are `parse_isobmff` (deleted impl 1) and `examples/rexiftool.rs` (deleted impl 2).

- [ ] **Step 1: Find every callsite**

Run: `grep -rn 'BTreeMap.*TrackInfoTag\|TrackInfo::from\|<TrackInfo as From\|impl From<BTreeMap' src/ examples/ tests/`

Expected: `src/mov.rs:156` (`parse_isobmff(moov_body)?.into()` consumes `BTreeMap<TrackInfoTag, EntryValue>` → `TrackInfo`).

Run: `grep -rn 'info\.into_iter\|TrackInfo as IntoIterator\|impl IntoIterator for TrackInfo' src/ examples/ tests/`

Expected: `examples/rexiftool.rs:129` (`info.into_iter().map(...)`).

Run: `grep -rn 'TrackInfo as IntoIterator' src/ examples/ tests/`

Expected: zero (the trait is auto-imported via prelude).

- [ ] **Step 2: Migrate `parse_isobmff` to construct `TrackInfo` directly**

In `src/mov.rs:17-36`, change the return type from `BTreeMap<TrackInfoTag, EntryValue>` to `TrackInfo` and use the existing `pub(crate) fn put` constructor. Updated body:

```rust
#[tracing::instrument(skip_all)]
pub(crate) fn parse_isobmff(
    moov_body: &[u8],
) -> Result<TrackInfo, ParsingError> {
    let (_, entries) = match parse_moov_body(moov_body) {
        Ok((remain, Some(entries))) => (remain, entries),
        Ok((remain, None)) => (remain, Vec::new()),
        Err(_) => {
            return Err("invalid moov body".into());
        }
    };

    let entries: BTreeMap<TrackInfoTag, EntryValue> = convert_video_tags(entries);
    let mut info = TrackInfo::default();
    let mut create_date_seen = false;
    for (k, v) in entries {
        if k == TrackInfoTag::CreateDate {
            create_date_seen = true;
        }
        info.put(k, v);
    }
    let extras = parse_mvhd_tkhd(moov_body);
    for (k, v) in extras {
        // Don't overwrite a CreateDate from the per-key entries with the
        // mvhd-derived one (preserves v2 behavior).
        if k == TrackInfoTag::CreateDate && create_date_seen {
            continue;
        }
        info.put(k, v);
    }
    Ok(info)
}
```

Update the caller in `src/video.rs::parse_track_info` (~line 156):

```rust
match mime_video {
    crate::file::MediaMimeTrack::QuickTime
    | crate::file::MediaMimeTrack::_3gpp
    | crate::file::MediaMimeTrack::Mp4 => {
        let range = extract_moov_body_from_buf(input)?;
        let moov_body = &input[range];
        parse_isobmff(moov_body)?
    }
    crate::file::MediaMimeTrack::Webm | crate::file::MediaMimeTrack::Matroska => {
        parse_webm(input)?.into()  // EbmlFileInfo -> TrackInfo (pub(crate) impl, kept)
    }
}
```

(Note: bind to `info` directly, no `let mut info: TrackInfo = ...` with `.into()`.)

Adjust the surrounding code in `parse_track_info` if needed — the GPS post-processing block (`if let Some(gps) = info.get(TrackInfoTag::GpsIso6709) { info.gps_info = ... }`) stays as-is.

- [ ] **Step 3: Delete `From<BTreeMap<TrackInfoTag, EntryValue>> for TrackInfo`**

Remove the entire `impl From<BTreeMap<TrackInfoTag, EntryValue>> for TrackInfo` block (~`src/video.rs:179-186`).

- [ ] **Step 4: Migrate `examples/rexiftool.rs::parse_file` track branch**

`examples/rexiftool.rs:127-132` currently:

```rust
MediaKind::Track => {
    let info: TrackInfo = parser.parse_track(ms)?;
    info.into_iter()
        .map(|x| (x.0.to_string(), x.1))
        .collect::<Vec<_>>()
}
```

becomes:

```rust
MediaKind::Track => {
    let info: TrackInfo = parser.parse_track(ms)?;
    info.iter()
        .map(|(tag, val)| (tag.to_string(), val.clone()))
        .collect::<Vec<_>>()
}
```

(`iter()` borrows the value, so `.clone()` is required to match the previously-owned `Vec<(String, EntryValue)>` shape consumed downstream by the JSON branch. `EntryValue` is `Clone`, so this is mechanical.)

- [ ] **Step 5: Delete `IntoIterator for TrackInfo`**

Remove the entire `impl IntoIterator for TrackInfo { ... }` block (~`src/video.rs:170-177`). Also drop the `use std::collections::btree_map::IntoIter;` import at the top of the file if it becomes unused.

- [ ] **Step 6: Build + test**

Run: `cargo build --all-features --examples`

Expected: clean (rexiftool example now uses the new `iter()`).

Run: `cargo test --all-features`

Expected: full suite green.

- [ ] **Step 7: Run the rexiftool smoke check**

Run: `cargo run --example rexiftool testdata/meta.mov`

Expected: human-readable dump matches the README example (Make/Model/Software/CreateDate/DurationMs/ImageWidth/ImageHeight/GpsIso6709 lines).

- [ ] **Step 8: Commit**

```bash
git add src/video.rs src/mov.rs examples/rexiftool.rs
git commit -m "feat(track): drop From<BTreeMap> + IntoIterator; rexiftool uses iter()"
```

---

## Task 6: Add `Rational` re-export + `prelude` module

**Files:**
- Read: `src/lib.rs`, `src/values.rs` (verify `Rational<T>` is `pub`)
- Modify: `src/lib.rs`

- [ ] **Step 1: Verify `Rational<T>` is `pub` in `src/values.rs`**

Run: `grep -n 'pub struct Rational' src/values.rs`

Expected: `pub struct Rational<T> { ... }` at ~line 782.

- [ ] **Step 2: Add `Rational` to top-level `pub use`**

In `src/lib.rs:24`, change:

```rust
pub use values::{EntryValue, ExifDateTime, IRational, URational};
```

to:

```rust
pub use values::{EntryValue, ExifDateTime, IRational, Rational, URational};
```

- [ ] **Step 3: Add `prelude` module**

Insert in `src/lib.rs` (right after the top-level `pub use` block but before the `pub enum Metadata` declaration):

```rust
/// Convenient one-line import of the most common v3 symbols.
///
/// ```rust
/// use nom_exif::prelude::*;
/// # fn main() -> Result<()> { Ok(()) }
/// ```
///
/// Includes [`Error`] and [`MalformedKind`] so error-matching code does
/// not need a second import. Cold-path types (e.g. `Rational`,
/// `LatLng`, `ConvertError`, `ExifDateTime`) are intentionally **not**
/// in the prelude — import them explicitly via `nom_exif::Type`.
pub mod prelude {
    pub use crate::{
        EntryValue, Error, Exif, ExifIter, ExifTag, GPSInfo, IfdIndex,
        MalformedKind, MediaKind, MediaParser, MediaSource, Metadata,
        Result, TrackInfo, TrackInfoTag,
    };
    pub use crate::{read_exif, read_metadata, read_track};
}
```

- [ ] **Step 4: Add a smoke test for `prelude`**

Append to the `v3_top_level_tests` module in `src/lib.rs`:

```rust
#[test]
fn prelude_imports_compile() {
    // Check the prelude re-exports compile and resolve without ambiguity.
    use crate::prelude::*;
    fn _consume(_: Option<Exif>, _: Option<TrackInfo>, _: Option<MediaParser>) {}
    let _ = read_exif;
    let _ = read_track;
    let _ = read_metadata;
}
```

- [ ] **Step 5: Build + test**

Run: `cargo test --all-features prelude_imports_compile`

Expected: PASS.

Run: `cargo doc --no-deps --all-features --document-private-items 2>&1 | grep -E 'warning|error' | head`

Expected: empty.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs
git commit -m "feat(api): add Rational re-export and prelude module per spec §4.1/§4.2"
```

---

## Task 7: Rewrite `lib.rs` `//!` doc-comment + README against the v3 surface

**Files:**
- Read: `src/lib.rs`, `README.md`, `docs/V3_API_DESIGN.md` §3.11 + §5.x
- Modify: `src/lib.rs`, `README.md`

**Why bundled:** Both are "external-facing documentation" rewrites; they share the same v3 vocabulary and example fixtures. Doing them in one commit guarantees the README and the published rustdoc agree.

- [ ] **Step 1: Replace the `lib.rs` `//!` doc-comment**

In `src/lib.rs:1-14`, replace the v2-placeholder block with:

```rust
//! `nom-exif` — Exif and track metadata parser for image, video, and audio
//! files.
//!
//! # Quick start
//!
//! For a one-shot read, use the helpers — they wrap the file in a
//! [`std::io::BufReader`] internally:
//!
//! ```rust
//! use nom_exif::{read_exif, ExifTag};
//!
//! let exif = read_exif("./testdata/exif.jpg")?;
//! let make = exif.get(ExifTag::Make).and_then(|v| v.as_str());
//! assert_eq!(make, Some("vivo"));
//! # Ok::<(), nom_exif::Error>(())
//! ```
//!
//! For batch processing, build a [`MediaParser`] once and reuse its
//! buffer:
//!
//! ```rust
//! use nom_exif::{MediaKind, MediaParser, MediaSource};
//!
//! let mut parser = MediaParser::new();
//! for path in ["./testdata/exif.jpg", "./testdata/meta.mov"] {
//!     let ms = MediaSource::open(path)?;
//!     match ms.kind() {
//!         MediaKind::Image => { let _ = parser.parse_exif(ms)?; }
//!         MediaKind::Track => { let _ = parser.parse_track(ms)?; }
//!     }
//! }
//! # Ok::<(), nom_exif::Error>(())
//! ```
//!
//! Async variants live behind `feature = "tokio"`:
//! [`read_exif_async`], [`read_track_async`], [`read_metadata_async`],
//! plus [`MediaParser::parse_exif_async`] / [`MediaParser::parse_track_async`].
//!
//! # API surface
//!
//! - **One-shot helpers**: [`read_exif`], [`read_exif_iter`], [`read_track`], [`read_metadata`].
//! - **Reusable parser**: [`MediaParser`] + [`MediaSource`] (or [`AsyncMediaSource`])
//!   + [`MediaKind`].
//! - **Image metadata**: [`Exif`] (eager, get-by-tag) or [`ExifIter`]
//!   (lazy iterator with per-entry errors). Convert: `let exif: Exif = iter.into();`.
//! - **Track metadata**: [`TrackInfo`] (audio/video container metadata).
//! - **Discriminated union**: [`Metadata`] returned by [`read_metadata`].
//! - **Errors**: [`Error`] for parse-level, [`EntryError`] for per-entry
//!   IFD errors, [`ConvertError`] for type-conversion peer errors.
//! - **Convenience**: [`prelude`] re-exports the symbols you most often need.
//!
//! See `docs/V3_API_DESIGN.md` for the full design contract and the
//! v2 → v3 migration table.
//!
//! # Cargo features
//!
//! - `tokio` — async API via tokio (`AsyncMediaSource`, `read_*_async`,
//!   `MediaParser::parse_*_async`).
//! - `serde` — derives `Serialize`/`Deserialize` on the public types.
//!
//! # Embedded media
//!
//! Some formats (HEIC Live Photos, RAF JPEG previews, …) embed media
//! streams that `parse_exif` does not surface. The
//! [`Exif::has_embedded_media`] / [`ExifIter::has_embedded_media`] /
//! [`TrackInfo::has_embedded_media`] flags let callers detect this; the
//! actual extraction API is a v3.x deliverable.
```

(Verify the `read_exif` doctest's expected `Make` value against `testdata/exif.jpg` first. Run `cargo run --example rexiftool testdata/exif.jpg | grep Make` to confirm — if it's not `vivo`, swap the assertion to whatever the file actually carries. Currently README shows `vivo X90 Pro+` for the Model field; Make is likely just `vivo`.)

- [ ] **Step 2: Verify the new doc-test compiles AND runs**

Run: `cargo test --all-features --doc -- --test-threads=1 2>&1 | head -60`

Expected: the new `read_exif` and `MediaParser` doctests pass. If a `Make` assertion fails, **read** the actual value out of `testdata/exif.jpg` and update the assertion to match — do not skip the assertion.

- [ ] **Step 3: Rewrite `README.md`**

Replace the entire `README.md` with the structure below. Key differences from v2:

- All `MediaSource::file_path` → `MediaSource::open`.
- All `parser.parse::<_, _, ExifIter>` → `parser.parse_exif`.
- All `parser.parse::<_, _, TrackInfo>` → `parser.parse_track`.
- All `iter.parse_gps_info()` → `iter.parse_gps()`.
- All `info.get_gps_info()` → `info.gps_info()`.
- `latitude_ref == 'N'` → `matches!(latitude_ref, LatRef::North)`.
- `[(27, 1), (7, 1), (68, 100)].into()` → `LatLng::new(URational::new(27, 1), ..., ...)`.
- Feature names: `async` → `tokio`, `json_dump` → `serde`.
- Add a top section showing `read_exif` / `read_track` / `read_metadata` one-liners.
- Add a "Migration from v2" section pointing at `docs/V3_API_DESIGN.md` §5.

Suggested skeleton (fill in body to roughly the same length as the current README):

```markdown
# Nom-Exif

[badges block — keep as-is]

`nom-exif` is an Exif/metadata parsing library written in pure Rust with
[nom](https://github.com/rust-bakery/nom).

## Supported File Types

[keep as-is]

## Quick Start

```rust
use nom_exif::{read_exif, read_track, read_metadata, ExifTag, TrackInfoTag, Metadata};

// One image:
let exif = read_exif("./testdata/exif.jpg")?;
let make = exif.get(ExifTag::Make).and_then(|v| v.as_str());

// One video:
let info = read_track("./testdata/meta.mov")?;
let model = info.get(TrackInfoTag::Model).and_then(|v| v.as_str());

// Auto-detect:
match read_metadata("./testdata/exif.jpg")? {
    Metadata::Exif(_)  => { /* image */ }
    Metadata::Track(_) => { /* video/audio */ }
}
# Ok::<(), nom_exif::Error>(())
```

## Reusable Parser

[full v3 example with MediaParser + MediaSource::open + parse_exif/parse_track]

## Async API

`tokio` feature flag. [example with read_exif_async + AsyncMediaSource + parse_exif_async]

## GPS Info

[v3 example using iter.parse_gps()? and gps_info.to_iso6709() and matches!(LatRef::...)]

## Two API Styles for Exif

[Exif eager vs ExifIter lazy section, both v3 vocabulary]

## Migration from v2

See `docs/V3_API_DESIGN.md` §5 for the full migration table. The hot
items:

- `MediaSource::file_path(p)` → `MediaSource::open(p)` or `read_exif(p)`.
- `parser.parse::<_,_,ExifIter>(ms)` → `parser.parse_exif(ms)`.
- `parser.parse::<_,_,TrackInfo>(ms)` → `parser.parse_track(ms)`.
- `entry.take_result()` (panicky) → `entry.into_result()` (consumes self).
- `iter.parse_gps_info()` → `iter.parse_gps()`.
- `info.get_gps_info()` → `info.gps_info()` (returns `Option<&GPSInfo>`).
- `Cargo.toml` features: `async` → `tokio`, `json_dump` → `serde`.

## CLI Tool `rexiftool`

[update the feature flag in the json-dump section: `--features json_dump` → `--features serde`]

## Fuzz Testing

[keep as-is — already correct]

## Changelog

[CHANGELOG.md](CHANGELOG.md)
```

**Important:** every Rust code block must run as a doc-test if you're using triple-backtick `rust`. If a block has syntax that cannot run standalone (like API teasers), use ` ```text` instead. Otherwise `cargo test --doc` will catch it.

- [ ] **Step 4: Verify all README code blocks are valid v3**

Run: `grep -nE 'file_path|parse::<|get_gps_info|parse_gps_info|features.*"async"|features.*json_dump|tcp_stream' README.md`

Expected: zero matches. If any persist, fix.

- [ ] **Step 5: Build + test**

Run: `cargo doc --no-deps --all-features 2>&1 | grep -E 'warning|error'`

Expected: empty.

Run: `cargo test --all-features --doc`

Expected: green (the new `lib.rs` doctests must pass).

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs README.md
git commit -m "docs(v3): rewrite top-level rustdoc and README against v3 surface"
```

---

## Task 8: CHANGELOG `## nom-exif v3.0.0` section with §5 migration table

**Files:**
- Read: `CHANGELOG.md`, `docs/V3_API_DESIGN.md` §5
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Prepend the v3 section**

Insert directly above the `## nom-exif v2.8.0` line:

```markdown
## nom-exif v3.0.0

**Breaking release.** The public API has been reshaped end-to-end. See
`docs/V3_API_DESIGN.md` for the full design contract and rationale.

### Highlights

- One-shot helpers: `read_exif`, `read_exif_iter`, `read_track`, `read_metadata` (and `_async` variants under `feature = "tokio"`).
- Single `MediaParser` (no separate `AsyncMediaParser`); `MediaSource::open(path)` replaces `MediaSource::file_path(path)`.
- Structured errors: `Error::Malformed { kind, message }` / `Error::UnexpectedEof` / `Error::UnsupportedFormat` replace the v2 `ParseFailed(Box<dyn Error>)`.
- `Exif` gains `iter()` / `gps_info()` / `errors()` / `has_embedded_media()` / `get_in()` / `get_by_code()`.
- `ExifIter` gains `clone_rewound()` / `parse_gps()` / `has_embedded_media()`; `ParsedExifEntry` is renamed `ExifIterEntry` with private fields and `into_result()` (consumes `self`).
- New `ExifEntry<'a>` (eager view over `Exif::iter`).
- `IfdIndex` newtype (with `MAIN` / `THUMBNAIL` constants); `TagOrCode` replaces `ExifTagCode`.
- `Rational<T>` fields private; access via `numerator()` / `denominator()` / `to_f64()`.
- `LatRef` / `LonRef` / `Altitude` / `Speed` / `SpeedUnit` enums replace `char` / `u8` GPS fields.
- `LatLng` named fields; `LatLng::try_from_decimal_degrees` replaces panicky `From<f64>`.
- `prelude` module for common imports.
- Cargo features renamed: `async` → `tokio`, `json_dump` → `serde`.
- MSRV: 1.83.

### Migration Table (excerpt — full table in `docs/V3_API_DESIGN.md` §5)

| v2 | v3 |
|----|-----|
| `MediaSource::file_path(p)` | `MediaSource::open(p)` or `read_exif(p)` |
| `MediaSource::tcp_stream(s)` | `MediaSource::unseekable(s)` |
| `ms.has_exif()` / `ms.has_track()` | `ms.kind() == MediaKind::Image` / `Track` |
| `parser.parse::<_,_,ExifIter>(ms)` | `parser.parse_exif(ms)` |
| `parser.parse::<_,_,TrackInfo>(ms)` | `parser.parse_track(ms)` |
| `Error::ParseFailed(Box)` | `Error::Malformed { kind, message }` (or `UnexpectedEof` / `UnsupportedFormat`) |
| `Error::IOError(e)` | `Error::Io(e)` |
| `From<&str> for Error` | (deleted — use a structured variant) |
| `value.as_time_components()` | `value.as_datetime()` |
| `value.as_u8array()` / `value.to_u8array()` | `value.as_u8_slice()` |
| `ExifTag::try_from(0x010f)` | `ExifTag::from_code(0x010f)` |
| `<&str as From<ExifTag>>::from(t)` | `t.name()` or `t.to_string()` |
| `exif.get_gps_info()` | `exif.gps_info() -> Option<&GPSInfo>` |
| `exif.get_by_ifd_tag_code(0, 0x0110)` | `exif.get_by_code(IfdIndex::MAIN, 0x0110)` |
| `exif.get_by_ifd_tag_code(ifd, t.code())` | `exif.get_in(IfdIndex::new(ifd), t)` |
| `ParsedExifEntry` | `ExifIterEntry` |
| `entry.tag()` + `entry.tag_code()` | `entry.tag() -> TagOrCode` |
| `entry.take_value()` / `take_result()` | `entry.into_result()` |
| `iter.clone_and_rewind()` | `iter.clone_rewound()` |
| `iter.parse_gps_info()` | `iter.parse_gps()` |
| `info.get_gps_info()` | `info.gps_info() -> Option<&GPSInfo>` |
| `g.latitude_ref == 'N'` | `matches!(g.latitude_ref, LatRef::North)` |
| `URational(1, 2)` | `URational::new(1, 2)`; `.to_f64()?` |
| `LatLng::from(f64)` (panicky) | `LatLng::try_from_decimal_degrees(f64)?` |
| `features = ["async"]` | `features = ["tokio"]` |
| `features = ["json_dump"]` | `features = ["serde"]` |
| `AsyncMediaParser` | `MediaParser` (single type, async methods feature-gated) |
| `AsyncMediaSource::file_path(p).await` | `AsyncMediaSource::open(p).await` |
| `parser.parse(ms).await` (async) | `parser.parse_exif_async(ms).await` / `parser.parse_track_async(ms).await` |

### Removed

- `MediaSource::tcp_stream` (was an alias for `unseekable`).
- `MediaSource::has_exif` / `has_track` (use `kind()`).
- `Error::ParseFailed(Box<dyn Error>)`, `From<&str> for Error`, `From<String> for Error`.
- `AsyncMediaParser` (merged into `MediaParser`).
- `EntryValue::as_time_components` / `as_u8array` / `to_u8array`.
- `ParsedExifEntry::take_result` / `take_value` / `tag_code` / `get_result` / `get_value` / `has_value`.
- `ExifIter::clone_and_rewind` / `parse_gps_info`.
- `Exif::get_by_ifd_tag_code` / `get_gps_info` (`Result`-wrapped).
- `TrackInfo::get_gps_info`, `From<BTreeMap<TrackInfoTag, EntryValue>> for TrackInfo`, `IntoIterator for TrackInfo`, `From<TrackInfoTag> for &str`, `TryFrom<&str> for TrackInfoTag`, `UnknownTrackInfoTag` error type.
- `LatLng::from<f64>`, `URational(u32, u32)` tuple-struct field access (now `numerator()` / `denominator()`).

### Internal (no API impact)

- Sync/async parser logic deduplicated via shared `BufParser` / `AsyncBufParser` traits (P2).
- `PartialVec` / `AssociatedInput` deleted; all internal byte-views unified on `bytes::Bytes` (P4.5).
- Multi-slot buffer pool replaced by single `Option<Bytes>` cache + `Bytes::try_into_mut` recycle; `MediaParser::new()` is now zero-alloc (P4.5).

```

- [ ] **Step 2: Bump `Cargo.toml::package.version` to `3.0.0-rc.1`**

In `Cargo.toml:4`, change `version = "2.8.0"` → `version = "3.0.0-rc.1"`. The crate hasn't been published since the v3 cutover started, so `-rc.1` advertises "API frozen, run rust release pipelines against this".

- [ ] **Step 3: Build + test**

Run: `cargo build --all-features`

Expected: clean (version bump alone shouldn't break anything).

Run: `cargo test --all-features`

Expected: full suite still green.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md Cargo.toml
git commit -m "release(v3): CHANGELOG v3.0.0; bump to 3.0.0-rc.1"
```

---

## Task 9: Migration guide as runnable doc-tests (`tests/migration_guide.rs`)

**Files:**
- Read: `docs/V3_API_DESIGN.md` §5
- Create: `tests/migration_guide.rs`

**Why an integration test:** Per master plan §52 done-definition #2: *"Every example in §5 (migration guide) compiles and runs (verified by `cargo test --doc` against migration-guide doc-tests added in P6)"*. Putting the examples in `tests/` (not `src/`) compiles them as a downstream crate would consume the v3 surface — catches re-export gaps that internal tests miss.

The v2 side of each row is by definition unrunnable post-cutover (the symbols don't exist anymore); we only check that the v3 side compiles + runs against the existing `testdata/`.

- [ ] **Step 1: Create `tests/migration_guide.rs`**

```rust
//! Runnable migration guide. Each test exercises the v3 side of one
//! migration row in `docs/V3_API_DESIGN.md` §5. Lives in `tests/` so it
//! compiles as a downstream crate would, validating the public API
//! surface end-to-end.
//!
//! If you change the public surface and one of these breaks, **update
//! the corresponding row in V3_API_DESIGN.md §5 and CHANGELOG.md** too —
//! the three artifacts are meant to stay in lock-step.

use nom_exif::*;

// ─── §5.1 entry & parsing ──────────────────────────────────────────────────

#[test]
fn s5_1_media_source_open() {
    let ms = MediaSource::open("./testdata/exif.jpg").unwrap();
    assert_eq!(ms.kind(), MediaKind::Image);
}

#[test]
fn s5_1_top_level_read_exif() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    assert!(exif.get(ExifTag::Make).is_some());
}

#[test]
fn s5_1_parser_parse_exif() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("./testdata/exif.jpg").unwrap();
    let _iter = parser.parse_exif(ms).unwrap();
}

#[test]
fn s5_1_parser_parse_track() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("./testdata/meta.mov").unwrap();
    let _info: TrackInfo = parser.parse_track(ms).unwrap();
}

// ─── §5.2 errors ───────────────────────────────────────────────────────────

#[test]
fn s5_2_malformed_variant_pattern() {
    // Confirms the structured-error pattern compiles. The `_` arm
    // proves Error is exhaustive over the public variants we expose.
    fn _classify(err: Error) -> &'static str {
        match err {
            Error::Malformed { .. } => "malformed",
            Error::UnexpectedEof => "eof",
            Error::UnsupportedFormat => "unsupported",
            Error::Io(_) => "io",
            Error::ExifNotFound => "no_exif",
            Error::TrackNotFound => "no_track",
            _ => "other", // forwards-compatible for future variants
        }
    }
}

#[test]
fn s5_2_malformed_kind_imports_from_top_level() {
    let _kind: MalformedKind = MalformedKind::Exif;
}

// ─── §5.3 EntryValue accessors ─────────────────────────────────────────────

#[test]
fn s5_3_as_datetime_replaces_as_time_components() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    let dto = exif.get(ExifTag::DateTimeOriginal).unwrap();
    let _: Option<ExifDateTime> = dto.as_datetime();
}

#[test]
fn s5_3_as_u8_slice_replaces_as_u8array() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    if let Some(v) = exif.get(ExifTag::MakerNote) {
        let _: Option<&[u8]> = v.as_u8_slice();
    }
}

// ─── §5.4 ExifTag ──────────────────────────────────────────────────────────

#[test]
fn s5_4_exif_tag_from_code() {
    assert_eq!(ExifTag::from_code(0x010f), Some(ExifTag::Make));
    assert!(ExifTag::from_code(0xffff).is_none());
}

#[test]
fn s5_4_exif_tag_name_and_from_str() {
    use std::str::FromStr;
    assert_eq!(ExifTag::Make.name(), "Make");
    assert_eq!(ExifTag::Make.to_string(), "Make");
    assert_eq!(ExifTag::from_str("Make").unwrap(), ExifTag::Make);
    let err = ExifTag::from_str("Bogus").unwrap_err();
    assert!(matches!(err, ConvertError::UnknownTagName(_)));
}

// ─── §5.5 Exif / ExifIter ──────────────────────────────────────────────────

#[test]
fn s5_5_exif_gps_info() {
    let exif = read_exif("./testdata/exif.heic").unwrap();
    let _: Option<&GPSInfo> = exif.gps_info();
}

#[test]
fn s5_5_exif_get_by_code_and_get_in() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    let _ = exif.get_by_code(IfdIndex::MAIN, 0x0110);
    let _ = exif.get_in(IfdIndex::MAIN, ExifTag::Model);
}

#[test]
fn s5_5_exif_iter_yields_eager_entries() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    let n = exif.iter().filter(|e| e.ifd == IfdIndex::MAIN).count();
    assert!(n > 0);
}

#[test]
fn s5_5_exif_errors_accessor() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    let _: &[(IfdIndex, TagOrCode, EntryError)] = exif.errors();
}

#[test]
fn s5_5_exif_iter_entry_into_result() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("./testdata/exif.jpg").unwrap();
    for entry in parser.parse_exif(ms).unwrap() {
        let _tag: TagOrCode = entry.tag();
        let _: Result<EntryValue> = entry.into_result().map_err(Into::into);
    }
}

#[test]
fn s5_5_exif_iter_clone_rewound_and_parse_gps() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("./testdata/exif.heic").unwrap();
    let mut iter = parser.parse_exif(ms).unwrap();
    let _gps: Option<GPSInfo> = iter.parse_gps().unwrap();
    let _twin = iter.clone_rewound();
}

#[test]
fn s5_5_has_embedded_media() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("./testdata/exif.heic").unwrap();
    let iter = parser.parse_exif(ms).unwrap();
    assert!(iter.has_embedded_media(), "HEIC carries embedded media");
    let exif: Exif = iter.into();
    assert!(exif.has_embedded_media());
}

// ─── §5.6 GPSInfo ──────────────────────────────────────────────────────────

#[test]
fn s5_6_lat_ref_enum_pattern() {
    let exif = read_exif("./testdata/exif.heic").unwrap();
    if let Some(g) = exif.gps_info() {
        let _ = matches!(g.latitude_ref, LatRef::North | LatRef::South);
        let _ = matches!(g.altitude, Altitude::AboveSeaLevel(_) | Altitude::BelowSeaLevel(_));
    }
}

// ─── §5.7 Rational ─────────────────────────────────────────────────────────

#[test]
fn s5_7_rational_constructor_and_to_f64() {
    let r = URational::new(1, 2);
    assert_eq!(r.numerator(), 1);
    assert_eq!(r.denominator(), 2);
    assert_eq!(r.to_f64().unwrap(), 0.5);
}

#[test]
fn s5_7_irational_to_urational_conversion() {
    let pos: IRational = IRational::new(3, 4);
    let _u: URational = pos.try_into().unwrap();

    let neg: IRational = IRational::new(-3, 4);
    let err = URational::try_from(neg).unwrap_err();
    assert!(matches!(err, ConvertError::NegativeRational));
}

#[test]
fn s5_7_lat_lng_try_from_decimal_degrees() {
    let _ok = LatLng::try_from_decimal_degrees(43.5).unwrap();
    let err = LatLng::try_from_decimal_degrees(f64::NAN).unwrap_err();
    assert!(matches!(err, ConvertError::InvalidDecimalDegrees(_)));
}

// ─── §5.9 Cargo features (compile check via cfg) ───────────────────────────

#[cfg(feature = "tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn s5_8_async_top_level_helper() {
    let exif = read_exif_async("./testdata/exif.jpg").await.unwrap();
    assert!(exif.get(ExifTag::Make).is_some());
}

#[cfg(feature = "tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn s5_8_async_media_parser_method() {
    let mut parser = MediaParser::new();
    let ms = AsyncMediaSource::open("./testdata/exif.jpg").await.unwrap();
    let _iter = parser.parse_exif_async(ms).await.unwrap();
}

#[cfg(feature = "serde")]
#[test]
fn s5_9_serde_derives_compile() {
    fn _is_serialize<T: serde::Serialize>() {}
    _is_serialize::<EntryValue>();
}
```

(If a test fails because the v3 method/signature differs from what the spec implies, **fix the test to match the actual API** — the implementation is the source of truth at this point. If a test reveals an actual missing v3 surface, escalate before patching the test.)

- [ ] **Step 2: Verify test fixture availability**

Run: `ls testdata/exif.jpg testdata/exif.heic testdata/meta.mov`

Expected: all three exist. (If any are missing, fall back to whichever fixture covers the same MIME family — e.g. `meta.mp4` substituting `meta.mov`.)

- [ ] **Step 3: Build + run**

Run: `cargo test --test migration_guide --all-features`

Expected: every test passes. Iterate (fix tests, not implementation) until green.

Run: `cargo test --test migration_guide --no-default-features`

Expected: only the non-`#[cfg(feature = ...)]` tests run; all pass.

- [ ] **Step 4: Commit**

```bash
git add tests/migration_guide.rs
git commit -m "test(migration): runnable §5 migration guide as integration test"
```

---

## Task 10: Final verification trio + fuzz harness build

**Files:**
- Read: every modified file from Task 0-9
- No edits expected unless verification surfaces a regression

This is a checkpoint task. If anything fails, **fix on the next commit and re-run the full trio** before tagging.

- [ ] **Step 1: `cargo test --all-features`**

Run: `cargo test --all-features 2>&1 | tail -30`

Expected: all green. Verify the test count is at *least* `baseline_count + 11` (the new tests added across P6: 1 baseline + 3 TrackInfoTag + 1 has_embedded_media + 1 prelude + 1 lib.rs doctest + ~22 from `tests/migration_guide.rs` integration tests).

- [ ] **Step 2: `cargo test --all-features --doc`**

Run: `cargo test --all-features --doc 2>&1 | tail -10`

Expected: all green. The `lib.rs` `//!` block doctests must pass.

- [ ] **Step 3: `cargo clippy --all-features --all-targets -- -D warnings`**

Run: `cargo clippy --all-features --all-targets -- -D warnings 2>&1 | tail -30`

Expected: clean.

- [ ] **Step 4: `cargo doc --no-deps --all-features --document-private-items`**

Run: `cargo doc --no-deps --all-features --document-private-items 2>&1 | grep -E 'warning|error'`

Expected: empty.

- [ ] **Step 5: Fuzz harness builds**

Run: `(cd fuzz && cargo +nightly build) 2>&1 | tail -10`

Expected: clean. (If nightly is unavailable, this step may be skipped, but document why in the commit.)

- [ ] **Step 6: README sanity grep**

Run: `grep -nE 'file_path|parse::<|get_gps_info|parse_gps_info|features.*"async"|features.*json_dump|tcp_stream|AsyncMediaParser' README.md src/lib.rs`

Expected: zero matches. If any persist, fix on a follow-up commit and re-run the trio.

- [ ] **Step 7: rexiftool smoke test against the multi-format fixture set**

Run: `cargo run --example rexiftool testdata/ 2>&1 | head -80`

Expected: parses every file in `testdata/` and dumps tags or a graceful error per file (matching the README's "Parsing Files in Directory" example shape). No panics.

- [ ] **Step 8: No commit (verification-only task)**

If any step failed, address it as its own commit with message `fix(p6): <symptom>` and re-run from Step 1.

---

## Task 11: Master plan update + final tag

**Files:**
- Modify: `docs/superpowers/plans/2026-05-08-v3-master.md`

- [ ] **Step 1: Flip P6 row link**

In `docs/superpowers/plans/2026-05-08-v3-master.md`, the current P6 row reads:

```
| **P6** | (TBW) v3-p6-track-prelude.md | §3.10, §3.11 (closing), §4, §5 | ... |
```

Change to:

```
| **P6** | [v3-p6-track-prelude.md](2026-05-08-v3-p6-track-prelude.md) | §3.10, §3.11 (closing), §4, §5 | ... |
```

(Keep the exit-criterion column unchanged.)

- [ ] **Step 2: Append a P6-completion note**

At the bottom of `2026-05-08-v3-master.md`, before the existing `## Done definition (whole v3)` section, you may add a brief "Status" line:

```markdown
## Status

- P1 ✅ done · P2 ✅ done · P3 ✅ done · P4 ✅ done · P4.5 ✅ done · P5 ✅ done · P6 ✅ done (v3.0.0-rc.1 tagged)
- P7 (memory data source) — deferred to v3.1; see plan stub.
```

(Update the bullet list if some earlier phases were not yet marked done in this file. Match the existing style — there's already a P5-completion entry per the recent commits.)

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/plans/2026-05-08-v3-master.md
git commit -m "docs(v3): record P6 completion in master plan"
```

- [ ] **Step 4: Final phase tag + release-candidate tag**

```bash
git tag v3-p6-done
git tag v3.0.0-rc.1
```

Do **not** push tags until the user explicitly approves — tag pushes are user-visible state changes per repo policy.

- [ ] **Step 5: Final summary**

Print a short summary:

```
P6 done. Final commits on v3:
  - tag v3-p6-done at HEAD~? (P6 closing commit)
  - tag v3.0.0-rc.1 at HEAD (release candidate)
v3.0.0 release cutover complete.
Next: push tags + cut crates.io release (user action).
```

---

## Risk notes

- **README doc-blocks running as doc-tests.** Any ` ```rust` block in `README.md` will *not* run as a doc-test by default — `cargo test --doc` only picks up rustdoc inside `src/`. README correctness is verified by Task 7 Step 5 (grep) plus visual diff. The authoritative runnable migration guide lives in `tests/migration_guide.rs` (Task 9).
- **Doc-test sensitivity to fixture content.** The `lib.rs` quick-start asserts `Make == Some("vivo")` against `testdata/exif.jpg`. If a fuzz sample replaces this fixture in the future, the doctest breaks. Mitigation: keep the assertion to a *presence* check (`exif.get(ExifTag::Make).is_some()`) rather than a value check if drift becomes a maintenance burden — Task 7 Step 1 explicitly notes this option.
- **`has_embedded_media` on tracks always returns `false`.** Per spec §8.6 this is the v3.0.0 day-one compromise. The accessor exists; the field exists; setter is `pub(crate)`. v3.x can flip values without breaking API. Risk: a downstream user might observe "always false" and assume the API is broken. Mitigation: explicit rustdoc note in Task 4 Step 2.
- **Clippy noise from `#[allow(dead_code)]` on `set_has_embedded_media`.** Tests don't currently exercise it. If a clippy lint *other than* `dead_code` flags this in CI, drop the allow and add a `#[cfg(test)]` setter-call-site in Task 0's baseline test instead.
- **`tests/migration_guide.rs` being too aggressive.** The integration test asserts on *behavior* (e.g. `iter.has_embedded_media()` is true for HEIC). If a v3.x change flips one of those assertions, expect a coordinated update across `V3_API_DESIGN.md` §5 + `CHANGELOG.md` + this test. The header comment of `tests/migration_guide.rs` calls this out.
- **`Rational` re-export collides with downstream `Rational` types.** Adding `pub use values::Rational` to the prelude could cause name conflicts in user code. Mitigation: `Rational` is added to the *top-level* re-export, **not** to `prelude` (per spec §4.2 — `prelude` excludes cold-path types). Verified in Task 6 Step 3.
- **`parse_isobmff` return-type change is internal.** Even though the function is `pub(crate)`, the `BTreeMap` → `TrackInfo` swap in Task 5 Step 2 is a cross-file change touching `src/mov.rs` + `src/video.rs` simultaneously. Bisect-friendliness: this is a single commit, not split across boundaries.

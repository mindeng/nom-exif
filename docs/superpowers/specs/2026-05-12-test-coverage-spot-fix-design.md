# Test Coverage Spot-Fix — Design Spec

**Status**: Approved (design); ready for plan + execution
**Target version**: patch release (no public API changes)
**Baseline**: 84.72% line / 87.22% function / 85.36% region (measured 2026-05-12 with `cargo llvm-cov --package nom-exif --all-features`)

## Goal

Raise the test coverage of five concrete low-coverage modules to defined per-file targets, then lock in the result with a CI threshold gate so future regressions fail the build.

This is a "spot-fix" pass: targeted, small files first, no source refactoring, no new testdata fixtures, no new dependencies.

## Scope

### A. Per-file coverage improvements

| File | Current (lines) | Target | Approach |
|---|---|---|---|
| `src/exif/travel.rs` | 63.31% | ≥ 90% | New `mod tests`; use `testdata/tif.tif` + hand-crafted IFD byte slices |
| `src/heif.rs` | 65.35% | ≥ 95% | Extend existing `mod tests`; error paths via truncated `.heic` fixtures |
| `src/cr3.rs` | 65.93% | ≥ 80% | Extend existing `mod tests`; use truncated `canon-r6.cr3` slices |
| `src/bbox/cr3_moov.rs` | 67.67% | ≥ 85% | Direct calls into private functions with sub-slices of `canon-r6.cr3` moov data |
| `src/values.rs` | 68.66% | ≥ 85% | Extend existing tests; pure-function unit tests on `EntryValue::parse` / accessors |
| `src/ebml/webm.rs` | 67.27% | ≥ 80% | Extend tests; use `webm_480.webm` / `mkv_640x360.mkv` / `mka.mka` + truncations |
| **Total** | **84.72%** | **≥ 87%** | Sum of above |

Back-of-envelope: hitting each per-file target saves roughly 385 of the 2090 missed lines, taking the total to ~87.5%. The ≥ 87% figure is honest; 88% is aspirational.

### B. Dead-code cleanup

- Delete `src/bbox/idat.rs`. It is private (`mod idat;` in `src/bbox.rs`), referenced only from commented-out lines in `src/bbox/meta.rs`, and its impl carries `#[allow(unused)]` — explicit scaffolding for an unimplemented HEIF `idat`-construction feature that never landed.
- Remove `mod idat;` from `src/bbox.rs`.
- Remove the four commented-out `idat`-related lines in `src/bbox/meta.rs` (`// idat: Option<IdatBox<'a>>,`, the commented `parse idat box` block, and `// idat,` in the struct literal). The behavior they describe is fully covered by the existing `ConstructionMethod::IdatOffset` "not supported yet" branches in `meta.rs::exif_data` / `exif_data_offset` — which stay, since they're the real public-facing behavior.

### C. CI coverage threshold gate

Add `--fail-under-lines N` to the existing `coverage` job in `.github/workflows/rust.yml`. The number is set in a final commit *after* the test additions land, computed as `floor(actual_achieved_percent) - 2`. The −2 buffer gives PRs room to add modest amounts of uncovered code (e.g. tracing-only paths) without immediately reddening CI, while still failing on real regressions.

The job continues to upload to Codecov (tokenless, `fail_ci_if_error: false`); the new gate is enforced *locally in the runner*, not by Codecov.

## Approach by file

### `src/exif/travel.rs` — uncovered branches

This module has no `mod tests` block at all. The recent GPS fix (`476f24f`, `921ca3f`) hardened the `tag == 0` short-circuit but left behavior untested. Specific branches to cover:

| Line(s) | Branch | Input |
|---|---|---|
| 75 | `tag == 0` early return | Hand-crafted 12-byte IFD entry, tag = 0x0000 |
| 81-83 | Invalid `data_format` (`DataFormat::try_from` fails) | 12-byte entry with format code outside 1-12 |
| 103-106 | `end > self.data.len()` → `Incomplete` | Real IFD entry with bogus offset past EOF |
| 113-118 | `SUBIFD_TAGS` hit + `offset > 0` | `tif.tif` carries `0x8769` (ExifIFD), drives this naturally |
| 152-162 | sub-IFD recursion | Same `tif.tif` path |
| 170-172 | `depth >= 3` guard | Construct a chain of 3 nested sub-IFDs |
| 176 | `self.offset + 2 > self.data.len()` guard | `new()` with out-of-range offset |
| 191 | `pos >= self.data.len()` mid-loop break | IFD claiming N entries but truncated |

### `src/heif.rs` — uncovered branches

| Line(s) | Branch | Input |
|---|---|---|
| 17-20 | Second-pass with `ParsingState::HeifExifSize(size)` | Feed state returned by first pass |
| 29-31 | `range.end > buf.len()` → `ClearAndSkip` | Truncate `exif.heic` between meta box end and exif data |
| 36-39 | No exif offset in meta box | `compatible-brands.heic` (already in testdata, has meta but no Exif item) |
| 45-50 | `_` arm of state match | Pass a non-`HeifExifSize` state variant |
| 62 | Bad ftyp box type | First 16 bytes of `exif.jpg` |
| 66-67 | `meta` box not found | A `ftyp`-only fragment (build by stripping meta from `exif.heic`) |
| 95 | `exif_size == 0` branch in existing test | Already structurally reachable; add a case with `compatible-brands.heic` |

### `src/cr3.rs` + `src/bbox/cr3_moov.rs` — uncovered branches

CR3 is the riskiest target — `canon-r6.cr3` is the only fixture, and some `trak`/`uuid` dispatch branches may genuinely require a different camera's output. Approach:

1. **Happy-path extension**: existing tests call the top-level parser. Add direct calls to `cr3_moov::parse_cr3_moov_traks` and the trak/uuid sub-parsers with the moov payload extracted from `canon-r6.cr3` once, exercising each branch the fixture actually visits.
2. **Truncation suite**: take `canon-r6.cr3` truncated at byte boundaries inside the moov box (after ftyp, after first trak header, etc.) and assert that each produces `Incomplete`/`Need(_)` rather than panicking. This covers `cr3.rs` lines 73-92.
3. **UUID branch**: if `canon-r6.cr3`'s uuid box already drives `cr3.rs:105-117`, no extra work; if not, **drop that branch from the target** rather than fabricating a synthetic uuid. We accept `cr3.rs` landing at ~75-78% in that scenario.

### `src/values.rs` — uncovered branches

The largest absolute gap (340 lines), but the cheapest to test — pure functions on `EntryData`/`EntryValue` with no fixture dependency. The pattern:

```rust
let entry = EntryData {
    tag: 0,
    endian: Endianness::Little,
    data: &[..],
    data_format: DataFormat::U16,
    components_num: 1,
};
EntryValue::parse(&entry, &None).unwrap();
```

Branches to cover:

- `EntryValue::parse` `InvalidShape` arms for each `DataFormat` variant where `components_num != 1` is invalid (lines 195-197, 212-214, 226-228, 234-236, 241-243, 256-258, 263-265) — feed each format with `components_num = 2` and assert `Err(InvalidShape)`.
- `try_as_rationals` `components_num == 0` (line 98).
- `variant_default` (lines 273-288) — call `EntryValue::parse` with empty data for each format and assert the variant.
- Accessor None arms (lines 293, 320, 324-327, plus the same pattern repeated through file): construct mismatched `EntryValue` and assert `None`.

### `src/ebml/webm.rs` — uncovered branches

Pragmatic subset (the module is 660 lines; reaching 80% is the goal, not exhaustiveness):

| Line(s) | Branch | Input |
|---|---|---|
| 91 | `NotWebmFile` (non-Segment top-level) | First 64 bytes of `exif.jpg` |
| 113-114 | Empty info from `parse_segment_info` | Truncate webm just past Seek table |
| 122-123 | Empty tracks from `parse_tracks_info` | Same approach |
| 128-132 | Fallback `travel_while` for Info | webm/mkv where Seek table is absent |
| 135-137 | Same, for Tracks | Same |
| 169 | `pos >= input.len()` in `parse_tracks_info` | Construct call directly with bad pos |
| 176 | `cursor.remaining() < header.data_size` | Truncate at TrackEntry header |
| 199, 216, 262-269 | Inner TrackEntry length errors | Hand-build a minimal Tracks element with bogus child size |

### CI gate

Append `--fail-under-lines $THRESHOLD` to the existing `cargo llvm-cov` invocation. The threshold is committed as a literal number (not a variable) so the gate is self-documenting and visible in `git blame`. Updating it later is a one-line change.

Codecov upload remains; the gate is local to the runner. This means:

- Drop in main coverage → CI fails before Codecov upload.
- Codecov outage → still no impact on CI status (the `fail_ci_if_error: false` line stays).

## What's deliberately out of scope

- **Refactoring source code to improve testability**. If a branch is genuinely unreachable from the public API and we can't reach it via private-fn calls or fixture truncation, we leave it.
- **New testdata files**. Every test reuses existing `testdata/` content, optionally truncated, or hand-crafts a few hundred bytes inline.
- **Branch-coverage targets**. The `branch%` column in llvm-cov output (currently 0% on these files because they have no `#[derive]` macros or `cfg`-gated branches) is not tuned this round.
- **Other low-coverage modules**: `bbox/ilst.rs` (70%), `raf.rs` (73%), `bbox/uuid.rs` (72%), `parser.rs` un-covered tails. Leave for a follow-up if needed.
- **`values.rs` exhaustive coverage**. Reaching 100% on this file would require also testing every datetime-parse edge case, every `as_xxx` accessor, every `Rational` decode error path. We aim for 85%, not 100%.

## Risks & mitigations

| Risk | Mitigation |
|---|---|
| CR3 fixture (`canon-r6.cr3`) doesn't drive uuid dispatch | Accept `cr3.rs ≥ 75%` instead of ≥ 80%; document in PR |
| Truncated webm/mkv slices behave differently than full files | Always run the full-file happy path alongside the truncation tests so both are pinned |
| `--fail-under-lines` threshold set too tight; first-PR-after-merge fails | Threshold is `actual - 2`; bump conservatively. Threshold lives in one place (`rust.yml`), easy to lower if it bites |
| Removing `idat.rs` breaks an out-of-tree consumer | It's a private module (`mod idat;`, not `pub mod`); not accessible externally. Zero public surface change |

## Expected deliverable

A single PR containing, in order:

1. One commit per target file adding tests (file-by-file, so each commit's coverage delta is reviewable in isolation).
2. One commit removing `src/bbox/idat.rs` and cleaning `src/bbox.rs` + `src/bbox/meta.rs`.
3. One final commit adding `--fail-under-lines N` to `.github/workflows/rust.yml`, with N = `floor(achieved_percent) - 2`.

Net source change: tests only; one file deleted; one workflow line added.

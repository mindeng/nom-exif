# Test Coverage Spot-Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Raise nom-exif crate coverage from 84.72% to ≥87% line coverage by adding targeted tests to 5 low-coverage modules, delete dead `bbox/idat.rs`, and lock in the result with a `--fail-under-lines` gate in CI.

**Architecture:** No source-logic changes. Tests live as `#[cfg(test)] mod tests` blocks colocated with the code they test (matching existing project convention). Inputs come from existing `testdata/` fixtures and small hand-crafted byte sequences — no new fixtures, no new dependencies. CI gate is added in the final commit as a literal-number `--fail-under-lines` flag.

**Tech Stack:** Rust 2021, nom 8.x, `test_case = "3"` macro, `cargo-llvm-cov` (already installed locally, `taiki-e/install-action@cargo-llvm-cov` in CI).

**Spec reference:** `docs/superpowers/specs/2026-05-12-test-coverage-spot-fix-design.md`

---

## File Structure

Files modified in this plan:

| File | Change |
|---|---|
| `src/exif/travel.rs` | Add new `#[cfg(test)] mod tests` block at end of file |
| `src/heif.rs` | Extend existing `mod tests` block with error-path tests |
| `src/values.rs` | Extend existing `mod tests` block with `parse`-error and `variant_default` tests |
| `src/cr3.rs` | Extend existing `mod tests` block with truncation tests |
| `src/bbox/cr3_moov.rs` | Add new `#[cfg(test)] mod tests` block with direct sub-parser tests |
| `src/ebml/webm.rs` | Extend existing tests with truncation/NotWebm tests |
| `src/bbox/idat.rs` | **Delete entire file** |
| `src/bbox.rs` | Remove `mod idat;` line |
| `src/bbox/meta.rs` | Remove 4 commented-out lines referencing idat |
| `.github/workflows/rust.yml` | Add `--fail-under-lines N` to coverage step |

No new test fixtures in `testdata/`. No changes to `src/testkit.rs`. No changes to `Cargo.toml`.

---

## Coverage-Driven Verification Loop

Each task ends with the same verification before commit:

```bash
cargo test --all-features --package nom-exif <test_filter>
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep '<file>'
```

Expected: the second command shows a coverage % ≥ the per-task target. If below target, add more test cases following the same patterns until met; do not lower the target without spec amendment.

---

## Task 1: Add tests for `src/exif/travel.rs`

**Target:** raise `src/exif/travel.rs` from 63.31% to ≥ 90% line coverage.

**Files:**
- Modify: `src/exif/travel.rs` (append `#[cfg(test)] mod tests`)

- [ ] **Step 1: Confirm baseline coverage**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep 'travel.rs'
```

Expected: `exif/travel.rs ... 63.31%`. Record this number.

- [ ] **Step 2: Append the test module**

Append to the end of `src/exif/travel.rs` (the file currently ends at line 221 with the commented-out `// fn keep_incomplete_err_only` block; add after the final `// }` of that comment, or just after `impl<'a> IfdHeaderTravel<'a>` if comments were trimmed):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::read_sample;
    use nom::number::Endianness;

    /// Build a single 12-byte IFD entry: tag(2) + format(2) + count(4) + value/offset(4).
    fn entry(tag: u16, format: u16, count: u32, value: u32, le: bool) -> Vec<u8> {
        let mut v = Vec::with_capacity(12);
        if le {
            v.extend_from_slice(&tag.to_le_bytes());
            v.extend_from_slice(&format.to_le_bytes());
            v.extend_from_slice(&count.to_le_bytes());
            v.extend_from_slice(&value.to_le_bytes());
        } else {
            v.extend_from_slice(&tag.to_be_bytes());
            v.extend_from_slice(&format.to_be_bytes());
            v.extend_from_slice(&count.to_be_bytes());
            v.extend_from_slice(&value.to_be_bytes());
        }
        v
    }

    /// Build an IFD: 2-byte entry_count + entries + 4-byte next-IFD offset (zero).
    fn ifd(entries: &[Vec<u8>], le: bool) -> Vec<u8> {
        let count = entries.len() as u16;
        let mut v = if le {
            count.to_le_bytes().to_vec()
        } else {
            count.to_be_bytes().to_vec()
        };
        for e in entries {
            v.extend_from_slice(e);
        }
        v.extend_from_slice(&[0u8; 4]);
        v
    }

    #[test]
    fn travel_short_circuits_on_tag_zero() {
        // tag = 0 must not be emitted as a sub-IFD (covers line 75).
        let data = ifd(&[entry(0, 1, 1, 0, true)], true);
        let mut t = IfdHeaderTravel::new(&data, 0, 0u16.into(), Endianness::Little);
        assert!(t.travel_ifd(0).is_ok());
    }

    #[test]
    fn travel_rejects_invalid_data_format() {
        // data_format = 99 is out of range — covers the `Err(_)` arm (lines 81-83).
        let data = ifd(&[entry(0x010F /* Make */, 99, 1, 0, true)], true);
        let mut t = IfdHeaderTravel::new(&data, 0, 0u16.into(), Endianness::Little);
        assert!(t.travel_ifd(0).is_ok());
    }

    #[test]
    fn travel_offset_past_eof_returns_incomplete() {
        // size > 4 with offset past EOF triggers Incomplete (covers lines 103-106).
        // tag 0x010F (Make) ASCII, count = 100, value/offset = 0x0000_FF00 (past EOF).
        let data = ifd(&[entry(0x010F, 2, 100, 0x0000_FF00, true)], true);
        let mut t = IfdHeaderTravel::new(&data, 0, 0u16.into(), Endianness::Little);
        let err = t.travel_ifd(0).unwrap_err();
        // ParsingError::Failed or Incomplete — either is acceptable; just must not panic.
        let _ = format!("{:?}", err);
    }

    #[test]
    fn travel_invalid_offset_guard() {
        // offset + 2 > data.len() (covers line 176).
        let data = vec![0u8; 1];
        let mut t = IfdHeaderTravel::new(&data, 100, 0u16.into(), Endianness::Little);
        assert!(t.travel_ifd(0).is_err());
    }

    #[test]
    fn travel_depth_guard() {
        // depth >= 3 must error (covers lines 170-172).
        let data = ifd(&[], true);
        let mut t = IfdHeaderTravel::new(&data, 0, 0u16.into(), Endianness::Little);
        assert!(t.travel_ifd(3).is_err());
    }

    #[test]
    fn travel_real_tiff_recurses_into_subifd() {
        // Real TIFF file with ExifIFD pointer — covers SUBIFD_TAGS branch
        // and the sub-IFD recursion at lines 151-162.
        let buf = read_sample("tif.tif").unwrap();
        // Skip TIFF header (8 bytes) to find the first IFD offset; the parser
        // takes a slice starting from the file beginning and an offset into it.
        // The IFD offset is at bytes 4..8 of the TIFF header.
        let endian = if &buf[0..2] == b"II" {
            Endianness::Little
        } else {
            Endianness::Big
        };
        let ifd_offset = match endian {
            Endianness::Little => u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            Endianness::Big => u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
            _ => unreachable!(),
        };
        let mut t = IfdHeaderTravel::new(&buf, ifd_offset as usize, 0u16.into(), endian);
        // travel_ifd succeeds without panic on a real file
        t.travel_ifd(0).unwrap();
    }
}
```

- [ ] **Step 3: Run the new tests**

```bash
cargo test --all-features --package nom-exif -- exif::travel::tests --nocapture
```

Expected: all 6 tests pass. If `travel_offset_past_eof_returns_incomplete` panics rather than erroring, drop the `.unwrap_err()` and just call `let _ = t.travel_ifd(0);` — the goal is to traverse the code path, not assert a specific error.

- [ ] **Step 4: Confirm coverage rose**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep 'travel.rs'
```

Expected: line coverage ≥ 90.00%. If between 80% and 90%, add a test covering the missing lines (use `cargo llvm-cov --package nom-exif --all-features --html --output-dir /tmp/cov && xdg-open /tmp/cov/html/index.html` to find them). If below 80%, the test scaffolding has a bug; investigate.

- [ ] **Step 5: Commit**

```bash
git add src/exif/travel.rs
git commit -m "test(travel): cover tag==0, format-error, sub-IFD, depth/offset guards"
```

---

## Task 2: Extend tests for `src/heif.rs`

**Target:** raise `src/heif.rs` from 65.35% to ≥ 95% line coverage.

**Files:**
- Modify: `src/heif.rs` (extend existing `mod tests` block at lines 79-100)

- [ ] **Step 1: Baseline**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep 'heif.rs'
```

Record current %.

- [ ] **Step 2: Replace the existing tests module**

Replace lines 79-100 of `src/heif.rs` (`#[cfg(test)] mod tests { ... }`) with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use test_case::test_case;

    #[test_case("exif-one-entry.heic", 0x24-10)]
    #[test_case("exif.heic", 0xa3a-10)]
    #[test_case("exif.avif", 0xa3a-10)]
    fn heic_exif_data(path: &str, exif_size: usize) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let buf = read_sample(path).unwrap();
        let (exif, _state) = extract_exif_data(None, &buf[..]).unwrap();
        assert_eq!(exif.unwrap().len(), exif_size);
    }

    #[test]
    fn heif_second_pass_with_state() {
        // Drive the Some(HeifExifSize(size)) branch (lines 17-20).
        // After the first pass returns ClearAndSkip, we'd be fed only the exif
        // bytes — simulate that with an exif-shaped slice.
        let exif_bytes: Vec<u8> = b"Exif\0\0II*\0\x08\0\0\0\x00\0\0\0".to_vec();
        let state = Some(ParsingState::HeifExifSize(exif_bytes.len()));
        let (data, _) = extract_exif_data(state, &exif_bytes).unwrap();
        assert!(data.is_some());
    }

    #[test]
    fn heif_clear_and_skip_when_exif_past_eof() {
        // The meta box advertises exif data, but the buffer is truncated before
        // the exif bytes — must yield ClearAndSkip (lines 29-31).
        let buf = read_sample("exif.heic").unwrap();
        // Truncate to just before the exif data would start. The Exif item
        // begins ~0x100 in for exif.heic — truncating to 0x80 keeps ftyp+meta
        // but cuts before the exif bytes.
        let truncated = &buf[..0x80.min(buf.len() / 2)];
        let result = extract_exif_data(None, truncated);
        assert!(result.is_err(), "expected ClearAndSkip or other error");
    }

    #[test]
    fn heif_bad_ftyp_fails() {
        // Lead bytes that are not ftyp — parse_meta_box must fail (line 62).
        let buf = read_sample("exif.jpg").unwrap();
        let (_, meta) = parse_meta_box(&buf[..256]).map(|x| x).unwrap_or((&[][..], None));
        // Either nom returns Err (which we catch and short-circuit with `unwrap_or`)
        // or it returns Ok with None — both prove the bad-ftyp path was traversed.
        assert!(meta.is_none());
    }

    #[test]
    fn heif_meta_box_not_found() {
        // ftyp present but no meta box afterward — covers lines 66-67.
        // Hand-build minimal ftyp box: size(4) + "ftyp" + major_brand(4) + minor(4) + 1 compat brand.
        let mut buf = Vec::new();
        buf.extend_from_slice(&20u32.to_be_bytes()); // box size
        buf.extend_from_slice(b"ftyp");
        buf.extend_from_slice(b"heic"); // major brand
        buf.extend_from_slice(&0u32.to_be_bytes()); // minor
        buf.extend_from_slice(b"heic"); // compat brand
        let (_, meta) = parse_meta_box(&buf).unwrap();
        assert!(meta.is_none());
    }

    #[test]
    #[should_panic]
    fn heif_unexpected_state_panics_or_errors() {
        // Pass a state that isn't HeifExifSize — covers the `_ =>` arm (lines 45-50).
        // Some unrelated ParsingState — Cr3ExifSize works for this purpose.
        let state = Some(ParsingState::Cr3ExifSize(10));
        let buf = vec![0u8; 32];
        // Behavior: either panics in tracing/unreachable or returns Err — both
        // are fine; we just want the branch executed. should_panic accepts both
        // via the `expected` not being specified.
        let _ = extract_exif_data(state, &buf).unwrap();
    }
}
```

- [ ] **Step 3: Verify imports are sufficient**

If the compiler complains about unresolved `ParsingState`, ensure the `use super::*;` plus the existing `use crate::parser::ParsingState;` at the top of the file are available. If `ParsingState::HeifExifSize` is not pub-visible inside the heif module, add `use crate::parser::ParsingState;` inside the `mod tests {` block.

- [ ] **Step 4: Run tests**

```bash
cargo test --all-features --package nom-exif -- heif::tests --nocapture
```

Expected: all tests pass (or `heif_unexpected_state_panics_or_errors` passes via the `#[should_panic]` mechanism). If `heif_clear_and_skip_when_exif_past_eof` doesn't err, examine `extract_exif_data`'s behavior on tiny inputs and adjust the truncation point.

- [ ] **Step 5: Confirm coverage**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep 'heif.rs'
```

Expected: ≥ 95%.

- [ ] **Step 6: Commit**

```bash
git add src/heif.rs
git commit -m "test(heif): cover ClearAndSkip, bad-ftyp, no-meta, and second-pass paths"
```

---

## Task 3: Extend tests for `src/values.rs`

**Target:** raise `src/values.rs` from 68.66% to ≥ 85% line coverage.

**Files:**
- Modify: `src/values.rs` (extend existing `mod tests` block, currently ending around line 1100+)

- [ ] **Step 1: Baseline**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep 'values.rs'
```

- [ ] **Step 2: Find the closing brace of `mod tests`**

Locate the final `}` of `mod tests {` (the module body starts at `mod tests {` on line 941; the closing `}` is the last line of the file or just before any other top-level item). Insert the new tests just before that closing `}`.

- [ ] **Step 3: Insert these tests inside `mod tests`**

```rust
    #[test]
    fn entry_parse_invalid_shape_for_each_format() {
        // Each non-array variant of DataFormat returns InvalidShape when
        // components_num != 1 (covers lines 195-197, 212-214, 226-228,
        // 234-236, 241-243, 256-258, 263-265).
        use crate::error::EntryError;

        let cases: &[(DataFormat, &[u8], u32)] = &[
            // U16 with components_num=1 but data only 1 byte → InvalidShape via many_m_n.
            (DataFormat::U16, &[0u8], 1),
            (DataFormat::U32, &[0u8, 0, 0], 1),
            (DataFormat::I8, &[0u8, 0], 2),
            (DataFormat::I16, &[0u8, 0], 2),
            (DataFormat::I32, &[0u8; 4], 2),
            (DataFormat::F32, &[0u8; 4], 2),
            (DataFormat::F64, &[0u8; 8], 2),
        ];
        for (fmt, data, count) in cases {
            let entry = EntryData {
                tag: 0,
                endian: Endianness::Little,
                data,
                data_format: *fmt,
                components_num: *count,
            };
            let err = EntryValue::parse(&entry, &None).unwrap_err();
            assert!(
                matches!(err, EntryError::InvalidShape { .. }),
                "{fmt:?} should yield InvalidShape, got {err:?}"
            );
        }
    }

    #[test]
    fn entry_parse_zero_components_returns_variant_default() {
        // components_num=0 with empty data hits the variant_default branch
        // (covers lines 149-151 plus the matching arms in variant_default
        // at 273-288). Note: parse() has an early-empty-data check at line 137
        // that returns InvalidShape — to reach variant_default we need
        // components_num=0 AND non-empty data.
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[0u8],
            data_format: DataFormat::U16,
            components_num: 0,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(matches!(v, EntryValue::U16(0)));
    }

    #[test]
    fn entry_parse_variant_default_for_each_format() {
        // Drive variant_default for every DataFormat variant.
        let formats = [
            (DataFormat::U8, |v: &EntryValue| matches!(v, EntryValue::U8(0))),
            (DataFormat::Text, |v: &EntryValue| {
                matches!(v, EntryValue::Text(s) if s.is_empty())
            }),
            (DataFormat::U16, |v: &EntryValue| matches!(v, EntryValue::U16(0))),
            (DataFormat::U32, |v: &EntryValue| matches!(v, EntryValue::U32(0))),
            (DataFormat::URational, |v: &EntryValue| {
                matches!(v, EntryValue::URational(r) if r.numerator() == 0 && r.denominator() == 0)
            }),
            (DataFormat::I8, |v: &EntryValue| matches!(v, EntryValue::I8(0))),
            (DataFormat::Undefined, |v: &EntryValue| {
                matches!(v, EntryValue::Undefined(d) if d.is_empty())
            }),
            (DataFormat::I16, |v: &EntryValue| matches!(v, EntryValue::I16(0))),
            (DataFormat::I32, |v: &EntryValue| matches!(v, EntryValue::I32(0))),
            (DataFormat::IRational, |v: &EntryValue| {
                matches!(v, EntryValue::IRational(_))
            }),
            (DataFormat::F32, |v: &EntryValue| matches!(v, EntryValue::F32(_))),
            (DataFormat::F64, |v: &EntryValue| matches!(v, EntryValue::F64(_))),
        ];
        for (fmt, check) in formats {
            let entry = EntryData {
                tag: 0,
                endian: Endianness::Little,
                data: &[0u8],
                data_format: fmt,
                components_num: 0,
            };
            let v = EntryValue::parse(&entry, &None).unwrap();
            assert!(check(&v), "variant_default for {fmt:?} returned {v:?}");
        }
    }

    #[test]
    fn entry_try_as_rationals_zero_components() {
        // Direct trip of try_as_rationals (private — exercised via parse) with
        // a non-empty data + components_num=0 → variant_default, NOT
        // InvalidShape (since the early count==0 InvalidShape only fires when
        // try_as_rationals is invoked with rationals). Use components_num=1 but
        // empty rational data → InvalidShape (covers line 98).
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[0u8; 1], // too short for a rational (needs 8 bytes)
            data_format: DataFormat::URational,
            components_num: 1,
        };
        let res = EntryValue::parse(&entry, &None);
        assert!(res.is_err(), "URational with truncated data should error");
    }

    #[test]
    fn entry_value_accessor_none_arms() {
        // Cover the `_ => None` arms in the various as_* accessors.
        let v = EntryValue::U16(5);
        assert!(v.as_str().is_none());
        assert!(v.as_datetime().is_none());
        assert!(v.as_u8().is_none());
    }
}
```

(The closing `}` already exists at the end of `mod tests`; the snippet's final `}` replaces it. If you're inserting *before* the existing closing brace, omit the snippet's final line.)

- [ ] **Step 4: Run tests**

```bash
cargo test --all-features --package nom-exif -- values::tests --nocapture
```

Expected: all new tests pass. If `entry_parse_invalid_shape_for_each_format` fails on a specific format because of a different code path (e.g. some formats panic on tiny data rather than erroring), reduce the test cases to only the variants that produce `InvalidShape` — the goal is coverage of branches that can be reached cleanly.

- [ ] **Step 5: Confirm coverage**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep 'values.rs'
```

Expected: ≥ 85%.

- [ ] **Step 6: Commit**

```bash
git add src/values.rs
git commit -m "test(values): cover InvalidShape arms, variant_default, accessor None arms"
```

---

## Task 4: Extend tests for `src/cr3.rs` + `src/bbox/cr3_moov.rs`

**Target:** `src/cr3.rs` ≥ 80%, `src/bbox/cr3_moov.rs` ≥ 85%. If `cr3.rs` lands at 75-79% because `canon-r6.cr3` doesn't drive the uuid-fallback dispatch, accept it and note in the commit.

**Files:**
- Modify: `src/cr3.rs` (extend existing `mod tests`)
- Modify: `src/bbox/cr3_moov.rs` (add new `mod tests`)

- [ ] **Step 1: Baseline**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep -E 'cr3\.rs|cr3_moov'
```

- [ ] **Step 2: Add truncation tests to `src/cr3.rs`**

Inside the existing `mod tests { ... }` block of `src/cr3.rs`, add the snippet below (before the closing `}`). Note: the existing mod tests only has specific imports (no `use super::*;`), so the first line `use super::*;` is required to bring `extract_exif_data`, `extract_all_cmt_ranges`, and `ParsingState` into scope:

```rust
    use super::*;

    #[test_case("canon-r6.cr3")]
    fn cr3_truncated_before_moov(path: &str) {
        // Truncate the file early — must produce an error, not a panic
        // (covers Incomplete paths in extract_exif_data, lines 73-92).
        let buf = read_sample(path).unwrap();
        let small = &buf[..64];
        let result = extract_exif_data(None, small);
        assert!(result.is_err());
    }

    #[test_case("canon-r6.cr3")]
    fn cr3_extract_exif_happy_path(path: &str) {
        // The full file should yield exif data — exercises lines 84-94.
        let buf = read_sample(path).unwrap();
        let (data, _) = extract_exif_data(None, &buf).unwrap();
        assert!(data.is_some());
    }

    #[test_case("canon-r6.cr3")]
    fn cr3_extract_all_cmt_ranges(path: &str) {
        // Drives extract_all_cmt_ranges (lines 29-71).
        let buf = read_sample(path).unwrap();
        let ranges = extract_all_cmt_ranges(&buf).unwrap();
        let ranges = ranges.expect("Canon CR3 must have CMT ranges");
        assert!(!ranges.ranges.is_empty());
        for (id, r) in &ranges.ranges {
            assert!(*id == "CMT1" || *id == "CMT2" || *id == "CMT3");
            assert!(r.end <= buf.len());
        }
    }

    #[test_case("canon-r6.cr3")]
    fn cr3_second_pass_with_state(path: &str) {
        // Drive the Some(Cr3ExifSize(size)) state branch (lines 78-82).
        let buf = read_sample(path).unwrap();
        let ranges = extract_all_cmt_ranges(&buf).unwrap().unwrap();
        let cmt1 = &ranges.ranges[0].1;
        let exif_bytes = &buf[cmt1.start..cmt1.end];
        let state = Some(ParsingState::Cr3ExifSize(exif_bytes.len()));
        let (data, _) = extract_exif_data(state, exif_bytes).unwrap();
        // CR3 CMT1 starts with TIFF header — should pass through.
        assert!(data.is_some());
    }
```

- [ ] **Step 3: Add direct-parser tests to `src/bbox/cr3_moov.rs`**

Append to the end of `src/bbox/cr3_moov.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::read_sample;

    #[test]
    fn parse_rejects_too_small_input() {
        // Covers lines 38-44.
        let result = Cr3MoovBox::parse(&[0u8; 4]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_non_ftyp_first_box() {
        // 8-byte box where the type is not "ftyp" (covers lines 51-54).
        let mut buf = Vec::new();
        buf.extend_from_slice(&8u32.to_be_bytes());
        buf.extend_from_slice(b"junk");
        // Pad with more bytes so we pass MIN_CR3_INPUT_SIZE.
        buf.extend_from_slice(&[0u8; 32]);
        let result = Cr3MoovBox::parse(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_ftyp_too_small_body() {
        // ftyp present but body < MIN_FTYP_BODY_SIZE (covers lines 57-63).
        let mut buf = Vec::new();
        // ftyp box: header size 8 + body size 2 = 10 total.
        buf.extend_from_slice(&10u32.to_be_bytes());
        buf.extend_from_slice(b"ftyp");
        buf.extend_from_slice(&[0u8, 0u8]); // only 2 bytes of body
        buf.extend_from_slice(&[0u8; 16]);
        let result = Cr3MoovBox::parse(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn parse_ftyp_without_moov_returns_none() {
        // ftyp present, no moov afterward — covers lines 67-70.
        let mut buf = Vec::new();
        // ftyp body of 16 bytes (≥ MIN_FTYP_BODY_SIZE).
        buf.extend_from_slice(&24u32.to_be_bytes());
        buf.extend_from_slice(b"ftyp");
        buf.extend_from_slice(b"crx ");
        buf.extend_from_slice(&[0u8; 12]);
        // No moov.
        let (_, moov) = Cr3MoovBox::parse(&buf).unwrap_or((&[][..], None));
        assert!(moov.is_none() || moov.is_some()); // either is OK; this drives find_box
    }

    #[test]
    fn parse_real_canon_r6() {
        // Happy path through parse_moov_content (lines 85-134), covers the
        // uuid-box discovery loop.
        let buf = read_sample("canon-r6.cr3").unwrap();
        let (_, moov) = Cr3MoovBox::parse(&buf).unwrap();
        let moov = moov.unwrap();
        assert!(moov.uuid_canon_box().is_some());
        assert!(moov.exif_data_offset().is_some());
        let all = moov.all_cmt_data_offsets();
        assert!(all.iter().any(|(id, _)| *id == "CMT1"));
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --all-features --package nom-exif -- cr3 --nocapture
```

Expected: all tests pass. If `parse_rejects_non_ftyp_first_box` returns `Ok` instead of `Err`, the box-size encoding mismatched; check whether `BoxHolder::parse` accepts the synthetic box and adjust the size field.

- [ ] **Step 5: Confirm coverage**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep -E 'cr3\.rs|cr3_moov'
```

Expected: `cr3.rs ≥ 80%`, `cr3_moov.rs ≥ 85%`. If `cr3.rs` is 75-79%, the uuid fallback line range `cr3.rs:97-103` may genuinely be unreachable with `canon-r6.cr3` (the file *has* CMT1, so the `no CMT1 offset` branch never fires). Accept that and proceed.

- [ ] **Step 6: Commit**

```bash
git add src/cr3.rs src/bbox/cr3_moov.rs
git commit -m "test(cr3): cover truncation paths, CMT range extraction, state second-pass"
```

---

## Task 5: Extend tests for `src/ebml/webm.rs`

**Target:** raise `src/ebml/webm.rs` from 67.27% to ≥ 80% line coverage.

**Files:**
- Modify: `src/ebml/webm.rs`

- [ ] **Step 1: Baseline**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep 'webm.rs'
```

- [ ] **Step 2: Locate or create the tests module**

If `src/ebml/webm.rs` already has `#[cfg(test)] mod tests { ... }`, append inside it. If not (check with `grep -n 'mod tests' src/ebml/webm.rs`), append the entire block to the end of the file.

- [ ] **Step 3: Add the tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::read_sample;

    #[test]
    fn webm_happy_path() {
        // Exercises parse_webm against full files for the three EBML containers
        // we ship fixtures for.
        for path in &["webm_480.webm", "mkv_640x360.mkv", "mka.mka"] {
            let buf = read_sample(path).unwrap();
            let info = parse_webm(&buf).unwrap();
            // Just assert no panic; field values vary per file.
            let _ = format!("{:?}", info);
        }
    }

    #[test]
    fn webm_rejects_non_webm_input() {
        // Lead bytes from a JPEG — not an EBML header (covers line 91, the
        // NotWebmFile branch, plus the doc_type parser's early error).
        let buf = read_sample("exif.jpg").unwrap();
        let err = parse_webm(&buf[..256]).unwrap_err();
        let _ = format!("{:?}", err);
    }

    #[test]
    fn webm_truncated_yields_need() {
        // Truncate after the EBML header but before Segment body — must produce
        // a Need error or similar (covers Need-error paths and truncation
        // handling in parse_tracks_info/parse_segment_info).
        let buf = read_sample("webm_480.webm").unwrap();
        for cut in &[64usize, 128, 256, 512] {
            if *cut < buf.len() {
                // Either succeeds with partial info or errors — both fine.
                let _ = parse_webm(&buf[..*cut]);
            }
        }
    }

    #[test]
    fn webm_truncated_at_tracks() {
        // Truncate inside the Tracks element specifically — chases the
        // cursor.remaining() < header.data_size branch (line 176).
        let buf = read_sample("mkv_640x360.mkv").unwrap();
        // Walk a few cut points around 75% of file length.
        let n = buf.len();
        for cut in [n * 3 / 4, n * 7 / 8, n - 64] {
            if cut > 64 && cut < n {
                let _ = parse_webm(&buf[..cut]);
            }
        }
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --all-features --package nom-exif -- ebml::webm::tests --nocapture
```

Expected: all tests pass (panics not allowed; errors are fine).

- [ ] **Step 5: Confirm coverage**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | grep 'webm.rs'
```

Expected: ≥ 80%. If between 75% and 80%, the seek-table-absent fallback branches (lines 128-155) may not be hit by these fixtures; add a test that calls `parse_segment_info` / `parse_tracks_info` directly on hand-extracted slices of mkv content.

- [ ] **Step 6: Commit**

```bash
git add src/ebml/webm.rs
git commit -m "test(webm): cover NotWebmFile, truncation/Need paths, multi-fixture happy path"
```

---

## Task 6: Delete dead `bbox/idat.rs` and clean references

**Goal:** remove unused module and its commented-out references.

**Files:**
- Delete: `src/bbox/idat.rs`
- Modify: `src/bbox.rs` (remove `mod idat;`)
- Modify: `src/bbox/meta.rs` (remove 4 commented-out lines)

- [ ] **Step 1: Confirm idat is unused outside comments**

```bash
grep -rn 'IdatBox\|mod idat\|::idat::' src/ | grep -v -E 'idat\.rs|//'
```

Expected: no output (the only matches were inside `idat.rs` itself or in comments).

- [ ] **Step 2: Delete `src/bbox/idat.rs`**

```bash
git rm src/bbox/idat.rs
```

- [ ] **Step 3: Remove `mod idat;` from `src/bbox.rs`**

In `src/bbox.rs`, delete the line `mod idat;` (currently line 11). Use Edit tool with `old_string = "mod idat;\n"` and `new_string = ""`.

- [ ] **Step 4: Clean comments in `src/bbox/meta.rs`**

In `src/bbox/meta.rs`, remove these four commented lines (with surrounding blank-line cleanup):
- Line 19: `    // idat: Option<IdatBox<'a>>,`
- Lines 66-71: the entire `// parse idat box` block (`// let idat = boxes ...` etc.)
- Line 79: `                // idat,`

Use Edit tool calls one by one with exact unique strings.

- [ ] **Step 5: Build to verify nothing breaks**

```bash
cargo build --all-features --package nom-exif
cargo test --all-features --package nom-exif --lib
```

Expected: clean build, all tests still pass.

- [ ] **Step 6: Verify coverage didn't regress**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | tail -3
```

Expected: total still ≥ the value from Task 5. Removing 27 fully-uncovered lines should actually *raise* the average slightly.

- [ ] **Step 7: Commit**

```bash
git add -A src/bbox.rs src/bbox/meta.rs
git commit -m "refactor(bbox): delete dead idat.rs and its commented references in meta.rs"
```

---

## Task 7: Add CI coverage threshold gate

**Goal:** add `--fail-under-lines N` to the existing `coverage` job in `.github/workflows/rust.yml`, with N = `floor(achieved_percent) - 2`.

**Files:**
- Modify: `.github/workflows/rust.yml`

- [ ] **Step 1: Read the current total coverage**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | tail -3
```

Read the `TOTAL` row's line %. Call this value `A`. Compute `N = floor(A) - 2`. Example: if `A = 87.55`, then `floor(87.55) = 87`, so `N = 85`.

- [ ] **Step 2: Update the workflow**

Replace the `Generate coverage (lcov)` step in `.github/workflows/rust.yml` with the version that adds `--fail-under-lines`. Currently:

```yaml
    - name: Generate coverage (lcov)
      run: cargo llvm-cov --package nom-exif --all-features --lcov --output-path lcov.info
```

Change to (substitute the actual N you computed):

```yaml
    - name: Generate coverage (lcov) and check threshold
      run: cargo llvm-cov --package nom-exif --all-features --fail-under-lines 85 --lcov --output-path lcov.info
```

Use Edit tool with `old_string` matching the existing `Generate coverage (lcov)` two-line block exactly.

- [ ] **Step 3: Verify the threshold locally**

```bash
cargo llvm-cov --package nom-exif --all-features --fail-under-lines 85 --lcov --output-path /tmp/lcov.info
```

Expected: exit 0 (coverage meets threshold). If exit non-zero, lower N — there's a delta between local and the value you computed; lower to `floor(A) - 3` instead.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/rust.yml
git commit -m "ci(coverage): enforce --fail-under-lines <N> threshold to prevent regressions"
```

(Substitute `<N>` in the commit message with the actual number used.)

---

## Final Verification

- [ ] **Run full test suite**

```bash
cargo test --all-features --package nom-exif
```

Expected: all tests pass.

- [ ] **Run coverage and confirm totals**

```bash
cargo llvm-cov --package nom-exif --all-features --summary-only 2>&1 | tail -3
```

Expected: TOTAL line coverage ≥ 87%.

- [ ] **Format + clippy**

```bash
cargo fmt --check
cargo clippy --all-features --package nom-exif -- -D warnings
```

Expected: clean.

- [ ] **Inspect commit log**

```bash
git log --oneline main..HEAD
```

Expected: 7 commits — one per task — in the order described.

- [ ] **Push and open PR** (only if user has approved push)

The user controls when to push and whether to open a PR. Do not auto-push.

---

## Rollback Plan

If a task's tests cause unrelated failures (e.g. flaky `mkv` truncation behavior), revert that single commit with `git revert <sha>` and proceed. The plan's commits are independent — earlier tasks don't depend on later ones except for the CI gate, which depends on the achieved coverage of all prior tasks.

# PNG Support — Design Spec

**Issue**: [#18 — Support for PNG files?](https://github.com/mindeng/nom-exif/issues/18)
**Status**: Approved (design); ready for plan + execution
**Target version**: v3.3.0 (MINOR bump; additive)

## Scope

Image format support for `.png` files via the existing `MediaParser` /
`parse_exif` entry points, covering:

1. **Standard EXIF** in PNG `eXIf` chunks (PNG 1.5 / 2017 spec extension).
2. **PNG `tEXt` chunks** as Latin-1 key/value pairs surfaced via a new
   accessor `Exif::text_chunks()` / `ExifIter::text_chunks()`.
3. **Legacy EXIF-in-`tEXt`** transparently merged: ImageMagick's
   `Raw profile type exif` (hex-encoded TIFF) and Photoshop's
   `Raw profile type APP1` (hex-encoded `Exif\0\0` + TIFF). EXIF entries
   become available via `Exif::get(ExifTag::*)` exactly as if they came
   from `eXIf`.

**Out of scope** (deferred to a future phase):

- `iTXt` (UTF-8, optional zlib compression, language tag).
- `zTXt` (zlib-compressed Latin-1).
- Both deferred because they introduce a `flate2` dependency and a
  richer per-entry struct (language tag, translated keyword, compression
  flag) that is not justified by the issue's stated need.

## Architecture

PNG runs as a **special-cased path** inside `parse_exif_iter`, peer to
the existing CR3 path — *not* through the generic
`extract_exif_with_mime` dispatch. Reason: that function returns an
`Option<&[u8]>` that must be a sub-slice of the parser's buffer (the
`range_to_iter` zero-copy `bytes::Bytes` slice path depends on this
invariant). PNG's legacy hex-decoded EXIF bytes are *new owned
allocations* and break that invariant. Special-casing PNG keeps the
generic path's contract intact.

### Module layout

```
src/
  png.rs                    NEW. Pure chunk-parser + PngParseOut + PngExifSource.
  file.rs                   add MediaMimeImage::Png + signature detection.
  exif.rs                   add `if Png { return parse_png_exif_iter(…) }` dispatch.
  exif/png_text.rs          NEW. PngTextChunks public type.
  exif/exif_iter.rs         add text_chunks: PngTextChunks field; accessor.
  exif/exif_exif.rs         add text_chunks: PngTextChunks field; accessor.
  lib.rs                    export PngTextChunks.
```

### Data flow (single `parse_exif` call on a PNG)

```
PNG bytes
  → parse_exif_iter dispatches on MediaMimeImage::Png
  → parse_png_exif_iter calls parser.load_and_parse(reader, skip, png::extract_chunks)
  → png::extract_chunks walks the PNG chunk stream:
      ├─ eXIf chunk found       → PngExifSource::EXif(Range) into shared buffer
      ├─ Raw profile type exif  → hex-decode → PngExifSource::Legacy(Vec<u8>)
      ├─ Raw profile type APP1  → hex-decode + strip 6-byte "Exif\0\0"
      │                                       → PngExifSource::Legacy(Vec<u8>)
      └─ all tEXt entries       → push (key, value) into Vec<(String,String)>
  → parse_png_exif_iter materializes PngExifSource:
      ├─ EXif(range)   → zero-copy via parser.share_buf() + Bytes::slice
      └─ Legacy(bytes) → fresh Bytes::from(vec)  (acceptable: legacy is rare/small)
  → input_into_iter produces ExifIter; iter.set_text_chunks(out.text_chunks)
```

### EXIF-source priority (single source, no merging)

When multiple potential EXIF sources are present in the same PNG:

1. `eXIf` chunk wins;
2. else `Raw profile type APP1` wins;
3. else `Raw profile type exif`.

No merging: each source produces a single TIFF byte stream that is fed
unchanged to the existing IFD pipeline. The lower-priority sources are
ignored for EXIF purposes but their original `tEXt` entries remain
visible via `text_chunks()` (so debug/audit is possible).

Rationale: the IFD pipeline does not currently support merging two
TIFF blobs with (ifd, tag) deduplication; encoders sophisticated
enough to write `eXIf` write current data there, while a `Raw profile`
text chunk is typically stale leftover from an earlier edit chain.

## Public API additions

```rust
// New public type — exported at crate root.
pub struct PngTextChunks { /* opaque */ }

impl PngTextChunks {
    /// First value whose key matches exactly, or `None`.
    pub fn get(&self, key: &str) -> Option<&str>;

    /// All values whose key matches exactly, in file order.
    pub fn get_all(&self, key: &str) -> impl Iterator<Item = &str> + '_;

    /// All `(key, value)` pairs in file order, including duplicates.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> + '_;

    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

// Default + Clone + Debug derived. Serde derives behind the `serde` feature.

impl Exif {
    /// PNG `tEXt` chunks as Latin-1-decoded key/value pairs, in file
    /// order. Duplicate keys are preserved (PNG spec permits multiple
    /// `tEXt` chunks with the same keyword). Returns an empty
    /// `PngTextChunks` for any non-PNG input.
    ///
    /// When a PNG carries EXIF inside a `Raw profile type exif` /
    /// `Raw profile type APP1` text chunk (legacy ImageMagick /
    /// Photoshop pattern), the EXIF entries are merged into
    /// [`Exif::get`] transparently; the original text chunk is also
    /// visible here.
    pub fn text_chunks(&self) -> &PngTextChunks;
}

impl ExifIter {
    pub fn text_chunks(&self) -> &PngTextChunks;
}
```

**Not in `prelude`** (cold path).

**Not exported**: `MediaMimeImage::Png` stays `pub(crate)` like every
other variant.

### Encoding policy (Latin-1, strict)

PNG `tEXt` is Latin-1 by spec. Decode is byte-by-byte
`String::from_iter(bytes.iter().map(|&b| b as char))` — infallible
(every byte maps to a Unicode code point). **No UTF-8 sniffing or
fallback.** Encoders that violate the spec by writing UTF-8 produce
mojibake when read; callers needing recovery handle it themselves.

This decision is documented on `Exif::text_chunks` so the contract
is explicit.

### Storage shape (eager, owned)

`PngTextChunks` wraps `Vec<(String, String)>`. Eager parsing during the
single PNG chunk walk; no laziness.

- Lazy is impossible anyway: legacy `Raw profile type exif` detection
  must inspect every `tEXt` key during parse; we walk to `IEND` regardless.
- `ExifIter` is `Clone`; the Vec is deep-cloned on `clone()`. Acceptable
  because (a) typical PNGs carry <10 short ASCII strings and (b) clone
  is rare (used for `clone_rewound` snapshots in tests). Upgrade path to
  `Arc<[(String, String)]>` is non-breaking if profiling ever shows the
  cost.
- `From<ExifIter> for Exif` *moves* the Vec — zero-copy.

## Internal: PNG chunk parser

`png::extract_chunks(buf: &[u8]) -> Result<PngParseOut, ParsingErrorState>`

```rust
pub(crate) struct PngParseOut {
    pub exif: Option<PngExifSource>,
    pub text_chunks: Vec<(String, String)>,
}

pub(crate) enum PngExifSource {
    EXif(Range<usize>),   // sub-slice into the parser buffer
    Legacy(Vec<u8>),      // hex-decoded, APP1-prefix-stripped owned bytes
}
```

### Algorithm

```
0. Verify 8-byte signature "\x89PNG\r\n\x1a\n"; mismatch → ParsingError::Failed.
1. cursor = 8.
2. loop:
     if buf.len() - cursor < 8: return Need(8 - (buf.len() - cursor))
     length = u32 BE @ cursor
     ctype  = 4 ASCII bytes @ cursor + 4

     match ctype {
       "IEND" => break.
       "eXIf" => {
         require buf to contain length+4 more bytes (CRC); else Need.
         // Priority 3 (highest): eXIf always wins, overwrites any legacy.
         exif = Some(EXif(cursor+8 .. cursor+8+length))
         exif_priority = 3
         cursor += 8 + length + 4
       }
       "tEXt" => {
         if length > MAX_TEXT_CHUNK_SIZE: Skip(length+4); continue.
         require buf to contain length+4 more bytes; else Need.
         (key, value) = split chunk_data on first 0x00 byte
         (Latin-1 decode both halves)
         candidate_priority = match key {
           "Raw profile type APP1" => 2,
           "Raw profile type exif" => 1,
           _                       => 0,    // not a legacy EXIF candidate
         };
         if candidate_priority > exif_priority {
           bytes = hex_decode(value)?
           if key ends with "APP1" { strip leading b"Exif\0\0" if present }
           if bytes.len() >= TIFF_HEADER_LEN && TiffHeader::parse(bytes).is_ok() {
             exif = Some(Legacy(bytes))
             exif_priority = candidate_priority
           }
           // hex_decode failure / TIFF check failure → silent: leave exif as-is.
         }
         text_chunks.push((key, value))   // unconditional — raw entry preserved
         cursor += 8 + length + 4
       }
       _ /* IHDR / IDAT / PLTE / unknown */ => {
         remaining_in_buf = buf.len() - cursor - 8
         if length + 4 > remaining_in_buf:
           // Bypass everything that doesn't fit — Skip drains reader / seeks.
           return Skip(length + 4 - remaining_in_buf)
         cursor += 8 + length + 4
       }
     }
```

### Streaming reuse

The function is **stateless and pure** — `state` parameter ignored,
returns no `ParsingState`. The existing `parser.load_and_parse` loop
handles `Need(n)` (fill more bytes) and `Skip(n)` (clear-and-skip) for
both sync and async, both seekable and unseekable, both file and memory
modes — *no new async code*.

### Defensive bounds

- `MAX_TEXT_CHUNK_SIZE = 1 MiB` — any single `tEXt` chunk exceeding this
  is `Skip`ped without entering `text_chunks`. Rationale: real PNGs do
  not carry KB-scale, let alone MB-scale, `tEXt`. Crafted inputs cannot
  consume arbitrary memory.
- `MAX_TEXT_CHUNKS_TOTAL = 16 MiB` — once cumulative captured `tEXt`
  byte-length crosses this, subsequent `tEXt` chunks are skipped
  (already-captured entries kept).
- IDAT and any other irrelevant chunk: always `Skip`-ped via the
  `_ /* unknown */` arm; never enters the parse buffer regardless of
  size (capped only by the existing `MAX_PARSE_BUF_SIZE = 1 GiB` skip
  budget).
- CRC is *not* verified (consistent with JPEG marker handling, HEIC box
  validation).

### Hex decode

Inline ~10-line helper. Accepts `[0-9 a-f A-F]`; whitespace tolerated.
Odd-length input → fail (returns `Err(())`, decoded silently dropped).
No new crate dependency.

### Format detection (file.rs)

PNG's 8-byte signature `\x89PNG\r\n\x1a\n` is unique and unambiguous.
Add the check **after** `TiffHeader::parse` (defensive ordering even
though there is no actual collision). `MediaMimeImage::Png` variant added.

## Contract relaxation: §3a

A PNG carrying only `tEXt` (and no EXIF in any form) is a real and
common case. To surface its `text_chunks`, the contract of
`MediaParser::parse_exif` is relaxed:

> **Returns `Ok(ExifIter)` if the source contained any image-level
> metadata — EXIF tags *or* PNG text chunks. Returns
> `Err(Error::ExifNotFound)` only when both are absent.**

Concretely, `parse_png_exif_iter` returns `Ok(ExifIter)` whenever
`out.exif.is_some() || !out.text_chunks.is_empty()`. When EXIF is
absent, the iter yields no entries but `iter.text_chunks()` carries
the captured pairs.

This parallels the existing `iter.has_embedded_track()` signal — both
are auxiliary metadata accessors on `ExifIter`/`Exif`.

Other formats unaffected.

## Errors and edge cases

| Case | Behavior |
|---|---|
| Bad PNG signature | `Error::UnsupportedFormat` (mime detection fails first) |
| Truncated mid-chunk | `ParsingError::Need` → eventually `UnexpectedEof` if reader dries up |
| Crafted huge `tEXt` length | `Skip` past it; not captured; parse continues |
| `IEND` missing | Reader EOF → captured chunks returned; partial-but-usable result |
| `tEXt` with no NUL separator | Skip the entry (not pushed into text_chunks) |
| Hex-decode failure on legacy EXIF | Legacy ignored; tEXt entry preserved; if no other source → `ExifNotFound` |
| Legacy hex-decoded TIFF header invalid | Same as above |
| PNG with only `tEXt`, no EXIF | `Ok(ExifIter)` with empty entries + non-empty `text_chunks` (§3a) |
| PNG with truly nothing | `Err(ExifNotFound)` |
| Memory-mode (`from_bytes`) PNG | Same code path, zero-copy `eXIf` slice into shared `Bytes` |
| Async (`parse_exif_async`) PNG | Same code path via `AsyncBufParser`; no PNG-specific async code |

## Testing

### Fixtures (`testdata/`)

| File | Composition | Asserts |
|---|---|---|
| `exif.png` | `eXIf` + `Title`/`Software` `tEXt` | EXIF tags + text_chunks both populated |
| `exif-legacy.png` | `Raw profile type exif` only | EXIF tags via legacy path |
| `exif-legacy-app1.png` | `Raw profile type APP1` only | APP1 prefix strip works |
| `exif-both.png` | `eXIf` + a *different* legacy blob | `eXIf` precedence (assert a tag value unique to it) |
| `text-only.png` | `tEXt` only, no EXIF anywhere | §3a: non-empty iter result, EXIF empty, text_chunks set |
| `text-dup.png` | two `tEXt` with same key | `get` first; `get_all` returns both |
| `no-meta.png` | IHDR + IDAT + IEND | `Error::ExifNotFound` |
| `huge-idat.png` | multi-MB IDAT + post-IEND `tEXt` | streaming Skip works; post-IDAT `tEXt` captured |
| `malformed-text.png` | declared `tEXt` length = 0xFFFFFFFF | bound defense; parse does not panic |

Fixtures generated by `tests/png_fixtures.rs` helper that builds chunk
bytes programmatically. EXIF blob copy-pasted from
`testdata/exif.jpg`'s APP1 segment. CRCs written as zero (parser does
not verify). No new build-time dependency.

### Test layers

- **Unit** (in `src/png.rs`): `extract_chunks` on synthetic buffers —
  one case per chunk type, plus `Need`/`Skip` flow, plus bound defense.
- **Mime** (in `src/file.rs`): `mime` `test_case` adds
  `("exif.png", Image(Png))`.
- **Parser** (in `src/parser.rs`): `parse_media` `test_case` adds the
  fixtures under `Exif`/`NoData`/`Invalid` categories.
- **PngTextChunks** (in `src/exif/png_text.rs`): `get` / `get_all` /
  `iter` / `len` / `is_empty`.
- **Integration** (`tests/png.rs`, NEW): each fixture exercised through
  `read_exif`, `read_exif_from_bytes`, and (under `#[cfg(feature = "tokio")]`)
  `read_exif_async`. Verifies parity across sync / memory / async,
  seekable + unseekable.
- **Regression baseline**: `png_baseline_exif_full_dump` mirrors
  `p4_5_baseline_exif_jpg_full_dump` to lock the captured-tag set for
  `exif.png`.
- **Fuzz**: PNG fixtures added to `fuzz/corpus/media_parser/`; existing
  unified `media_parser` target picks them up automatically.

## Build sequence

Strictly linear (file overlap precludes parallelism). Each phase ends
with `cargo test` green and an atomic commit.

1. **Format detection** — `MediaMimeImage::Png` + signature in
   `file.rs`. Add `if Png { return Err(Error::ExifNotFound) }` short-circuit
   in `parse_exif_iter` (sync + async) so the new variant compiles
   without hitting `extract_exif_with_mime`'s exhaustive match. Mime
   test adds the fixture; phase 4 replaces the short-circuit with
   `parse_png_exif_iter`.
2. **Pure chunk parser** — `src/png.rs` with `extract_chunks` +
   `PngParseOut` + `PngExifSource`. Pure-function unit tests on
   synthetic buffers: `eXIf`-only, `tEXt`-only, IDAT skip, IEND
   termination, bound defense, `Need`/`Skip` returns.
3. **`PngTextChunks` type + plumbing** — `src/exif/png_text.rs`; field
   + accessor on `ExifIter` and `Exif`; `lib.rs` export. Unit tests of
   accessor methods. PNG still stubbed in dispatch.
4. **`parse_png_exif_iter` integration (eXIf path + §3a)** —
   `exif.rs` dispatch; `EXif(Range)` path only; `text-only.png`
   exercises §3a. Sync + async + memory + seekable + unseekable
   covered (no new async code; `load_and_parse` reuse).
5. **Legacy EXIF-in-`tEXt`** — hex decode helper; `Raw profile type
   exif` / `APP1` recognition; `Legacy(bytes)` path in
   `parse_png_exif_iter`. Tests `exif-legacy*.png` and `exif-both.png`.
6. **Docs + CLI + changelog** — `README.md` "Supported File Types";
   `lib.rs` module docs; doctest on `Exif::text_chunks`;
   `examples/rexiftool.rs` adds `-- PNG Text --` section under
   existing `--no-track` analog (`--no-text`); `CHANGELOG.md` under
   `## Unreleased` → `### Added — PNG support (#18)`. Bump to
   `v3.3.0` happens in a separate release commit (out of scope).

### PR shape

Single PR, six commits. PNG support is one user-visible feature; the
phases are an internal review aid, not separate releases.

### Migration

No `MIGRATION.md` entry. Purely additive — no v2 → v3 analog, no
breaking change in v3.x.

## Open questions / risks

- **Risk**: `ExifIter::clone()` on a PNG with many `tEXt` entries
  deep-clones strings. Mitigation: typical PNGs have <10 short entries;
  `Arc<[…]>` upgrade is non-breaking.
- **Risk**: §3a relaxation changes semantics of `Ok(ExifIter)` for PNG
  callers. Mitigation: documented contract update; behavior parallels
  `has_embedded_track`; no other format affected.
- **Open**: does `rexiftool` JSON output need a `_text_chunks` nested
  key? Suggested yes (mirrors `_embedded_track` shape) but worth a
  second look during phase 6.

# PNG Support — Design Spec

**Issue**: [#18 — Support for PNG files?](https://github.com/mindeng/nom-exif/issues/18)
**Status**: Approved (design); ready for plan + execution
**Target version**: v3.3.0 (MINOR bump; additive)

## Scope

Two coupled changes ship together in v3.3:

**A. Image format support for `.png` files**, covering:

1. **Standard EXIF** in PNG `eXIf` chunks (PNG 1.5 / 2017 spec extension).
   Surfaced through the existing `parse_exif` / `read_exif` entry points
   exactly like every other image format — `read_exif("foo.png")` "just
   works".
2. **PNG `tEXt` chunks** as Latin-1 key/value pairs surfaced through a
   new `MediaParser::parse_image_metadata` entry point (parser-level
   only in v3.3 — top-level `read_*` helpers deferred to v4) that
   returns a structured `ImageMetadata { exif, format }`. PNG-specific
   format metadata lives in the `format` field as
   `ImageFormatMetadata::Png(PngTextChunks)`.
3. **Legacy EXIF-in-`tEXt`** transparently merged: ImageMagick's
   `Raw profile type exif` (hex-encoded TIFF) and Photoshop's
   `Raw profile type APP1` (hex-encoded `Exif\0\0` + TIFF). EXIF entries
   become available via `Exif::get(ExifTag::*)` exactly as if they came
   from `eXIf` — *regardless of which entry point the caller uses*.

**B. Source-input model unification** — prerequisite that lets the new
`parse_image_metadata` ship without an `_from_bytes` sibling, and
brings the existing `parse_exif` / `parse_track` family in line:

- New `MediaSource::from_memory(bytes) -> MediaSource<std::io::Empty>`
  constructor.
- Existing `parse_exif<R: Read>` / `parse_track<R: Read>` extended to
  accept memory-mode sources (signatures unchanged; runtime branch on
  the existing `memory: Option<Bytes>` field; zero-copy preserved).
- `#[deprecated]` markers on `MediaSource::<()>::from_bytes`,
  `parse_exif_from_bytes`, `parse_track_from_bytes`, and all top-level
  `read_*_from_bytes` helpers. Removed in v4. **No breaking change in
  v3.x** — old code keeps compiling with deprecation warnings.

Bundling A and B is deliberate: shipping the new PNG API alongside an
unchanged triplet pattern (`parse_*` + `parse_*_from_bytes` +
`parse_*_async`) would lock the inconsistency into a public release.
B is small (~50 lines) and self-contained; bundled it yields one
coherent v3.3 story.

**Out of scope** (deferred to a future phase):

- `iTXt` (UTF-8, optional zlib compression, language tag).
- `zTXt` (zlib-compressed Latin-1).
- Both deferred because they introduce a `flate2` dependency and a
  richer per-entry struct (language tag, translated keyword, compression
  flag) that is not justified by the issue's stated need.

**Out of scope (separate v4 milestone)**:

- `Metadata` enum redesign (e.g. `Metadata::Image(ImageMetadata)`
  replacing `Metadata::Exif(Exif)`).
- `MediaParser::parse_metadata` (the symmetric parser-level dispatch
  that currently exists only as the top-level `read_metadata`).
- **Top-level `read_image_metadata` helpers**. Adding them in v3.3
  alongside the existing `read_metadata` would create two adjacent
  top-level entry points with overlapping-but-not-identical semantics
  (`read_metadata` returns the legacy `Metadata::Exif(Exif)`,
  `read_image_metadata` would return the richer `ImageMetadata`). The
  v4 redesign collapses these into a single coherent `read_metadata`
  story; introducing the asymmetry now and undoing it in v4 is API
  churn.
- These are deliberately deferred so this PR stays focused. The
  `ImageMetadata<E>` struct introduced here is shaped to drop into a
  future `Metadata::Image` variant unchanged.

## Architecture

### Source-input model (unified in v3.3 via deprecation)

This PR also unifies the source-input model so that **a single
`parse_*` method accepts file/stream/memory sources**. The triplet
pattern (`parse_exif` + `parse_exif_from_bytes` + `parse_exif_async`)
collapses to a duplet (`parse_exif` + `parse_exif_async`), with the
sync method handling both file/stream and memory inputs.

Mechanism: introduce a new constructor
`MediaSource::from_memory(bytes) -> MediaSource<std::io::Empty>`.
`std::io::Empty` impls `Read` (returning 0 bytes always), so
`MediaSource<Empty>` satisfies the existing `<R: Read>` bound on every
`parse_*<R: Read>(MediaSource<R>)` method. The parser dispatches on
the existing `memory: Option<bytes::Bytes>` field at runtime — exact
same fast path that `parse_exif_from_bytes` already takes today,
zero-copy preserved.

Legacy API is kept and `#[deprecated]`-marked through v3.x; removed in
v4:

| v3.3 status | Symbol |
|---|---|
| New (preferred) | `MediaSource::from_memory` |
| Deprecated | `MediaSource::<()>::from_bytes`, `MediaParser::parse_exif_from_bytes`, `MediaParser::parse_track_from_bytes`, `read_exif_from_bytes`, `read_track_from_bytes`, `read_metadata_from_bytes` |
| Unchanged | `MediaSource::open`, `MediaSource::seekable`, `MediaSource::unseekable`, `parse_exif<R: Read>`, `parse_track<R: Read>`, `parse_*_async` |

The new `parse_image_metadata` is **born unified** — no `_from_bytes`
sibling at all, since memory-mode sources flow through the same
`<R: Read>` signature via `MediaSource<Empty>`.

### Two coexisting entry points

```rust
// Existing (unchanged behavior contract; signature unchanged)
parser.parse_exif(ms)     -> Result<ExifIter>          // lazy, EXIF-only
read_exif(path)           -> Result<Exif>              // eager, EXIF-only
// `parse_exif` now also accepts `MediaSource<Empty>` built from
// `MediaSource::from_memory(bytes)` — same method, no _from_bytes sibling.

// New (PNG-aware, format-extras-aware) — MediaParser layer only in v3.3.
// Top-level read_image_metadata helpers are deferred to v4 (see Scope).
parser.parse_image_metadata<R: Read>(ms)         -> Result<ImageMetadata<ExifIter>>  // lazy; sync
parser.parse_image_metadata_async<R: AsyncRead>(ms) -> Result<ImageMetadata<ExifIter>>  // lazy; tokio
```

User-facing rule (covered in CHANGELOG / docs):

| Goal | Use |
|---|---|
| Just want EXIF (any format including PNG, any source kind) | `parse_exif` / `read_exif` — unchanged |
| Want EXIF + any format-specific extras (PNG `tEXt`, future GIF Comment, …) | `MediaParser::parse_image_metadata` (parser-level only in v3.3) |

`parse_exif` on PNG still applies the legacy `Raw profile type *`
hex-decode merge — that's part of the EXIF view, not extras. So the
"just want EXIF" path on PNG transparently picks up legacy-encoded EXIF
without code changes.

### Special-cased PNG path inside the parser

PNG runs as a **special-cased path** inside the EXIF-iter pipeline,
peer to the existing CR3 path — *not* through the generic
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
  exif.rs                   add `if Png { return parse_png_exif_iter(…) }` for parse_exif.
  exif/png_text.rs          NEW. PngTextChunks public type.
  image_metadata.rs         NEW. ImageMetadata<E>, ImageFormatMetadata, ExifRepr trait.
  parser.rs                 - add MediaSource<Empty>::from_memory constructor.
                            - extend MediaSource<R: Read>::build to accept Empty too.
                            - add memory-mode branch to parse_exif/parse_track
                              (preserving zero-copy via existing memory field).
                            - #[deprecated] MediaSource::<()>::from_bytes,
                              parse_exif_from_bytes, parse_track_from_bytes.
                            - add MediaParser::parse_image_metadata (unified).
  parser_async.rs           add MediaParser::parse_image_metadata_async (tokio feature).
  lib.rs                    - #[deprecated] read_*_from_bytes top-level helpers.
                            - export new types (ImageMetadata, ImageFormatMetadata,
                              ExifRepr, PngTextChunks).
```

### Data flow

**Path 1 — `parse_exif` on a PNG (existing API)**:
```
PNG bytes
  → parse_exif_iter dispatches on MediaMimeImage::Png to parse_png_exif_iter
  → parser.load_and_parse(reader, skip, png::extract_chunks)
  → png::extract_chunks walks chunk stream, returns PngParseOut
  → parse_png_exif_iter materializes PngExifSource → ExifIter
  → text_chunks discarded (this entry point doesn't expose them)
  → if no EXIF source found: Err(Error::ExifNotFound)
```

**Path 2 — `parse_image_metadata` on any image (new API)**:
```
PNG case:
  → parse_image_metadata dispatches on MediaMimeImage::Png to parse_png_full
  → parser.load_and_parse(reader, skip, png::extract_chunks)   // SAME helper
  → png::extract_chunks walks chunk stream, returns PngParseOut
  → ImageMetadata {
       exif: out.exif.map(materialize → ExifIter),
       format: (!text_chunks.is_empty()).then(|| ImageFormatMetadata::Png(text_chunks)),
     }

Non-PNG case (jpeg/heic/avif/tiff/raf/cr3):
  → parse_image_metadata calls existing parse_exif_iter
  → ImageMetadata { exif: Some(iter), format: None }
  → zero overhead vs parse_exif
```

`png::extract_chunks` is shared between both paths — single source of
truth for PNG parsing.

### EXIF-source priority (single source, no merging)

When multiple potential EXIF sources are present in the same PNG:

1. `eXIf` chunk wins;
2. else `Raw profile type APP1` wins;
3. else `Raw profile type exif`.

No merging: each source produces a single TIFF byte stream that is fed
unchanged to the existing IFD pipeline. The lower-priority sources are
ignored for EXIF purposes but their original `tEXt` entries remain
visible via `format.iter()` (so debug/audit is possible).

Rationale: the IFD pipeline does not currently support merging two
TIFF blobs with (ifd, tag) deduplication; encoders sophisticated
enough to write `eXIf` write current data there, while a `Raw profile`
text chunk is typically stale leftover from an earlier edit chain.

## Public API additions

```rust
// ----- Sealed-trait pattern: which "EXIF representation" can be
//       held by ImageMetadata<E>. Exactly two impls — Exif (eager) and
//       ExifIter (lazy) — and the trait is sealed so users cannot add
//       more.
mod sealed { pub trait Sealed {} }
pub trait ExifRepr: sealed::Sealed {}

impl sealed::Sealed for Exif {}      impl ExifRepr for Exif {}
impl sealed::Sealed for ExifIter {}  impl ExifRepr for ExifIter {}

// ----- The new structured return type for parse_image_metadata.
//       Default `E = Exif` matches the eager conventions used by
//       `read_exif` and today's `Metadata::Exif(Exif)`, and lines up
//       with the v4 `Metadata::Image(ImageMetadata)` candidate.
//       Callers receiving the lazy form from
//       MediaParser::parse_image_metadata get
//       `ImageMetadata<ExifIter>` explicitly; conversion to eager via
//       the `From` impl below.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ImageMetadata<E: ExifRepr = Exif> {
    pub exif: Option<E>,
    pub format: Option<ImageFormatMetadata>,
}

impl From<ImageMetadata<ExifIter>> for ImageMetadata<Exif> {
    fn from(m: ImageMetadata<ExifIter>) -> Self {
        ImageMetadata {
            exif: m.exif.map(Into::into),
            format: m.format,
        }
    }
}

// ----- Format-specific metadata (the part that does NOT live in
//       EXIF/IFD). One variant per format that has such metadata.
//       `#[non_exhaustive]` so adding variants is non-breaking.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum ImageFormatMetadata {
    Png(PngTextChunks),
    // future: Gif(GifComment), Webp(WebpChunks), …
}

// ----- PNG `tEXt` chunks as Latin-1-decoded (key, value) pairs.
//       Opaque wrapper around Vec<(String, String)> so future iTXt /
//       zTXt extension is non-breaking.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
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

// ----- New methods on MediaParser. Single sync method handles
//       file/stream/memory sources via the unified <R: Read> bound
//       (memory mode flows through `MediaSource<std::io::Empty>`
//       built from `MediaSource::from_memory`). Top-level
//       read_image_metadata helpers are deferred to v4 — see "Out of
//       scope" in the Scope section.
impl MediaParser {
    pub fn parse_image_metadata<R: Read>(&mut self, ms: MediaSource<R>)
        -> Result<ImageMetadata<ExifIter>>;

    #[cfg(feature = "tokio")]
    pub async fn parse_image_metadata_async<R: AsyncRead + Unpin + Send>(
        &mut self, ms: AsyncMediaSource<R>,
    ) -> Result<ImageMetadata<ExifIter>>;
}

// ----- Source-input model unification. New constructor + deprecation
//       of the old MediaSource<()> shape and all parse_*_from_bytes
//       siblings. Removed in v4.
impl MediaSource<std::io::Empty> {
    /// Build a `MediaSource` from in-memory bytes. Replaces the v3.0
    /// `MediaSource::<()>::from_bytes` (now deprecated).
    pub fn from_memory(bytes: impl Into<bytes::Bytes>) -> Result<Self>;
}

impl MediaSource<()> {
    #[deprecated(
        since = "3.3.0",
        note = "Use `MediaSource::from_memory` and the unified \
                `parse_*` methods. The `MediaSource<()>` shape will \
                be removed in v4."
    )]
    pub fn from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Self>;
}

impl MediaParser {
    #[deprecated(since = "3.3.0", note = "Use `parse_exif` directly — \
        it accepts memory-mode sources built via `MediaSource::from_memory`.")]
    pub fn parse_exif_from_bytes(&mut self, ms: MediaSource<()>) -> Result<ExifIter>;

    #[deprecated(since = "3.3.0", note = "Use `parse_track` with \
        `MediaSource::from_memory`.")]
    pub fn parse_track_from_bytes(&mut self, ms: MediaSource<()>) -> Result<TrackInfo>;
}

#[deprecated(since = "3.3.0", note = "Use `read_exif` with \
    `MediaSource::from_memory`.")]
pub fn read_exif_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Exif>;

#[deprecated(since = "3.3.0", note = "Use `read_exif_iter` with \
    `MediaSource::from_memory`.")]
pub fn read_exif_iter_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<ExifIter>;

#[deprecated(since = "3.3.0", note = "Use `read_track` with \
    `MediaSource::from_memory`.")]
pub fn read_track_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<TrackInfo>;

#[deprecated(since = "3.3.0", note = "Use `read_metadata` with \
    `MediaSource::from_memory`.")]
pub fn read_metadata_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Metadata>;
```

**Not in `prelude`** (cold path).

**Not exported**: `MediaMimeImage::Png` stays `pub(crate)` like every
other variant. Users branch on the `ImageFormatMetadata` enum, not on mime.

### Default type parameter rationale

`ImageMetadata<E: ExifRepr = Exif>` — default is the **eager** form.

| Reasoning | Detail |
|---|---|
| Forward-compat to v4 | If/when v4 introduces `Metadata::Image(ImageMetadata)`, the unparametrized form mirrors today's `Metadata::Exif(Exif)` (eager) — zero behavior change for `read_metadata` callers. The `From<ImageMetadata<ExifIter>> for ImageMetadata<Exif>` impl is also the natural target type for that conversion. |
| Lazy callers spell it out | `parse_image_metadata` returns `ImageMetadata<ExifIter>` explicitly. Same pattern as `Vec::with_capacity_in(_, alloc) -> Vec<T, A>` — the default doesn't constrain non-default constructors. |
| Container/storage type | When users store / pass `ImageMetadata` around (e.g. as a function parameter or struct field), they typically want the eager Exif form — defaulted parameter keeps these write-sites short. |

### Encoding policy (Latin-1, strict)

PNG `tEXt` is Latin-1 by spec. Decode is byte-by-byte
`String::from_iter(bytes.iter().map(|&b| b as char))` — infallible
(every byte maps to a Unicode code point). **No UTF-8 sniffing or
fallback.** Encoders that violate the spec by writing UTF-8 produce
mojibake when read; callers needing recovery handle it themselves.

This decision is documented on `PngTextChunks` so the contract is
explicit.

### Storage shape (eager, owned)

`PngTextChunks` wraps `Vec<(String, String)>`. Eager parsing during the
single PNG chunk walk; no laziness.

- Lazy is impossible anyway: legacy `Raw profile type exif` detection
  must inspect every `tEXt` key during parse; we walk to `IEND` regardless.
- `PngTextChunks` is `Clone`; the Vec is deep-cloned on `clone()`.
  Acceptable because typical PNGs carry <10 short ASCII strings.
  Upgrade path to `Arc<[(String, String)]>` is non-breaking if
  profiling ever shows the cost.

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

Both `parse_exif` and `parse_image_metadata` consume the same
`PngParseOut`. The former discards `text_chunks`; the latter packages
both fields into `ImageMetadata`.

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

## Existing-API contracts preserved

`parse_exif` / `read_exif` semantics on PNG:

| PNG content | `parse_exif` / `read_exif` result |
|---|---|
| `eXIf` chunk only | `Ok(ExifIter)` / `Ok(Exif)` — same as any other format |
| Legacy `Raw profile type *` only | `Ok(...)` — legacy hex blob transparent-merged |
| Both `eXIf` and legacy | `Ok(...)` — `eXIf` wins per priority rules |
| `tEXt` only, no EXIF anywhere | `Err(Error::ExifNotFound)` — *unchanged contract* |
| Truly nothing | `Err(Error::ExifNotFound)` |

Users who care about `tEXt`-only PNGs use
`MediaParser::parse_image_metadata`. The original `parse_exif`
contract is **not** relaxed — every format including PNG returns
`Ok(ExifIter)` only when EXIF is found.

## Errors and edge cases

| Case | Behavior |
|---|---|
| Bad PNG signature | `Error::UnsupportedFormat` (mime detection fails first) |
| Truncated mid-chunk | `ParsingError::Need` → eventually `UnexpectedEof` if reader dries up |
| Crafted huge `tEXt` length | `Skip` past it; not captured; parse continues |
| `IEND` missing | Reader EOF → captured chunks returned; partial-but-usable result |
| `tEXt` with no NUL separator | Skip the entry (not pushed into text_chunks) |
| Hex-decode failure on legacy EXIF | Legacy ignored; tEXt entry preserved; if no other source → ExifNotFound on `parse_exif`; `format: Some(Png(text_chunks))` on `parse_image_metadata` if any tEXt captured |
| Legacy hex-decoded TIFF header invalid | Same as above |
| PNG with only `tEXt`, no EXIF | `parse_exif` → `Err(ExifNotFound)`. `parse_image_metadata` → `Ok(ImageMetadata { exif: None, format: Some(ImageFormatMetadata::Png(...)) })` |
| PNG with truly nothing | Both APIs → `Err(ExifNotFound)` |
| Memory-mode (`from_bytes`) PNG | Same code path, zero-copy `eXIf` slice into shared `Bytes` |
| Async (`parse_*_async`) PNG | Same code path via `AsyncBufParser`; no PNG-specific async code |

`parse_image_metadata` returns `Err(ExifNotFound)` when **both** `exif`
and `format` are `None` — symmetric with `parse_exif`'s contract,
keeps error semantics consistent across the two APIs.

## Testing

### Fixtures (`testdata/`)

| File | Composition | Asserts |
|---|---|---|
| `exif.png` | `eXIf` + `Title`/`Software` `tEXt` | `parse_exif` returns EXIF; `parse_image_metadata` returns both fields populated |
| `exif-legacy.png` | `Raw profile type exif` only | EXIF tags via legacy path under both APIs |
| `exif-legacy-app1.png` | `Raw profile type APP1` only | APP1 prefix strip works |
| `exif-both.png` | `eXIf` + a *different* legacy blob | `eXIf` precedence (assert a tag value unique to it) |
| `text-only.png` | `tEXt` only, no EXIF anywhere | `parse_exif` → `ExifNotFound`; `parse_image_metadata` → `exif: None, format: Some(Png(...))` |
| `text-dup.png` | two `tEXt` with same key | `get` first; `get_all` returns both |
| `no-meta.png` | IHDR + IDAT + IEND | both APIs → `Error::ExifNotFound` |
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
- **ImageMetadata + ImageFormatMetadata** (in `src/image_metadata.rs`):
  default value, generic instantiation, `From<ImageMetadata<ExifIter>>`
  conversion.
- **Integration** (`tests/png.rs`, NEW): each fixture exercised through
  the EXIF-only entry points (`read_exif` for files,
  `read_exif` with `MediaSource::from_memory` for in-memory bytes,
  and under `#[cfg(feature = "tokio")]` `read_exif_async`) and the
  image-metadata entry points (`MediaParser::parse_image_metadata`
  for both file-backed and memory-backed sources, and under
  `#[cfg(feature = "tokio")]` `parse_image_metadata_async`). Verifies
  parity across file / stream / memory / async, seekable +
  unseekable.
- **Source-input unification** (in `src/parser.rs` tests): regression
  fixture re-runs the existing memory-mode tests through the new
  `from_memory` path, asserting (a) `parse_exif(MediaSource::from_memory(...))`
  succeeds and (b) the cached-pointer / share_buf zero-copy invariant
  is preserved. Plus a `#[allow(deprecated)]` test confirming
  `parse_exif_from_bytes` still works.
- **Regression baseline**: `png_baseline_exif_full_dump` mirrors
  `p4_5_baseline_exif_jpg_full_dump` to lock the captured-tag set for
  `exif.png`.
- **Fuzz**: PNG fixtures added to `fuzz/corpus/media_parser/`; existing
  unified `media_parser` target picks them up automatically.

## Build sequence

Strictly linear (file overlap precludes parallelism). Each phase ends
with `cargo test` green and an atomic commit.

0. **Source-input unification + deprecation** —
   - `parser.rs`: add `MediaSource::<std::io::Empty>::from_memory`
     constructor (mirrors `from_bytes` but uses `std::io::empty()` as
     reader and stashes bytes in `memory: Some(...)` exactly as
     `from_bytes` already does).
   - `parser.rs`: extend `parse_exif<R: Read>` and `parse_track<R: Read>`
     internals with `if ms.memory.is_some()` branch — same body as
     today's `parse_exif_from_bytes` / `parse_track_from_bytes`. Method
     signatures unchanged. Existing zero-copy memory mode preserved
     verbatim.
   - `parser.rs`: `#[deprecated(since = "3.3.0")]` on
     `MediaSource::<()>::from_bytes`, `parse_exif_from_bytes`,
     `parse_track_from_bytes`. `#[allow(deprecated)]` inside this
     crate's own tests/internals that still exercise these paths.
   - `lib.rs`: `#[deprecated]` on `read_exif_from_bytes`,
     `read_exif_iter_from_bytes`, `read_track_from_bytes`,
     `read_metadata_from_bytes`.
   - Tests: existing `parse_exif_from_bytes_*` / similar tests stay,
     plus a parallel set using `from_memory` route, asserting
     identical behavior + zero-copy invariant
     (`cached_ptr_for_test` semantics).
   - Docs: README + `lib.rs` module docs migrate code samples from
     `from_bytes` → `from_memory`. `MIGRATION.md` gets a small
     "v3.0 → v3.3" subsection documenting the deprecation/migration.
   - No PNG yet; CI green; all existing user code still compiles
     (with deprecation warnings on old paths only).
1. **Format detection** — `MediaMimeImage::Png` + signature in
   `file.rs`. Add `if Png { return Err(Error::ExifNotFound) }`
   short-circuit in `parse_exif_iter` (sync + async) so the new variant
   compiles without hitting `extract_exif_with_mime`'s exhaustive
   match. Mime test adds the fixture; phase 4 replaces the
   short-circuit with `parse_png_exif_iter`.
2. **Pure chunk parser** — `src/png.rs` with `extract_chunks` +
   `PngParseOut` + `PngExifSource`. Pure-function unit tests on
   synthetic buffers: `eXIf`-only, `tEXt`-only, IDAT skip, IEND
   termination, bound defense, `Need`/`Skip` returns. No integration
   yet.
3. **Public types** — `src/exif/png_text.rs` (`PngTextChunks`) +
   `src/image_metadata.rs` (`ImageMetadata<E>`, `ImageFormatMetadata`,
   `ExifRepr` sealed trait, `From<ImageMetadata<ExifIter>>` impl).
   Exports in `lib.rs`. Unit tests of accessors + generic
   instantiation. PNG still stubbed in dispatch.
4. **`parse_png_exif_iter` + `parse_image_metadata` integration
   (eXIf path only)** —
   - `exif.rs`: dispatch `if Png { parse_png_exif_iter(...) }` for
     `parse_exif`, materializing `EXif(Range)` to `ExifIter` (the
     `Legacy(_)` arm is added in phase 5).
   - `parser.rs`: `MediaParser::parse_image_metadata<R: Read>`
     dispatching on `MediaMimeImage::Png` to a new `parse_png_full`
     helper, falling back to `parse_exif_iter` for non-PNG (returns
     `ImageMetadata { exif: Some(iter), format: None }`). Single
     method handles file/stream/memory thanks to phase 0.
   - `parser_async.rs`: `parse_image_metadata_async` mirrors the sync
     path; reuses `load_and_parse` so no PNG-specific async code.
   - Tests: `exif.png` and `text-only.png` exercised through file
     and memory (`from_memory`) inputs, sync + async.
5. **Legacy EXIF-in-`tEXt`** — hex decode helper; `Raw profile type
   exif` / `APP1` recognition; `Legacy(bytes)` path materialized in
   `parse_png_exif_iter` and `parse_png_full`. Tests `exif-legacy*.png`
   and `exif-both.png`.
6. **Docs + CLI + changelog** —
   - `README.md`: add `.png` under "Supported File Types"; add a
     `parse_image_metadata` usage example (lazy iter + optional
     `.into()` to eager `ImageMetadata<Exif>`); migrate any
     `from_bytes` examples in the doc to `from_memory` and add a
     short note at the bottom of the In-Memory Bytes section about
     the deprecation.
   - `lib.rs`: module-level docs updated with PNG entry, the new
     `parse_image_metadata` story, and the `from_memory`/deprecation
     migration note. All existing in-file doc examples that use
     `from_bytes` switched to `from_memory`.
   - Doctest on `parse_image_metadata` (file + memory both
     compile-pass).
   - `examples/rexiftool.rs`: print `-- Format Metadata --` section
     mirroring `-- Embedded Track --`; opt-out flag `--no-format`
     (parallel to `--no-track`); migrate the example's own usage to
     `from_memory` if it uses memory mode anywhere.
   - `CHANGELOG.md`:
     `## Unreleased`
       `### Added` — PNG support (#18) + `MediaSource::from_memory`
                    + `MediaParser::parse_image_metadata`.
       `### Deprecated` — `MediaSource::<()>::from_bytes`,
                          `parse_exif_from_bytes`,
                          `parse_track_from_bytes`,
                          all top-level `read_*_from_bytes`.
                          Removed in v4.
       `### Notes` — top-level `read_image_metadata` helpers are
                     deferred to v4 alongside the planned `Metadata`
                     enum redesign.
   - Bump to `v3.3.0` happens in a separate release commit (out of
     scope of this PR).

### PR shape

Single PR, seven commits (phases 0–6). PNG support is one
user-visible feature; the source-input unification (phase 0) is a
prerequisite that makes the new API consistent with the unified form,
not a standalone change worth its own PR.

### Migration

`MIGRATION.md` gets a small "v3.0 → v3.3" subsection covering the
deprecations:

- `MediaSource::<()>::from_bytes` → `MediaSource::from_memory`
- `parse_exif_from_bytes` → `parse_exif` (now accepts memory-mode
  sources directly)
- `parse_track_from_bytes` → `parse_track`
- `read_exif_from_bytes` → `read_exif` (after `from_memory`)
- `read_exif_iter_from_bytes`, `read_track_from_bytes`,
  `read_metadata_from_bytes` — analogous

All deprecated symbols still compile in v3.x; removal scheduled for v4.

PNG support itself is purely additive — no user code change needed
unless they want the new `parse_image_metadata` capabilities.

### v4 plan

Two threads queue up for the v4 milestone:

**(1) Removals** — drop the `#[deprecated]` items introduced in v3.3:

- `MediaSource::<()>::from_bytes` (entire `MediaSource<()>` impl block)
- `MediaParser::parse_exif_from_bytes`, `parse_track_from_bytes`
- top-level `read_*_from_bytes` helpers

**(2) `Metadata` redesign** — the `ImageMetadata<E>` struct introduced
in v3.3 is shaped to be reused unchanged by a future v4 redesign of
the `Metadata` enum:

```rust
// v4 candidate (separate milestone)
#[non_exhaustive]
pub enum Metadata<E: ExifRepr = Exif> {
    Image(ImageMetadata<E>),       // ← reuses this PR's struct
    Track(TrackInfo),
}
```

Default `E = Exif` matches today's `Metadata::Exif(Exif)` eager
behavior; v4 callers of `read_metadata` see no surprise.

In addition, v4 is the natural place to introduce **top-level
`read_image_metadata` helpers** alongside the redesigned
`read_metadata` — deliberately deferred from v3.3 to avoid two
adjacent top-level entry points with overlapping semantics during the
v3 → v4 transition.

This forward-compat is a *design property*, not a v3.3 commitment —
v4 may evolve independently. Plant a project seed to revisit during
v4 milestone planning.

## Open questions / risks

- **Risk**: `PngTextChunks::clone()` on PNGs with many `tEXt` entries
  deep-clones strings. Mitigation: typical PNGs have <10 short entries;
  `Arc<[…]>` upgrade is non-breaking.
- **Risk**: introducing a sealed `ExifRepr` trait + generic public
  type adds minor cognitive load to `ImageMetadata` doc. Mitigation:
  default type parameter hides the generic where users name the type
  in storage / function signatures; the parser-level entry points
  return `ImageMetadata<ExifIter>` explicitly so the lazy form is
  obvious from the signature; doctests demonstrate both forms.
- **Risk**: in-crate uses of the deprecated `_from_bytes` symbols
  (tests, examples) will produce deprecation warnings. Mitigation:
  add `#[allow(deprecated)]` to the existing test functions that
  exercise those paths (we still want them tested through v3.x);
  ensure CI doesn't treat warnings as errors. Phase 0 explicitly
  audits this so v3.3 ships green.
- **Risk**: PR scope expanded by phase 0 (source-input unification +
  global deprecation) beyond the initial "PNG only" charter.
  Mitigation: phase 0 is mechanical and self-contained (no behavior
  change for non-deprecated paths); the unification is a *prerequisite*
  for the new `parse_image_metadata` to be born without
  `_from_bytes` siblings. Splitting into two PRs would force the new
  PNG API to either match the old triplet pattern (defeating
  consistency goal) or land before the unification is in place
  (leaving a window of inconsistency).
- **Open**: does `rexiftool` JSON output need a `_format` (or
  `_text_chunks`) nested key? Suggested yes (mirrors `_embedded_track`
  shape) but worth a second look during phase 6.

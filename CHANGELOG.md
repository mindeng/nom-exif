# Changelog

## nom-exif v3.4.2 (2026-05-20)

### Fixed

- **Streaming PNG parsing for files with non-trivial IDAT** — every
  real-world PNG (i.e. anything beyond a stripped-down test fixture)
  surfaced `malformed iso-bmff box: PNG: bad signature` from
  `parse_exif` / `parse_image_metadata`. Root cause was a two-part
  bug in the chunk walker: (a) `ClearAndSkip(total - remaining)`
  under-requested the skip distance by exactly `cursor + remaining`
  bytes — semantically the caller should advance the parser's
  logical position by `cursor + total`, not just past the buffer's
  end — leaving the parser stranded mid-IDAT; (b) on the resumed call
  `extract_chunks` always re-validated `buf[..8]` against the PNG
  signature, but the resumed buffer started mid-stream and the check
  failed. Fixed both: skip request is now `cursor + total`, and a new
  `ParsingState::PngPastSignature` tells the resumed call to skip the
  signature check. In-memory mode (`from_memory`) was unaffected
  because the full file is buffered at once and `ClearAndSkip` never
  fires. Fixes [#55](https://github.com/mindeng/nom-exif/issues/55).

### Fixed (behaviour)

- **`Error::Malformed.kind` correctly identifies the failing
  structural unit.** Previously every parse failure that flowed
  through `From<ParsedError> for Error` or
  `From<nom::Err<...>> for Error` was hard-coded as
  `MalformedKind::IsoBmffBox` / `MalformedKind::TiffHeader`
  respectively — misleading for PNG / JPEG / EBML inputs. The
  `MalformedKind` is now threaded through `ParsingError::Failed`,
  `ParsedError::Failed`, and `LoopAction::Failed`, and surfaced
  unchanged at the `Error` boundary. Downstream code that
  (incorrectly) matched on `kind == IsoBmffBox` to catch *any*
  parse failure will need updating; conformant code that uses a
  `_ =>` arm (required by `#[non_exhaustive]`) is unaffected.

### Added

- `MalformedKind::PngChunk` variant. `MalformedKind` is
  `#[non_exhaustive]`, so adding a variant is non-breaking.

## nom-exif v3.4.1 (2026-05-12)

### Fixed

- **GPS sub-IFD parsing for Sony A7C2 HIF (and any camera that emits
  GPSVersionID first)** — `IfdIter::parse_tag_entry` short-circuited
  on `tag == 0` as a defensive guard against zero-padded malformed
  IFDs. But tag 0 is also the legitimate GPSVersionID — the
  spec-defined first entry of the GPS sub-IFD. Aborting iteration
  there caused the whole sub-IFD to be dropped, silently losing every
  GPS field. Now gated on `!self.is_gps_subifd()` so the defense
  survives in non-GPS contexts while GPSVersionID parses normally.
  Fixes [#50](https://github.com/mindeng/nom-exif/issues/50).

## nom-exif v3.4.0 (2026-05-10)

### Changed (BREAKING for `serde` feature)
- **Structured `Serialize` for `EntryValue`**. The previous impl
  stringified everything via `Display`, which meant numeric arrays
  and `Undefined` byte blobs were truncated with `...` after 8 / 9
  elements — JSON consumers silently lost data, and rationals came
  out as opaque strings like `"175/100 (1.7500)"`. The new shape:
  - Scalar numerics → JSON numbers.
  - `Text` / `DateTime` / `NaiveDateTime` → strings (formats
    unchanged).
  - `URational` / `IRational` → `{"numerator", "denominator"}`
    objects (uses the existing `Rational<T>` `Serialize` derive).
  - `URationalArray` / `IRationalArray` → JSON arrays of those
    objects, never truncated.
  - `Undefined(Vec<u8>)` → continuous lowercase hex string
    (e.g. `"30323230"`), never truncated.
  - `U8Array` / `U16Array` / `U32Array` → JSON arrays of numbers.
### Changed (BREAKING for `Display` / `to_string`)
- **`Display` no longer truncates arrays.** The 8-element ellipsis cap
  on `Undefined`, `U8Array`, `U16Array`, `U32Array`, and the 3-element
  cap on `URationalArray` / `IRationalArray` are gone. `to_string()`
  now emits every element. Callers that need a length cap should
  impose it at their layer (rexiftool already does this).
- **`EntryValue::Undefined` rendering redesigned.** When all bytes are
  printable ASCII (`0x20..=0x7E`), it now displays as a quoted string
  (e.g. `ExifVersion` → `"0220"`, `GPSProcessingMethod` → `"CELLID"`).
  Otherwise it displays as a continuous lowercase hex string prefixed
  with `0x` (e.g. `ComponentsConfiguration` → `0x01020300`). The
  `Undefined[0xNN, 0xNN, ...]` wrapper is gone. `U8Array` /
  `U16Array` / `U32Array` keep their `Name[...]` form.

## nom-exif v3.3.0 (2026-05-10)

### Added
- **PNG support (#18)** — `read_exif("foo.png")` and friends now work
  for PNG files, covering standard `eXIf` chunks and legacy
  hex-encoded EXIF in `Raw profile type exif` / `Raw profile type
  APP1` `tEXt` chunks (ImageMagick / Photoshop pattern). Legacy
  hex-encoded EXIF is transparently merged into `Exif::get(...)`.
- **`MediaParser::parse_image_metadata`** — new entry point that
  returns `ImageMetadata { exif, format }`, surfacing PNG `tEXt`
  chunks via `ImageFormatMetadata::Png(PngTextChunks)`. Single
  method handles file/stream/memory inputs (no `_from_bytes`
  sibling). Async variant under the `tokio` feature.
- **`MediaSource::from_memory`** — replaces `MediaSource::<()>::from_bytes`.
  Returns `MediaSource<std::io::Empty>` so `parse_exif<R: Read>`,
  `parse_track<R: Read>`, and `parse_image_metadata<R: Read>` can
  all accept memory-mode sources directly.
- **`AsyncMediaSource::from_memory`** (tokio feature) — async
  counterpart, returns `AsyncMediaSource<tokio::io::Empty>`. The
  three `parse_*_async` methods all accept memory-mode sources
  directly with the same zero-copy `bytes::Bytes` story as sync.
- **New public types**: `ImageMetadata<E: ExifRepr = Exif>`,
  `ImageFormatMetadata` (`#[non_exhaustive]`), `PngTextChunks`,
  `ExifRepr` sealed trait.
- `examples/rexiftool` prints PNG `tEXt` chunks under a
  `-- Format Metadata --` section (and `_format` JSON key); add
  `--no-format` to suppress.

### Deprecated
- `MediaSource::<()>::from_bytes` — use `MediaSource::from_memory`.
- `MediaParser::parse_exif_from_bytes` — use `parse_exif` directly
  with a `MediaSource::from_memory` source.
- `MediaParser::parse_track_from_bytes` — analogous.
- `read_exif_from_bytes`, `read_exif_iter_from_bytes`,
  `read_track_from_bytes`, `read_metadata_from_bytes` — analogous.
- All deprecated symbols still compile and pass their original
  tests in v3.x. Removal scheduled for v4.

### Notes
- Top-level `read_image_metadata` helpers are deferred to v4
  alongside the planned `Metadata` enum redesign (a single
  `read_metadata` returning `Metadata::Image(ImageMetadata)`).
  Mixed-content batch users on v3.3 still match on
  `MediaSource::kind()` to dispatch between `parse_image_metadata`
  and `parse_track`.
- PNG `iTXt` and `zTXt` chunks are not yet supported (would require
  a `flate2` dependency for `iTXt`'s optional zlib-compressed
  variant). Their addition is non-breaking — `PngTextChunks` is
  shaped to extend.

## nom-exif v3.2.0 (2026-05-09)

### Added

- **AVIF (AV1 Image File Format) support.** Files with `ftyp` major or
  compatible brands `avif`, `avis`, or `avio` are now recognized and
  routed through the existing HEIF Exif extractor — AVIF reuses the
  ISO BMFF `meta` / `Exif` item layout from ISO/IEC 23008-12, so no
  new parser was needed. New `MediaMimeImage::Avif` variant.
  AVIF detection runs before the HEIF compatible-brand check because
  AVIF files commonly include `mif1` / `miaf` in their compatible-brand
  list. Closes #45.

- New test fixture `testdata/exif.avif` (12 KB; transcoded from
  `testdata/exif.heic` via ImageMagick with Exif preserved).

## nom-exif v3.1.1 (2026-05-09)

### Fixed

- Apply `cargo fmt` to long-line breaks introduced in 3.1.0
  (`src/exif/exif_exif.rs`, `src/jpeg.rs`, `src/parser.rs`). Pure
  formatting; no functional changes. The 3.1.0 release passed every CI
  job except `cargo fmt --check`; 3.1.1 closes that gap. Users on
  3.1.0 should upgrade only if their build pipeline runs
  `cargo fmt --check` against the published source.

## nom-exif v3.1.0 (2026-05-09)

### Added

- **Motion Photo extraction for JPEG.** `parse_exif` now content-detects
  three XMP layouts during its APP-marker walk and sets
  [`Exif::has_embedded_track`] / [`ExifIter::has_embedded_track`] to
  `true` when any is present:
  1. **Adobe XMP Container directory** — `<Container:Directory>` with
     a `Container:Item` entry whose `Item:Mime="video/mp4"` and
     `Item:Semantic="MotionPhoto"`. Used by modern Pixel cameras
     (including Pixel 9 Pro XL Ultra HDR Motion Photos) and Samsung
     Galaxy Motion Photos.
  2. **`GCamera:MotionPhotoOffset="N"`** attribute (mid-era Pixel
     `PXL_*.MP.jpg`).
  3. **`GCamera:MicroVideoOffset="N"`** attribute (pre-2018 Pixel
     `MVIMG_*.jpg`).

  When the flag is `true`, `MediaParser::parse_track` on the same
  source locates the trailing MP4 (computing the offset from the
  Container directory or attribute) and returns its `TrackInfo` —
  previously it returned `Error::TrackNotFound` for any image MIME.
  Samsung's `Item:Padding` semantics ("padding bytes between this item
  and the next") are honored: for the final item the padding is
  ignored (no bytes after it).

  Detection is demand-driven: `parse_exif` only reads bytes past the
  EXIF segment when the XMP scanner reports it ran out mid-walk
  (capped at 256 KB extra to bound malformed inputs). Plain JPEGs and
  the common case where XMP fits inside the EXIF-fill pay zero extra
  I/O.

- **`rexiftool --no-track` flag.** When the source is an image with an
  embedded track, the example tool now extracts the track too: under
  an `-- Embedded Track --` separator in text mode, or a nested
  `_embedded_track` object in `--json` mode. Pass `--no-track` to
  suppress.

- New synthetic test fixture `testdata/motion_photo_pixel_synth.jpg`
  built from existing repo files via
  `testdata/scripts/build_motion_photo_fixture.py` (no third-party
  content; legally clean).

### Renamed

- `Exif::has_embedded_media()` → `Exif::has_embedded_track()`
- `ExifIter::has_embedded_media()` → `ExifIter::has_embedded_track()`

The original names implied "any embedded media" but the actual
semantics target a paired media track. Old names remain as
`#[deprecated]` aliases that forward to the new methods.

### Deprecated (no replacement)

- `TrackInfo::has_embedded_media()` is now deprecated and always
  returns `false`. The 3.0.0 method was reserved for "track source
  carries another embedded track" detection (e.g. mka with both
  audio and video) but the detection was never wired up. Without a
  concrete use case there is no symmetric
  `TrackInfo::has_embedded_track()` in v3.1; the deprecated method
  stays as a no-op for source compatibility.

### Changed

- `has_embedded_track` is now **content-detected**, not MIME-guessed.
  In 3.0.0 this flag was `true` for any HEIC/HEIF/RAF source whether
  or not a track actually existed; in 3.1.0 it returns `true` only
  when a real Motion Photo signal is observed in the JPEG's XMP.
  Plain HEIC, plain JPEG, and RAF correctly return `false`.

### Fixed

- `MediaMimeImage::Raf` no longer flips `has_embedded_track()` to
  `true` — RAF's preview is a still JPEG, not a media track.

## nom-exif v3.0.0 (2026-05-09)

**Breaking release.** The public API has been reshaped end-to-end. The
canonical migration guide is [`docs/MIGRATION.md`](docs/MIGRATION.md);
internal design rationale lives in `docs/V3_API_DESIGN.md`.

### Highlights

- One-shot helpers: `read_exif`, `read_exif_iter`, `read_track`, `read_metadata` (and `_async` variants under `feature = "tokio"`).
- Zero-copy memory input: `MediaSource::<()>::from_bytes(impl Into<bytes::Bytes>)` constructor + `MediaParser::parse_exif_from_bytes` / `MediaParser::parse_track_from_bytes` methods + one-shot `read_exif_from_bytes` / `read_exif_iter_from_bytes` / `read_track_from_bytes` / `read_metadata_from_bytes` helpers. Accepts `Vec<u8>`, `&'static [u8]`, `Bytes`, and `Bytes::from_owner(...)`; returned `ExifIter` / sub-IFDs / CR3 CMT blocks share the user's allocation via `bytes::Bytes` refcount.
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

### Migration Table (excerpt — full guide in [`docs/MIGRATION.md`](docs/MIGRATION.md))

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
| `TrackInfoTag::ImageWidth` / `ImageHeight` | `TrackInfoTag::Width` / `Height` (Track context only; `ExifTag::ImageWidth/ImageHeight` unchanged) |
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
- `BufferedParserState` gains a memory mode (no public surface change); streaming parse path is untouched (P7).

## nom-exif v2.8.0

### Added

- Add Sony XAVC mp4 support [#48](https://github.com/mindeng/nom-exif/pull/48)

### Changed

- Replace `unreachable!()` panics in HEIF/TIFF state machines with proper error returns
- Remove commented-out dead code in `partial_vec.rs`
- Clarify `Load` vs `BufParser` migration path in source comments
- Use macro to generate `TryFromBytes` implementations, reducing repetitive boilerplate

## nom-exif v2.7.0

### Added

- `EntryValue::as_time_components`

### Fixed

- Conversion function as_time(&self) fails with Naive DateTime [#47](https://github.com/mindeng/nom-exif/issues/47)

## nom-exif v2.6.1

### Fixed

- Bug in extracting creation date in iPhone MOV files [#46](https://github.com/mindeng/nom-exif/issues/46)

## nom-exif v2.6.0

### Added

- Added support for Canon CR3 raw file format #44

## nom-exif v2.5.4

### Fixed

- Fixed fuzzing-induced hangs.

## nom-exif v2.5.3

### Fixed

- Fixed fuzzing-induced crashes.

## nom-exif v2.5.2

### Fixed

- IFD parse error for large "MakerNote" entries (TIFF/IIQ source) #40

## nom-exif v2.5.1

### Fixed

- Panic when parsing GPSInfo #37

## nom-exif v2.5.0

### Added

- `EntryValue::NaiveDateTime` #38

### Fixed

- Try to repair broken `OffsetTimeOriginal`/`OffsetTime`,
  if it's not repairable, then parse time as an `NaiveDateTime`. #39

## nom-exif v2.4.3

### Fixed

- Ignore undetermined language flag (0x55c4) when parsing `TrackInfoTag::Author` info #36

## nom-exif v2.4.2

### Fixed

- Fix: panic: skip a large number of bytes #28 #26
- Fix: panic when parsing moov/trak #29

## nom-exif v2.4.1

### Fixed

- Fix: Panic for Nikon D200 raw NEF file (parse_ifd_entry_header) #34

## nom-exif v2.4.0

### Added

- Add `TrackInfoTag::Author` #36
  - Parse `udta/auth` or `com.apple.quicktime.author` info from `mp4`/`mov` files

## nom-exif v2.3.1

[v2.3.0..v2.3.1](https://github.com/mindeng/nom-exif/compare/v2.3.0..v2.3.1)

### Fixed

- parse GPS strings correctly #35

## nom-exif v2.3.0

[v2.2.1..v2.3.0](https://github.com/mindeng/nom-exif/compare/v2.2.1..v2.3.0)

### Added

- `EntryValue::U8Array`

### Fixed

- Doesn't recognize DateTimeOriginal tag in file. #33
- Update to avoid Rust 1.83 lints #30
- println! prints lines to output #27
- range start index panic in src/exif/exif_iter.rs #25
- Panic range end index 131151 out of range for slice of length 129 #24
- Memory allocation failed when decoding invalid file #22
- assertion failed: data.len() >= 6 when checking broken exif file #21
- Panic depth shouldn't be greater than 1 #20
- freeze when checking broken file #19

## nom-exif v2.2.1

[v2.1.1..v2.2.1](https://github.com/mindeng/nom-exif/compare/v2.1.1..v2.2.1)

### Added

- Added support for RAF (Fujifilm RAW) file type.

## nom-exif v2.1.1

[v2.1.0..v2.1.1](https://github.com/mindeng/nom-exif/compare/v2.1.0..v2.1.1)

### Fixed

- Fix endless loop caused by some broken images.

## nom-exif v2.1.0

[v2.0.2..v2.1.0](https://github.com/mindeng/nom-exif/compare/v2.0.2..v2.1.0)

### Added

- Type alias: `IRational`
- Supports 32-bit target platforms, e.g.: `armv7-linux-androideabi`

### Fixed

- Fix compiling errors on 32-bit target platforms

## nom-exif v2.0.2

[v2.0.0..v2.0.2](https://github.com/mindeng/nom-exif/compare/v2.0.0..v2.0.2)

### Changed

- Deprecated
  - `parse_mov_metadata`: Please use `MediaParser` instead.

## nom-exif v2.0.0

[v1.5.2..v2.0.0](https://github.com/mindeng/nom-exif/compare/v1.5.2..v2.0.0)

### Added

- Support more file types
  - `*.tiff`
  - `*.webm`
  - `*.mkv`, `*.mka`
  - `*.3gp`

- rexiftool
  - Add `--debug` command line parameter for printing and saving debug logs

- Structs
  - `MediaSource`
  - `MediaParser`
  - `AsyncMediaSource`
  - `AsyncMediaParser`
  - `TrackInfo`
  
- Enums
  - `TrackInfoTag`
  
- Type Aliases
  - `URational`
  
### Changed

- Deprecated
  - `parse_exif`     : Please use `MediaParser` instead.
  - `parse_exif_async` : Please use `MediaParser` instead.
  - `parse_heif_exif` : Please use `MediaParser` instead.
  - `parse_jpeg_exif` : Please use `MediaParser` instead.
  - `parse_metadata` : Please use `MediaParser` instead.
  - `FileFormat`     : Please use `MediaSource` instead.

## nom-exif v1.5.2

[v1.5.1..v1.5.2](https://github.com/mindeng/nom-exif/compare/v1.5.1..v1.5.2)

### Fixed

- Bug fixed: "Box is too big" error when parsing some mov/mp4 files

  No need to limit box body size when parsing/traveling box headers, only need
  to do that limitation when parsing box body (this restriction is necessary
  for the robustness of the program). Additionally, I also changed the size
  limit on the box body to a more reasonable value.

## nom-exif v1.5.1

[v1.5.0..v1.5.1](https://github.com/mindeng/nom-exif/compare/v1.5.0..v1.5.1)

### Added

- `ParsedExifEntry`

### Changed

- `ExifTag::Unknown`

## nom-exif v1.5.0

[v1.4.1..v1.5.0](https://github.com/mindeng/nom-exif/compare/v1.4.1..v1.5.0)

### Added

- `parse_exif`
- `parse_exif_async`
- `ExifIter`
- `GPSInfo`
- `LatLng`
- `FileFormat`
- `Exif::get`
- `Exif::get_by_tag_code`
- `EntryValue::URationalArray`
- `EntryValue::IRationalArray`
- `Error::InvalidEntry`
- `Error::EntryHasBeenTaken`

### Changed

- `Exif::get_values` deprecated
- `Exif::get_value` deprecated
- `Exif::get_value_by_tag_code` deprecated
- `Error::NotFound` deprecated

## nom-exif v1.4.1

[v1.4.0..v1.4.1](https://github.com/mindeng/nom-exif/compare/v1.4.0..v1.4.1)

### Performance Improved

- Avoid data copying when extracting moov body.

### Added

- impl `Send` + `Sync` for `Exif`, so we can use it in multi-thread environment

## nom-exif v1.4.0

[v1.3.0..v1.4.0](https://github.com/mindeng/nom-exif/compare/v1.3.0..v1.4.0)

### Performance Improved

- Avoid data copying during parsing IFD entries.

## nom-exif v1.3.0

[v1.2.6..v1.3.0](https://github.com/mindeng/nom-exif/compare/v1.2.6..v1.3.0)

### Changed

- Introduce tracing, and replace printing with tracing.

## nom-exif v1.2.6

[v1.2.5..v1.2.6](https://github.com/mindeng/nom-exif/compare/v1.2.5..v1.2.6)

### Fixed

- Bug fixed: [A broken JPEG file - Library cannot read it, that exiftool reads
  properly #2](https://github.com/mindeng/nom-exif/issues/2)
- Bug fixed: [Another Unsupported MP4 file
  #7](https://github.com/mindeng/nom-exif/issues/7#issuecomment-2226853761)

### Internal

- Remove redundant `fn open_sample` definitions in test cases.
- Use `read_sample` instead of `open_sample` when possible.

## nom-exif v1.2.5

[v1.2.4..v1.2.5](https://github.com/mindeng/nom-exif/compare/v1.2.4..v1.2.5)

### Fixed

- Bug fixed: [Unsupported mov file?
  #7](https://github.com/mindeng/nom-exif/issues/7)

### Internal

- Change `travel_while` to return a result of optional `BoxHolder`, so we can
  distinguish whether it is a parsing error or just not found.

## nom-exif v1.2.4

[8c00f1b..v1.2.4](https://github.com/mindeng/nom-exif/compare/8c00f1b..v1.2.4)

### Improved

- **Compatibility** has been greatly improved: compatible brands in ftyp box
  has been checked, and now it can support various compatible MP4/MOV files.

## nom-exif v1.2.3

[2861cbc..8c00f1b](https://github.com/mindeng/nom-exif/compare/2861cbc..8c00f1b)

### Fixed

- **All** clippy warnings has been fixed!

### Changed

- **Deprecated** some less commonly used APIs and introduced several new ones,
  mainly to satisfy clippy requirements, e.g.:

  - `GPSInfo.to_iso6709` -> `format_iso6709`
  - `URational.to_float` -> `as_float`
  
  See commit [8c5dc26](https://github.com/mindeng/nom-exif/commit/8c5dc26).

## nom-exif v1.2.2

[9b7fdf7..2861cbc](https://github.com/mindeng/nom-exif/compare/9b7fdf7..2861cbc)

### Added

- **Fuzz testing**: Added *afl-fuzz* for fuzz testing.

### Changed

### Fixed

- **Robustness improved**: Fixed all crash issues discovered during fuzz
  testing.
- **Clippy warnings**: Checked with the latest clippy and fixed almost all of
  the warnings.

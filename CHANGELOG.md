# Changelog

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

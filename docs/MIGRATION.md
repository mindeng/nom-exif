# Migrating from nom-exif v2 to v3

This is the canonical, user-facing migration guide for the v3.0.0 breaking
release. Every row in this document is exercised by `tests/migration_guide.rs`,
which compiles against the public API as a downstream crate would.

If a row here is wrong, the test will fail. If you change the public surface,
update the corresponding row here, the entry in `CHANGELOG.md`, and the
matching test — the three artifacts are meant to stay in lock-step.

For internal architecture decisions and design rationale, see
`docs/V3_API_DESIGN.md`.

---

## 1. Entry & parsing

| v2 | v3 |
|----|-----|
| `MediaSource::file_path(p)` | `MediaSource::open(p)` (or `read_exif(p)`) |
| `MediaSource::tcp_stream(s)` | `MediaSource::unseekable(s)` |
| `ms.has_exif()` | `ms.kind() == MediaKind::Image` |
| `ms.has_track()` | `ms.kind() == MediaKind::Track` (note: `Video` was renamed `Track`; pure-audio containers like `.mka` fall under this kind too) |
| `parser.parse::<_, _, ExifIter>(ms)` | `parser.parse_exif(ms)` |
| `parser.parse::<_, _, TrackInfo>(ms)` | `parser.parse_track(ms)` |
| `MediaSource<R, S>` (two type parameters) | `MediaSource<R>` (the `S` parameter was deleted) |
| Implicit seek-fallback-to-read (the v2 `Skip` trait's `bool` return) | Removed — seek failure now returns `Error::Io` |

New convenience helpers (no v2 equivalent):

```rust
let exif = nom_exif::read_exif("photo.jpg")?;       // one-shot eager
let iter = nom_exif::read_exif_iter("photo.jpg")?;  // one-shot lazy
let info = nom_exif::read_track("video.mp4")?;
let meta = nom_exif::read_metadata("file.heic")?;   // returns Metadata::{Exif,Track}
```

## 2. Errors

| v2 | v3 |
|----|-----|
| `Error::ParseFailed(Box<dyn Error>)` | Structured variants: `Malformed { kind, message }`, `UnexpectedEof`, `UnsupportedFormat` |
| `Error::IOError(e)` | `Error::Io(e)` (renamed for brevity) |
| `From<&str> for Error`, `From<String> for Error` | Removed — use a structured variant |
| `EntryError` (crate-private enum with `String` payloads) | Public enum with three structured variants: `Truncated`, `InvalidShape`, `InvalidValue(&'static str)` |
| No entry-level → file-level error propagation | `From<EntryError> for Error` (maps to `Malformed { kind: IfdEntry, .. }`) |
| Conversion errors scattered across `crate::Error` and standalone types | Unified into `ConvertError` (a peer type — `ConvertError` and `Error` do not convert into each other) |

## 3. EntryValue accessors

| v2 | v3 |
|----|-----|
| `value.as_time_components() -> Option<(NaiveDateTime, Option<FixedOffset>)>` | `value.as_datetime() -> Option<ExifDateTime>`, where `ExifDateTime` is `Aware`/`Naive` with `aware()` / `into_naive()` / `or_offset(fallback)` accessors |
| `value.as_u8array()` | `value.as_u8_slice()` |
| `value.to_u8array()` | Removed — use `as_u8_slice().map(<[u8]>::to_vec)` |
| Missing `as_i64` / `as_f64` / `as_u16_slice` / etc. | Filled in. `as_f32` is intentionally **not** provided (`as_f64` covers it via widening); `as_i8` / `as_i16` are present even though those widths are rare in modern EXIF |

## 4. ExifTag

| v2 | v3 |
|----|-----|
| `ExifTag::try_from(0x010f)` | `ExifTag::from_code(0x010f)` |
| `<&str as From<ExifTag>>::from(t)` | `t.name()` or `t.to_string()` |
| No `&str → ExifTag` | `ExifTag::from_str("Make")` (impl `FromStr`) |

## 5. Exif / ExifIter

| v2 | v3 |
|----|-----|
| `exif.get_gps_info()? -> Option<GPSInfo>` (Result-wrapped) | `exif.gps_info() -> Option<&GPSInfo>` |
| `exif.get_by_ifd_tag_code(0, 0x0110)` | `exif.get_by_code(IfdIndex::MAIN, 0x0110)` |
| `exif.get_by_ifd_tag_code(ifd, ExifTag::Make.code())` | `exif.get_in(IfdIndex::new(ifd), ExifTag::Make)` (`IfdIndex` field is private — use `new()` or the `MAIN`/`THUMBNAIL` constants) |
| Cannot iterate over `Exif` | `exif.iter()` (filter by IFD: `exif.iter().filter(\|e\| e.ifd == IfdIndex::MAIN)`) |
| Cannot retrieve per-entry errors from `Exif` | `exif.errors() -> &[(IfdIndex, TagOrCode, EntryError)]` |
| `ParsedExifEntry` (the lazy iter's yield type) | Renamed `ExifIterEntry` (paired with `ExifIter`) |
| `entry.tag()` + `entry.tag_code()` | `entry.tag() -> TagOrCode` |
| `entry.take_value()` | `entry.into_result().ok()` (or clone first) |
| `entry.take_result()` (panic risk) | `entry.into_result()` (consumes `self`) |
| `iter.clone_and_rewind()` | `iter.clone_rewound()` (or `let mut x = iter.clone(); x.rewind();`) |
| `iter.parse_gps_info()` | `iter.parse_gps()` |
| (none) | New: `Exif::has_embedded_media()` / `ExifIter::has_embedded_media()` — true when the container embeds an unparsed extra media stream (e.g. HEIC Live Photo) |

## 6. GPSInfo

```rust
// v2
let g = exif.get_gps_info()?.unwrap();
if g.latitude_ref == 'N' { /* ... */ }
let alt_above = g.altitude_ref == 0;

// v3
let g = exif.gps_info().unwrap();
if matches!(g.latitude_ref, LatRef::North) { /* ... */ }
let alt_above = matches!(g.altitude, Altitude::AboveSeaLevel(_));
```

The `char` / `u8` GPS fields are now strongly-typed enums: `LatRef`,
`LonRef`, `Altitude`, `Speed`, `SpeedUnit`.

## 7. Rational / LatLng

```rust
// v2
let r = URational(1, 2);
let f = r.0 as f64 / r.1 as f64;

// v3
let r = URational::new(1, 2);
let f = r.to_f64().unwrap();   // handles denominator == 0

// IRational → URational (v2 silently truncated negatives; v3 fails explicitly)
let u: URational = ir.try_into()?;  // ConvertError::NegativeRational
```

```rust
// LatLng from decimal degrees
// v2
let p = LatLng::from(43.5_f64);  // internal unwrap could panic

// v3
let p = LatLng::try_from_decimal_degrees(43.5)?;  // ConvertError::InvalidDecimalDegrees
```

`URational` / `IRational` tuple-struct field access (`.0` / `.1`) is gone;
use `.numerator()` / `.denominator()`.

## 8. Async

```rust
// v2
let mut parser = AsyncMediaParser::new();
let ms = AsyncMediaSource::file_path("a.jpg").await?;
let iter: ExifIter = parser.parse(ms).await?;

// v3
let mut parser = MediaParser::new();
let ms = AsyncMediaSource::open("a.jpg").await?;
let iter = parser.parse_exif_async(ms).await?;

// Or, one-shot:
let exif = nom_exif::read_exif_async("a.jpg").await?;
```

`AsyncMediaParser` is gone — there is one `MediaParser` with feature-gated
async methods (`parse_exif_async` / `parse_track_async`). The async surface
is enabled by `feature = "tokio"`.

## 9. Cargo features

| v2 | v3 |
|----|-----|
| `nom-exif = { version = "2", features = ["async"] }` | `nom-exif = { version = "3", features = ["tokio"] }` |
| `nom-exif = { version = "2", features = ["json_dump"] }` | `nom-exif = { version = "3", features = ["serde"] }` |

Feature names only — semantics and functionality are unchanged.

## 10. TrackInfo / TrackInfoTag

| v2 | v3 |
|----|-----|
| `TrackInfoTag::ImageWidth` / `ImageHeight` | `TrackInfoTag::Width` / `Height` (the `Image` prefix is wrong in a video/audio container; aligns with Matroska's `PixelWidth`/`PixelHeight` and ISOBMFF's `width`/`height`. `ExifTag::ImageWidth`/`ImageHeight` are unchanged — `Image` is correct in EXIF context) |
| `info.get_gps_info() -> Option<GPSInfo>` (Result-wrapped) | `info.gps_info() -> Option<&GPSInfo>` (parallels `Exif::gps_info`) |
| `<&str as From<TrackInfoTag>>::from(t)` | `t.name()` or `t.to_string()` |
| `TryFrom<&str> for TrackInfoTag` (with `UnknownTrackInfoTag` error) | `TrackInfoTag::from_str("Make")` (impl `FromStr`, `Err = ConvertError`) |
| `From<BTreeMap<TrackInfoTag, EntryValue>> for TrackInfo` | Removed — internal construction detail, not part of the public API |
| `IntoIterator for TrackInfo` (owned iteration) | Removed — use `info.iter()` instead |
| (none) | New: `TrackInfo::has_embedded_media()` (always `false` in 3.0.0; reserved for parity with `Exif::has_embedded_media`) |

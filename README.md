# Nom-Exif

[![crates.io](https://img.shields.io/crates/v/nom-exif.svg)](https://crates.io/crates/nom-exif)
[![Documentation](https://docs.rs/nom-exif/badge.svg)](https://docs.rs/nom-exif)
[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mindeng/nom-exif/actions/workflows/rust.yml/badge.svg)](https://github.com/mindeng/nom-exif/actions)

`nom-exif` is an Exif/metadata parsing library written in pure Rust with
[nom](https://github.com/rust-bakery/nom).

## Supported File Types

- Image
  - .heic, .heif, etc.
  - .jpg, .jpeg
  - .tiff, .tif, .iiq (Phase One IIQ images), etc.
  - .RAF (Fujifilm RAW)
  - .CR3 (Canon RAW)
- Video/Audio
  - ISO base media file format (ISOBMFF): .mp4, .mov, .3gp, etc.
  - Matroska based file format: .webm, .mkv, .mka, etc.

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

For batch processing, build a `MediaParser` once and reuse its buffer
across calls:

```rust
use nom_exif::{MediaKind, MediaParser, MediaSource, ExifTag, TrackInfoTag};

let mut parser = MediaParser::new();

let files = [
    "./testdata/exif.heic",
    "./testdata/exif.jpg",
    "./testdata/meta.mov",
];

for f in files {
    let ms = MediaSource::open(f)?;
    match ms.kind() {
        MediaKind::Image => {
            let iter = parser.parse_exif(ms)?;
            let exif: nom_exif::Exif = iter.into();
            let _ = exif.get(ExifTag::Make);
        }
        MediaKind::Track => {
            let info = parser.parse_track(ms)?;
            let _ = info.get(TrackInfoTag::Make);
        }
    }
}
# Ok::<(), nom_exif::Error>(())
```

`MediaSource` accepts any `Read` (or `Read + Seek`):

- `MediaSource::open(path)` — convenience for files.
- `MediaSource::seekable(reader)` — any `Read + Seek` source.
- `MediaSource::unseekable(reader)` — `Read`-only source (e.g. a network
  stream); slower for formats that store metadata at the end of the file
  (such as `.mov`).

## Two API styles for Exif

The library exposes both **eager** and **lazy** views of EXIF metadata.

```rust
use nom_exif::{read_exif, read_exif_iter, ExifTag};

// Eager — easiest. Get-by-tag, parsed up front.
let exif = read_exif("./testdata/exif.jpg")?;
let make = exif.get(ExifTag::Make).and_then(|v| v.as_str());

// Lazy — finer-grained. Parse-on-demand, per-entry errors visible.
let iter = read_exif_iter("./testdata/exif.jpg")?;
for entry in iter {
    let _tag = entry.tag();          // TagOrCode (Tag(...) or Unknown(code))
    let _ifd = entry.ifd();          // IfdIndex
    let _ = entry.into_result();      // Result<EntryValue, EntryError>
}
# Ok::<(), nom_exif::Error>(())
```

## Async API

Enable the `tokio` feature in your `Cargo.toml`:

```toml
[dependencies]
nom-exif = { version = "3", features = ["tokio"] }
```

Then use the `_async` helpers, or call `parse_exif_async` /
`parse_track_async` on a `MediaParser` directly:

```rust,no_run
# #[cfg(feature = "tokio")]
# async fn demo() -> nom_exif::Result<()> {
use nom_exif::{read_exif_async, MediaParser, AsyncMediaSource};

// One-shot:
let exif = read_exif_async("./testdata/exif.jpg").await?;

// Reusable:
let mut parser = MediaParser::new();
let ms = AsyncMediaSource::open("./testdata/exif.jpg").await?;
let iter = parser.parse_exif_async(ms).await?;
# let _ = (exif, iter); Ok(())
# }
```

## GPS Info

`Exif` and `TrackInfo` both expose `gps_info()`. `ExifIter` adds
`parse_gps()` for early termination once GPS tags have been read.

```rust
use nom_exif::{read_exif, LatRef, LonRef, Altitude};

let exif = read_exif("./testdata/exif.heic")?;
if let Some(g) = exif.gps_info() {
    let _ = matches!(g.latitude_ref, LatRef::North | LatRef::South);
    let _ = matches!(g.longitude_ref, LonRef::East | LonRef::West);
    let _ = matches!(g.altitude, Altitude::AboveSeaLevel(_) | Altitude::BelowSeaLevel(_));
    let _iso = g.to_iso6709();
}
# Ok::<(), nom_exif::Error>(())
```

## Migration from v2

See `docs/V3_API_DESIGN.md` §5 for the full migration table. Hot items:

- `MediaSource::file_path(p)` → `MediaSource::open(p)` or `read_exif(p)`.
- `parser.parse::<_,_,ExifIter>(ms)` → `parser.parse_exif(ms)`.
- `parser.parse::<_,_,TrackInfo>(ms)` → `parser.parse_track(ms)`.
- `entry.take_result()` (panicky) → `entry.into_result()` (consumes self).
- `iter.parse_gps_info()` → `iter.parse_gps()`.
- `info.get_gps_info()` → `info.gps_info()` (returns `Option<&GPSInfo>`).
- `g.latitude_ref == 'N'` → `matches!(g.latitude_ref, LatRef::North)`.
- Cargo features: `async` → `tokio`, `json_dump` → `serde`.

## CLI Tool `rexiftool`

### Human Readable Output

`cargo run --example rexiftool testdata/meta.mov`:

```text
Make                            => Apple
Model                           => iPhone X
Software                        => 12.1.2
CreateDate                      => 2024-02-02T08:09:57+00:00
DurationMs                      => 500
ImageWidth                      => 720
ImageHeight                     => 1280
GpsIso6709                      => +27.1281+100.2508+000.000/
```

Pass `--debug` to enable tracing logs:

`cargo run --example rexiftool -- --debug ./testdata/meta.mov`

### JSON Dump

`cargo run --features serde --example rexiftool testdata/meta.mov -j`:

```text
{
  "ImageWidth": "720",
  "Software": "12.1.2",
  "ImageHeight": "1280",
  "Make": "Apple",
  "GpsIso6709": "+27.1281+100.2508+000.000/",
  "CreateDate": "2024-02-02T08:09:57+00:00",
  "Model": "iPhone X",
  "DurationMs": "500"
}
```

### Parsing Files in a Directory

`rexiftool` also supports batch parsing of all files in a folder
(non-recursive).

`cargo run --example rexiftool testdata/`:

```text
File: "testdata/embedded-in-heic.mov"
------------------------------------------------
Make                            => Apple
Model                           => iPhone 15 Pro
Software                        => 17.1
CreateDate                      => 2023-11-02T12:01:02+00:00
DurationMs                      => 2795
ImageWidth                      => 1920
ImageHeight                     => 1440
GpsIso6709                      => +22.5797+113.9380+028.396/

File: "testdata/exif.jpg"
------------------------------------------------
ImageWidth                      => 3072
Model                           => vivo X90 Pro+
ImageHeight                     => 4096
ModifyDate                      => 2023-07-09T20:36:33+08:00
...
```

## Fuzz Testing

The project uses [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
(libFuzzer) for fuzz testing. Requires nightly Rust.

**Run the fuzzer:**

```sh
# Use testdata/ as seed corpus, write new corpus to fuzz/corpus/media_parser/
cargo +nightly fuzz run media_parser fuzz/corpus/media_parser/ testdata/
```

**Reproduce a crash:**

```sh
cargo +nightly fuzz run media_parser fuzz/artifacts/media_parser/<crash-file>
```

**Minimize a crash input:**

```sh
cargo +nightly fuzz tmin media_parser fuzz/artifacts/media_parser/<crash-file>
```

## Changelog

[CHANGELOG.md](CHANGELOG.md)

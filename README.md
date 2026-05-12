# Nom-Exif

[![crates.io](https://img.shields.io/crates/v/nom-exif.svg)](https://crates.io/crates/nom-exif)
[![Documentation](https://docs.rs/nom-exif/badge.svg)](https://docs.rs/nom-exif)
[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mindeng/nom-exif/actions/workflows/rust.yml/badge.svg)](https://github.com/mindeng/nom-exif/actions)
[![codecov](https://codecov.io/gh/mindeng/nom-exif/graph/badge.svg)](https://codecov.io/gh/mindeng/nom-exif)

`nom-exif` is a pure Rust library for **both image EXIF and video / audio
track metadata** through a single unified API. Built on
[nom](https://github.com/rust-bakery/nom).

## Highlights

- Pure Rust — no FFmpeg, no libexif, no system deps; cross-compiles
  cleanly.
- Image **and** video / audio in one crate — `MediaParser` dispatches to
  the right backend by detected MIME, no per-format wrappers.
- **Motion Photo** support — Pixel and Samsung Motion Photos (JPEG with
  an embedded MP4) are detected automatically; `parse_track` extracts
  the embedded video's track metadata.
- RAW format support — Canon CR3, Fujifilm RAF, Phase One IIQ,
  alongside JPEG / HEIC / TIFF.
- Three input modes — files, arbitrary `Read` / `Read + Seek` (network
  streams, pipes), or in-RAM bytes (WASM, mobile, HTTP proxies).
- Sync and async unified under one `MediaParser`.
- Eager (`Exif`, get-by-tag) or lazy (`ExifIter`, parse-on-demand) —
  per-entry errors surface in both modes (`Exif::errors()` /
  per-iter `Result`), so one bad tag doesn't poison the parse.
- Allocation-frugal — parser buffer is recycled across calls; sub-IFDs
  share the same allocation (no deep copies).
- Fuzz-tested with `cargo-fuzz` against malformed and adversarial input.

## Supported File Types

- **Image**: JPEG, PNG, HEIC/HEIF, AVIF, TIFF, Phase One IIQ, Fujifilm RAF, Canon CR3
- **Video/Audio**: MP4, MOV, 3GP (ISOBMFF); MKV, WEBM, MKA (Matroska)

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

## Motion Photos

Pixel and Samsung phones store **Motion Photos** as a single JPEG with a
short MP4 video appended after the image data. `parse_exif` reads the
photo's EXIF as usual and sets a flag when it sees the
`GCamera:MotionPhoto="1"` XMP signal; `parse_track` on the same source
then extracts the embedded MP4's metadata.

```rust
use nom_exif::{MediaParser, MediaSource, TrackInfoTag};

let path = "PXL_20240101_120000000.MP.jpg";
let mut parser = MediaParser::new();

// 1. Parse the still image as usual.
let iter = parser.parse_exif(MediaSource::open(path)?)?;
println!("has_embedded_track = {}", iter.has_embedded_track());

// 2. If true, re-open the source (parse_exif consumed it) and call
//    parse_track to extract the embedded MP4's metadata.
if iter.has_embedded_track() {
    let track = parser.parse_track(MediaSource::open(path)?)?;
    println!("video {:?}x{:?}",
        track.get(TrackInfoTag::Width),
        track.get(TrackInfoTag::Height));
}
# Ok::<(), nom_exif::Error>(())
```

`has_embedded_track` is **content-detected**, not a MIME-level guess — a
plain JPEG without the Motion Photo XMP returns `false` and `parse_track`
returns `Error::TrackNotFound`.

**Coverage**: Pixel/Google Motion Photos and Samsung Galaxy Motion
Photos that use the Adobe XMP Container directory format (modern Pixel
including Ultra HDR, modern Galaxy JPEGs).

## In-Memory Bytes

When the payload is already in RAM (decoded HTTP body, WASM-loaded
asset, mobile-cached blob), use `MediaSource::from_memory` to skip the
`File` / `Read` round-trip. Memory mode is **zero-copy**: the underlying
allocation is shared with the returned `Exif` / `ExifIter` / `TrackInfo`
via `bytes::Bytes` reference counting.

```rust
use nom_exif::{MediaSource, MediaParser, ExifTag};

let raw: Vec<u8> = std::fs::read("./testdata/exif.jpg")?;
let ms = MediaSource::from_memory(raw)?;
let mut parser = MediaParser::new();
let iter = parser.parse_exif(ms)?;
let exif: nom_exif::Exif = iter.into();
let make = exif.get(ExifTag::Make).and_then(|v| v.as_str());
# let _ = make; Ok::<(), nom_exif::Error>(())
```

`MediaSource::from_memory` accepts anything convertible into
`bytes::Bytes`: `Vec<u8>`, `&'static [u8]`, `Bytes`, and HTTP-body types
that implement `Into<Bytes>` directly.

## Format-Specific Metadata (`parse_image_metadata`)

Some image formats carry metadata that doesn't fit the EXIF/IFD model
— PNG `tEXt` chunks are the headline example. The new (v3.3+)
`MediaParser::parse_image_metadata` returns a structured
`ImageMetadata { exif, format }` covering both:

```rust
use nom_exif::{MediaParser, MediaSource, ImageFormatMetadata};

let mut parser = MediaParser::new();
let ms = MediaSource::open("./testdata/exif.png")?;
let img = parser.parse_image_metadata(ms)?;

if let Some(ImageFormatMetadata::Png(text_chunks)) = img.format {
    let _title = text_chunks.get("Title");
    let _software = text_chunks.get("Software");
}
# Ok::<(), nom_exif::Error>(())
```

`img.exif` is the standard `Option<ExifIter>` — convert to `Exif`
with `.into()` and read tags as in any other example.

For PNG specifically, this also captures legacy EXIF embedded in
`Raw profile type exif` / `Raw profile type APP1` `tEXt` chunks
(ImageMagick / Photoshop pattern) — those are transparently
hex-decoded and merged into `img.exif`. The original `tEXt` entry
is still visible via `img.format`.

`parse_image_metadata` accepts the same source types as `parse_exif`:
files, in-memory bytes (via `MediaSource::from_memory`), and async
sources. The top-level `read_image_metadata` convenience helper is
deferred to v4 (alongside the planned `Metadata` enum redesign).

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

v3.0.0 reshapes the public API end-to-end. The full migration guide lives
in [`docs/MIGRATION.md`](docs/MIGRATION.md) — every row there is exercised
by `tests/migration_guide.rs`. A few high-traffic items:

- `MediaSource::file_path(p)` → `MediaSource::open(p)` or `read_exif(p)`.
- `parser.parse::<_,_,ExifIter>(ms)` → `parser.parse_exif(ms)`.
- `parser.parse::<_,_,TrackInfo>(ms)` → `parser.parse_track(ms)`.
- `entry.take_result()` (panicky) → `entry.into_result()` (consumes self).
- `iter.parse_gps_info()` → `iter.parse_gps()`.
- `info.get_gps_info()` → `info.gps_info()` (returns `Option<&GPSInfo>`).
- `g.latitude_ref == 'N'` → `matches!(g.latitude_ref, LatRef::North)`.
- Cargo features: `async` → `tokio`, `json_dump` → `serde`.

## CLI Tool `rexiftool`

`rexiftool` is a companion CLI built on `nom-exif`, published as a
separate crate:

```sh
cargo install rexiftool
rexiftool photo.heic        # key => value
rexiftool photo.heic -j     # JSON
rexiftool ./photos/         # batch (non-recursive)
```

Pre-built binaries (macOS Intel / Apple Silicon, Linux x86_64,
Windows x86_64) are attached to each `rexiftool-v*` GitHub release.
Full usage docs: [crates/rexiftool/README.md](crates/rexiftool/README.md)
or [crates.io/crates/rexiftool](https://crates.io/crates/rexiftool).

## Contributing

Enable the repository's pre-commit hook once per clone so commits that
would fail `cargo fmt --check` in CI are rejected locally:

```sh
git config core.hooksPath .githooks
```

The hook lives in `.githooks/pre-commit` and runs `cargo fmt --check`
(sub-second). Bypass with `git commit --no-verify` for emergencies.

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

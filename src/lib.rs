//! `nom-exif` is a pure Rust library for **both image EXIF and
//! video / audio track metadata** through a single unified API.
//!
//! # Highlights
//!
//! - Pure Rust — no FFmpeg, no libexif, no system deps; cross-compiles
//!   cleanly.
//! - Image **and** video / audio in one crate — [`MediaParser`] dispatches
//!   to the right backend by detected MIME, no per-format wrappers.
//! - RAW format support — Canon CR3, Fujifilm RAF, Phase One IIQ,
//!   alongside JPEG / HEIC / TIFF.
//! - Three input modes — files, arbitrary `Read` / `Read + Seek`
//!   (network streams, pipes), or in-RAM bytes (WASM, mobile, HTTP
//!   proxies).
//! - Sync and async unified under one [`MediaParser`].
//! - Eager ([`Exif`], get-by-tag) or lazy ([`ExifIter`], parse-on-demand)
//!   — per-entry errors surface in both modes ([`Exif::errors`] /
//!   per-iter `Result`), so one bad tag doesn't poison the parse.
//! - Allocation-frugal — parser buffer is recycled across calls;
//!   sub-IFDs share the same allocation (no deep copies).
//! - Fuzz-tested with `cargo-fuzz` against malformed and adversarial input.
//!
//! # Quick start
//!
//! For a one-shot read, use the helpers:
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
//! # Reading from in-memory bytes
//!
//! When the payload is already in RAM (WASM, mobile, HTTP proxy, decoded
//! response body), use [`MediaSource::from_memory`] to skip the `File` /
//! `Read` round-trip entirely. Memory mode is **zero-copy**: the underlying
//! allocation is shared with the returned [`Exif`] / [`ExifIter`] /
//! [`TrackInfo`] via [`bytes::Bytes`] reference counting.
//!
//! ```rust
//! use nom_exif::{MediaSource, MediaParser, ExifTag};
//!
//! let raw = std::fs::read("./testdata/exif.jpg")?;
//! let ms = MediaSource::from_memory(raw)?;
//! let mut parser = MediaParser::new();
//! let iter = parser.parse_exif(ms)?;
//! let exif: nom_exif::Exif = iter.into();
//! assert_eq!(exif.get(ExifTag::Make).and_then(|v| v.as_str()), Some("vivo"));
//! # Ok::<(), nom_exif::Error>(())
//! ```
//!
//! For batch processing of many in-memory payloads, build a [`MediaParser`]
//! once and call [`MediaParser::parse_exif`] / [`MediaParser::parse_track`]
//! with sources built via [`MediaSource::from_memory`] per payload.
//!
//! v3.0-style API (deprecated since v3.3): the top-level
//! `read_exif_from_bytes` family and `MediaSource::<()>::from_bytes`
//! still compile but produce deprecation warnings. Migrate to
//! `MediaSource::from_memory` + `parse_exif` / `read_exif`.
//!
//! # API surface
//!
//! - **One-shot helpers**: [`read_exif`], [`read_exif_iter`], [`read_track`], [`read_metadata`]
//!   for files; [`read_exif_from_bytes`], [`read_exif_iter_from_bytes`],
//!   [`read_track_from_bytes`], [`read_metadata_from_bytes`] for in-memory bytes (deprecated since v3.3).
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
//! See `docs/MIGRATION.md` for the v2 → v3 migration guide and
//! `docs/V3_API_DESIGN.md` for the internal design contract.
//!
//! # Cargo features
//!
//! - `tokio` — async API via tokio (`AsyncMediaSource`, `read_*_async`,
//!   `MediaParser::parse_*_async`).
//! - `serde` — derives `Serialize`/`Deserialize` on the public types.
//!
//! # Embedded media tracks
//!
//! Some images embed a media track that `parse_exif` doesn't surface —
//! most commonly **Pixel/Google Motion Photo** JPEGs, which carry a short
//! MP4 video appended after the JPEG image data. The
//! [`Exif::has_embedded_track`] / [`ExifIter::has_embedded_track`] flags
//! are set by `parse_exif` when it observes a concrete content signal
//! (e.g. the `GCamera:MotionPhoto="1"` XMP attribute). When the flag is
//! `true`, call [`MediaParser::parse_track`] on the same source to
//! extract the embedded MP4's metadata — `parse_track` automatically
//! locates and parses the trailer.
//!
//! ```no_run
//! use nom_exif::{MediaParser, MediaSource};
//! let mut parser = MediaParser::new();
//! let path = "PXL_20240101_120000000.MP.jpg";
//! let iter = parser.parse_exif(MediaSource::open(path)?)?;
//! if iter.has_embedded_track() {
//!     // Re-open: MediaSource is consumed by parse_exif.
//!     let track = parser.parse_track(MediaSource::open(path)?)?;
//!     // ...
//! }
//! # Ok::<(), nom_exif::Error>(())
//! ```
//!
//! **Coverage**: Pixel/Google Motion Photos and Samsung Galaxy Motion
//! Photos that use the Adobe XMP Container directory format (modern
//! Pixel including Ultra HDR, modern Galaxy JPEGs).
//!
//! The pre-3.1 names (`has_embedded_media`) are `#[deprecated]` aliases
//! that forward to the new methods.

pub use parser::{MediaKind, MediaParser, MediaSource};
pub use video::{TrackInfo, TrackInfoTag};

#[cfg(feature = "tokio")]
pub use parser_async::AsyncMediaSource;

pub use exif::gps::{Altitude, LatRef, LonRef, Speed, SpeedUnit};
pub use exif::{
    Exif, ExifEntry, ExifIter, ExifIterEntry, ExifTag, GPSInfo, IfdIndex, LatLng, TagOrCode,
};
pub use values::{EntryValue, ExifDateTime, IRational, Rational, URational};

pub use error::{ConvertError, EntryError, Error, MalformedKind};

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
    pub use crate::{read_exif, read_metadata, read_track};
    pub use crate::{
        EntryValue, Error, Exif, ExifIter, ExifTag, GPSInfo, IfdIndex, MalformedKind, MediaKind,
        MediaParser, MediaSource, Metadata, Result, TrackInfo, TrackInfoTag,
    };
}

/// Crate-wide convenience alias for `std::result::Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;

/// One-shot result of [`read_metadata`]: either Exif (image) or TrackInfo
/// (video/audio). Closed enum — see spec §8.6 for why there's no `Both`
/// variant.
#[derive(Debug, Clone)]
pub enum Metadata {
    Exif(Exif),
    Track(TrackInfo),
}

use std::path::Path;

/// Read EXIF metadata from a file in a single call.
///
/// For batch processing, prefer constructing a [`MediaParser`] once and
/// reusing its parse buffer via [`MediaParser::parse_exif`].
pub fn read_exif(path: impl AsRef<Path>) -> Result<Exif> {
    let iter = read_exif_iter(path)?;
    Ok(iter.into())
}

/// Read EXIF metadata from a file as a lazy iterator. Like [`read_exif`]
/// but returns an [`ExifIter`] so per-entry errors can be inspected and
/// values fetched without materializing the full [`Exif`] map.
///
/// For batch processing, reuse a [`MediaParser`] via [`MediaParser::parse_exif`].
pub fn read_exif_iter(path: impl AsRef<Path>) -> Result<ExifIter> {
    let file = std::fs::File::open(path)?;
    let ms = MediaSource::seekable(file)?;
    let mut parser = MediaParser::new();
    parser.parse_exif(ms)
}

/// Read track metadata from a video / audio file in a single call.
///
/// For batch processing, reuse a [`MediaParser`] via [`MediaParser::parse_track`].
pub fn read_track(path: impl AsRef<Path>) -> Result<TrackInfo> {
    let file = std::fs::File::open(path)?;
    let ms = MediaSource::seekable(file)?;
    let mut parser = MediaParser::new();
    parser.parse_track(ms)
}

/// Read metadata from a file, dispatching by detected [`MediaKind`]:
/// images return [`Metadata::Exif`], video / audio containers return
/// [`Metadata::Track`].
///
/// Use this when the caller does not know up-front whether the file is an
/// image or a track. For batch processing, reuse a [`MediaParser`] and
/// branch on [`MediaSource::kind`] manually.
pub fn read_metadata(path: impl AsRef<Path>) -> Result<Metadata> {
    let file = std::fs::File::open(path)?;
    let ms = MediaSource::seekable(file)?;
    let mut parser = MediaParser::new();
    match ms.kind() {
        MediaKind::Image => parser.parse_exif(ms).map(|i| Metadata::Exif(i.into())),
        MediaKind::Track => parser.parse_track(ms).map(Metadata::Track),
    }
}

/// **Deprecated since v3.3.0**: use [`read_exif`] with
/// [`MediaSource::from_memory`] directly.
#[deprecated(
    since = "3.3.0",
    note = "Use `read_exif` with `MediaSource::from_memory`."
)]
pub fn read_exif_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Exif> {
    #[allow(deprecated)]
    let iter = read_exif_iter_from_bytes(bytes)?;
    Ok(iter.into())
}

#[deprecated(
    since = "3.3.0",
    note = "Use `read_exif_iter` with `MediaSource::from_memory`."
)]
pub fn read_exif_iter_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<ExifIter> {
    let ms = MediaSource::from_memory(bytes)?;
    let mut parser = MediaParser::new();
    parser.parse_exif(ms)
}

#[deprecated(
    since = "3.3.0",
    note = "Use `read_track` with `MediaSource::from_memory`."
)]
pub fn read_track_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<TrackInfo> {
    let ms = MediaSource::from_memory(bytes)?;
    let mut parser = MediaParser::new();
    parser.parse_track(ms)
}

#[deprecated(
    since = "3.3.0",
    note = "Use `read_metadata` with `MediaSource::from_memory`."
)]
pub fn read_metadata_from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Metadata> {
    let ms = MediaSource::from_memory(bytes)?;
    let mut parser = MediaParser::new();
    match ms.kind() {
        MediaKind::Image => parser.parse_exif(ms).map(|i| Metadata::Exif(i.into())),
        MediaKind::Track => parser.parse_track(ms).map(Metadata::Track),
    }
}

#[cfg(feature = "tokio")]
mod tokio_top_level {
    use super::*;

    pub async fn read_exif_async(path: impl AsRef<std::path::Path>) -> Result<Exif> {
        let iter = read_exif_iter_async(path).await?;
        Ok(iter.into())
    }

    pub async fn read_exif_iter_async(path: impl AsRef<std::path::Path>) -> Result<ExifIter> {
        let file = tokio::fs::File::open(path).await?;
        let ms = parser_async::AsyncMediaSource::seekable(file).await?;
        let mut parser = MediaParser::new();
        parser.parse_exif_async(ms).await
    }

    pub async fn read_track_async(path: impl AsRef<std::path::Path>) -> Result<TrackInfo> {
        let file = tokio::fs::File::open(path).await?;
        let ms = parser_async::AsyncMediaSource::seekable(file).await?;
        let mut parser = MediaParser::new();
        parser.parse_track_async(ms).await
    }

    pub async fn read_metadata_async(path: impl AsRef<std::path::Path>) -> Result<Metadata> {
        let file = tokio::fs::File::open(path).await?;
        let ms = parser_async::AsyncMediaSource::seekable(file).await?;
        let mut parser = MediaParser::new();
        match ms.kind() {
            MediaKind::Image => parser
                .parse_exif_async(ms)
                .await
                .map(|i| Metadata::Exif(i.into())),
            MediaKind::Track => parser.parse_track_async(ms).await.map(Metadata::Track),
        }
    }
}

#[cfg(feature = "tokio")]
pub use tokio_top_level::{
    read_exif_async, read_exif_iter_async, read_metadata_async, read_track_async,
};

mod bbox;
mod cr3;
mod ebml;
mod error;
mod exif;
mod file;
mod heif;
mod jpeg;
mod mov;
mod parser;
#[cfg(feature = "tokio")]
mod parser_async;
mod raf;
mod slice;
mod utils;
mod values;
mod video;

#[cfg(test)]
mod testkit;

#[cfg(test)]
mod v3_top_level_tests {
    use super::*;

    #[test]
    fn read_exif_jpg() {
        let exif = read_exif("testdata/exif.jpg").unwrap();
        assert!(exif.get(ExifTag::Make).is_some());
    }

    #[test]
    fn read_track_mov() {
        let info = read_track("testdata/meta.mov").unwrap();
        assert!(info.get(TrackInfoTag::Make).is_some());
    }

    #[test]
    fn read_metadata_dispatches_image() {
        match read_metadata("testdata/exif.jpg").unwrap() {
            Metadata::Exif(_) => {}
            Metadata::Track(_) => panic!("expected Exif variant"),
        }
    }

    #[test]
    fn read_metadata_dispatches_track() {
        match read_metadata("testdata/meta.mov").unwrap() {
            Metadata::Track(_) => {}
            Metadata::Exif(_) => panic!("expected Track variant"),
        }
    }

    #[cfg(feature = "tokio")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn read_exif_async_jpg() {
        let exif = read_exif_async("testdata/exif.jpg").await.unwrap();
        assert!(exif.get(ExifTag::Make).is_some());
    }

    #[cfg(feature = "tokio")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn read_track_async_mov() {
        let info = read_track_async("testdata/meta.mov").await.unwrap();
        assert!(info.get(TrackInfoTag::Make).is_some());
    }

    #[test]
    #[allow(deprecated)]
    fn read_exif_from_bytes_jpg() {
        let raw = std::fs::read("testdata/exif.jpg").unwrap();
        let exif = read_exif_from_bytes(raw).unwrap();
        assert!(exif.get(ExifTag::Make).is_some());
    }

    #[test]
    #[allow(deprecated)]
    fn read_exif_iter_from_bytes_jpg() {
        let raw = std::fs::read("testdata/exif.jpg").unwrap();
        let iter = read_exif_iter_from_bytes(raw).unwrap();
        assert!(iter.into_iter().count() > 0);
    }

    #[test]
    #[allow(deprecated)]
    fn read_track_from_bytes_mov() {
        let raw = std::fs::read("testdata/meta.mov").unwrap();
        let info = read_track_from_bytes(raw).unwrap();
        assert!(info.get(TrackInfoTag::Make).is_some());
    }

    #[test]
    #[allow(deprecated)]
    fn read_metadata_from_bytes_dispatches_image() {
        let raw = std::fs::read("testdata/exif.jpg").unwrap();
        match read_metadata_from_bytes(raw).unwrap() {
            Metadata::Exif(_) => {}
            Metadata::Track(_) => panic!("expected Exif variant"),
        }
    }

    #[test]
    #[allow(deprecated)]
    fn read_metadata_from_bytes_dispatches_track() {
        let raw = std::fs::read("testdata/meta.mov").unwrap();
        match read_metadata_from_bytes(raw).unwrap() {
            Metadata::Track(_) => {}
            Metadata::Exif(_) => panic!("expected Track variant"),
        }
    }

    #[test]
    #[allow(deprecated)]
    fn read_exif_from_bytes_static_slice() {
        let raw: &'static [u8] = include_bytes!("../testdata/exif.jpg");
        let exif = read_exif_from_bytes(raw).unwrap();
        assert!(exif.get(ExifTag::Make).is_some());
    }

    #[test]
    fn prelude_imports_compile() {
        use crate::prelude::*;
        fn _consume(_: Option<Exif>, _: Option<TrackInfo>, _: Option<MediaParser>) {}
        // Verify the function symbols are in scope (compilation is the test).
        let _e = read_exif("testdata/exif.jpg");
        let _t = read_track("testdata/meta.mov");
        let _m = read_metadata("testdata/exif.jpg");
    }
}

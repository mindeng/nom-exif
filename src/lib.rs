//! `nom-exif` — Exif and track metadata parser for image, video, and audio
//! files.
//!
//! **v3 (in progress):** the API is being reshaped; this top-level docstring
//! is a placeholder until P6 lands the full v3 tutorial. The public symbols
//! you most likely want:
//!
//! - One-shot helpers: [`read_exif`], [`read_exif_iter`], [`read_track`],
//!   [`read_metadata`].
//! - Reusable parser: [`MediaParser`] + [`MediaSource`] + [`MediaKind`].
//! - Async variants under `feature = "tokio"`.
//!
//! See the v3 design document at `docs/V3_API_DESIGN.md` for the full
//! migration story.

pub use parser::{MediaKind, MediaParser, MediaSource};
pub use video::{TrackInfo, TrackInfoTag};

#[cfg(feature = "tokio")]
pub use parser_async::AsyncMediaSource;

pub use exif::{Exif, ExifIter, ExifTag, GPSInfo, LatLng, ParsedExifEntry};
pub use exif::gps::{Altitude, LatRef, LonRef, Speed, SpeedUnit};
pub use values::{EntryValue, ExifDateTime, IRational, URational};

pub use error::{ConvertError, EntryError, Error, MalformedKind};
pub type Result<T> = std::result::Result<T, Error>;

/// One-shot result of [`read_metadata`]: either Exif (image) or TrackInfo
/// (video/audio). Closed enum — see spec §8.6 for why there's no `Both`
/// variant.
#[derive(Debug, Clone)]
pub enum Metadata {
    Exif(Exif),
    Track(TrackInfo),
}

use std::io::BufReader;
use std::path::Path;

/// Read EXIF metadata from a file in a single call. Wraps the `File` in a
/// `BufReader` internally so the hot path (`for path in paths { read_exif(path)? }`)
/// is immune to per-syscall overhead.
///
/// For batch processing, prefer constructing a [`MediaParser`] once and
/// reusing its parse buffer via [`MediaParser::parse_exif`].
pub fn read_exif(path: impl AsRef<Path>) -> Result<Exif> {
    let iter = read_exif_iter(path)?;
    Ok(iter.into())
}

pub fn read_exif_iter(path: impl AsRef<Path>) -> Result<ExifIter> {
    let file = std::fs::File::open(path)?;
    let ms = MediaSource::seekable(BufReader::new(file))?;
    let mut parser = MediaParser::new();
    parser.parse_exif(ms)
}

pub fn read_track(path: impl AsRef<Path>) -> Result<TrackInfo> {
    let file = std::fs::File::open(path)?;
    let ms = MediaSource::seekable(BufReader::new(file))?;
    let mut parser = MediaParser::new();
    parser.parse_track(ms)
}

pub fn read_metadata(path: impl AsRef<Path>) -> Result<Metadata> {
    let file = std::fs::File::open(path)?;
    let ms = MediaSource::seekable(BufReader::new(file))?;
    let mut parser = MediaParser::new();
    match ms.kind() {
        MediaKind::Image => parser.parse_exif(ms).map(|i| Metadata::Exif(i.into())),
        MediaKind::Track => parser.parse_track(ms).map(Metadata::Track),
    }
}

#[cfg(feature = "tokio")]
mod tokio_top_level {
    use super::*;
    use tokio::io::BufReader as TokioBufReader;

    pub async fn read_exif_async(path: impl AsRef<std::path::Path>) -> Result<Exif> {
        let iter = read_exif_iter_async(path).await?;
        Ok(iter.into())
    }

    pub async fn read_exif_iter_async(path: impl AsRef<std::path::Path>) -> Result<ExifIter> {
        let file = tokio::fs::File::open(path).await?;
        let ms = parser_async::AsyncMediaSource::seekable(TokioBufReader::new(file)).await?;
        let mut parser = MediaParser::new();
        parser.parse_exif_async(ms).await
    }

    pub async fn read_track_async(path: impl AsRef<std::path::Path>) -> Result<TrackInfo> {
        let file = tokio::fs::File::open(path).await?;
        let ms = parser_async::AsyncMediaSource::seekable(TokioBufReader::new(file)).await?;
        let mut parser = MediaParser::new();
        parser.parse_track_async(ms).await
    }

    pub async fn read_metadata_async(path: impl AsRef<std::path::Path>) -> Result<Metadata> {
        let file = tokio::fs::File::open(path).await?;
        let ms = parser_async::AsyncMediaSource::seekable(TokioBufReader::new(file)).await?;
        let mut parser = MediaParser::new();
        match ms.kind() {
            MediaKind::Image => parser.parse_exif_async(ms).await.map(|i| Metadata::Exif(i.into())),
            MediaKind::Track => parser.parse_track_async(ms).await.map(Metadata::Track),
        }
    }
}

#[cfg(feature = "tokio")]
pub use tokio_top_level::{read_exif_async, read_exif_iter_async, read_metadata_async, read_track_async};

mod bbox;
mod buffer;
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
mod partial_vec;
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
}

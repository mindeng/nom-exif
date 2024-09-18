//! `nom-exif` is an Exif/metadata parsing library written in pure Rust with
//! [nom](https://github.com/rust-bakery/nom).
//!
//! ## Supported File Types
//!
//! - Image
//!   - *.heic, *.heif, etc.
//!   - *.jpg, *.jpeg
//!   - *.tiff, *.tif
//! - Video/Audio
//!   - ISO base media file format (ISOBMFF): *.mp4, *.mov, *.3gp, etc.
//!   - Matroska based file format: *.webm, *.mkv, *.mka, etc.
//!
//! ## Key Features
//!
//! - Ergonomic Design
//!
//!   - **Unified Workflow** for Various File Types
//!   
//!     Now, multimedia files of different types and formats (including images,
//!     videos, and audio) can be processed using a unified method. This consistent
//!     API interface simplifies user experience and reduces cognitive load.
//!     
//!     The usage is demonstrated in the following examples. `examples/rexiftool`
//!     is also a good example.
//!   
//!   - Two style APIs for Exif
//!   
//!     *iterator* style ([`ExifIter`]) and *get* style ([`Exif`]). The former is
//!     parse-on-demand, and therefore, more detailed error information can be
//!     captured; the latter is simpler and easier to use.
//!   
//! - Performance
//!
//!   - *Zero-copy* when appropriate: Use borrowing and slicing instead of
//!     copying whenever possible.
//!     
//!   - Minimize I/O operations: When metadata is stored at the end/middle of a
//!     large file (such as a QuickTime file does), `Seek` rather than `Read`
//!     to quickly locate the location of the metadata (if the reader supports
//!     `Seek`).
//!   
//!   - Share I/O and parsing buffer between multiple parse calls: This can
//!     improve performance and avoid the overhead and memory fragmentation
//!     caused by frequent memory allocation. This feature is very useful when
//!     you need to perform batch parsing.
//!     
//!   - Pay as you go: When working with [`ExifIter`], all entries are
//!     lazy-parsed. That is, only when you iterate over [`ExifIter`] will the
//!     IFD entries be parsed one by one.
//!     
//! - Robustness and stability
//!
//!   Through long-term [Fuzz testing](https://github.com/rust-fuzz/afl.rs), and
//!   tons of crash issues discovered during testing have been fixed. Thanks to
//!   [@sigaloid](https://github.com/sigaloid) for [pointing this
//!   out](https://github.com/mindeng/nom-exif/pull/5)!
//!
//! - Supports both *sync* and *async* APIs
//!
//! ## Unified Workflow for Various File Types
//!
//! By using `MediaSource` & `MediaParser`, multimedia files of different types and
//! formats (including images, videos, and audio) can be processed using a unified
//! method.
//!
//! Here's an example:
//!
//! ```rust
//! use nom_exif::*;
//!
//! fn main() -> Result<()> {
//!     let mut parser = MediaParser::new();
//!
//!     let files = [
//!         "./testdata/exif.heic",
//!         "./testdata/exif.jpg",
//!         "./testdata/tif.tif",
//!         "./testdata/meta.mov",
//!         "./testdata/meta.mp4",
//!         "./testdata/webm_480.webm",
//!         "./testdata/mkv_640x360.mkv",
//!         "./testdata/mka.mka",
//!         "./testdata/3gp_640x360.3gp"
//!     ];
//!
//!     for f in files {
//!         let ms = MediaSource::file_path("./testdata/exif.heic")?;
//!
//!         if ms.has_exif() {
//!             // Parse the file as an Exif-compatible file
//!             let mut iter: ExifIter = parser.parse(ms)?;
//!             // ...
//!         } else if ms.has_track() {
//!             // Parse the file as a track
//!             let info: TrackInfo = parser.parse(ms)?;
//!             // ...
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Sync API: `MediaSource` + `MediaParser`
//!
//! `MediaSource` is an abstraction of multimedia data sources, which can be
//! created from any object that implements the `Read` trait, and can be parsed by
//! `MediaParser`.
//!
//! Example:
//!
//! ```rust
//! use nom_exif::*;
//!
//! fn main() -> Result<()> {
//!     let mut parser = MediaParser::new();
//!     
//!     let ms = MediaSource::file_path("./testdata/exif.heic")?;
//!     assert!(ms.has_exif());
//!     
//!     let mut iter: ExifIter = parser.parse(ms)?;
//!     let exif: Exif = iter.into();
//!     assert_eq!(exif.get(ExifTag::Make).unwrap().as_str().unwrap(), "Apple");
//!
//!     let ms = MediaSource::file_path("./testdata/meta.mov")?;
//!     assert!(ms.has_track());
//!     
//!     let info: TrackInfo = parser.parse(ms)?;
//!     assert_eq!(info.get(TrackInfoTag::Make), Some(&"Apple".into()));
//!     assert_eq!(info.get(TrackInfoTag::Model), Some(&"iPhone X".into()));
//!     assert_eq!(info.get(TrackInfoTag::GpsIso6709), Some(&"+27.1281+100.2508+000.000/".into()));
//!     assert_eq!(info.get_gps_info().unwrap().latitude_ref, 'N');
//!     assert_eq!(
//!         info.get_gps_info().unwrap().latitude,
//!         [(27, 1), (7, 1), (68, 100)].into(),
//!     );
//!
//!     // `MediaSource` can also be created from a `TcpStream`:
//!     // let ms = MediaSource::tcp_stream(stream)?;
//!
//!     // Or from any `Read + Seek`:
//!     // let ms = MediaSource::seekable(stream)?;
//!     
//!     // From any `Read`:
//!     // let ms = MediaSource::unseekable(stream)?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! See [`MediaSource`] & [`MediaParser`] for more information.
//!
//! ## Async API: `AsyncMediaSource` + `AsyncMediaParser`
//!
//! Likewise, `AsyncMediaParser` is an abstraction for asynchronous multimedia data
//! sources, which can be created from any object that implements the `AsyncRead`
//! trait, and can be parsed by `AsyncMediaParser`.
//!
//! Enable `async` feature flag for `nom-exif` in your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! nom-exif = { version = "1", features = ["async"] }
//! ```
//!
//! See [`AsyncMediaSource`] & [`AsyncMediaParser`] for more information.
//!
//! ## GPS Info
//!
//! `ExifIter` provides a convenience method for parsing gps information. (`Exif` &
//! `TrackInfo` also provide a `get_gps_info` mthod).
//!     
//! ```rust
//! use nom_exif::*;
//!
//! fn main() -> Result<()> {
//!     let mut parser = MediaParser::new();
//!     
//!     let ms = MediaSource::file_path("./testdata/exif.heic")?;
//!     let iter: ExifIter = parser.parse(ms)?;
//!
//!     let gps_info = iter.parse_gps_info()?.unwrap();
//!     assert_eq!(gps_info.format_iso6709(), "+43.29013+084.22713+1595.950CRSWGS_84/");
//!     assert_eq!(gps_info.latitude_ref, 'N');
//!     assert_eq!(gps_info.longitude_ref, 'E');
//!     assert_eq!(
//!         gps_info.latitude,
//!         [(43, 1), (17, 1), (2446, 100)].into(),
//!     );
//!     Ok(())
//! }
//! ```
//!
//! For more usage details, please refer to the [API
//! documentation](https://docs.rs/nom-exif/latest/nom_exif/).
//!
//! ## CLI Tool `rexiftool`
//!
//! ### Human Readable Output
//!
//! `cargo run --example rexiftool testdata/meta.mov`:
//!
//! ``` text
//! Make                            => Apple
//! Model                           => iPhone X
//! Software                        => 12.1.2
//! CreateDate                      => 2024-02-02T08:09:57+00:00
//! Duration                        => 500
//! ImageWidth                      => 720
//! ImageHeight                     => 1280
//! GpsIso6709                      => +27.1281+100.2508+000.000/
//! ```
//!
//! ### Json Dump
//!
//! `cargo run --example rexiftool testdata/meta.mov -j`:
//!
//! ``` text
//! {
//!   "ImageWidth": "720",
//!   "Software": "12.1.2",
//!   "ImageHeight": "1280",
//!   "Make": "Apple",
//!   "GpsIso6709": "+27.1281+100.2508+000.000/",
//!   "CreateDate": "2024-02-02T08:09:57+00:00",
//!   "Model": "iPhone X",
//!   "Duration": "500"
//! }
//! ```
//!
//! ### Parsing Files in Directory
//!
//! `rexiftool` also supports batch parsing of all files in a folder
//! (non-recursive).
//!
//! `cargo run --example rexiftool testdata/`:
//!
//! ```text
//! File: "testdata/embedded-in-heic.mov"
//! ------------------------------------------------
//! Make                            => Apple
//! Model                           => iPhone 15 Pro
//! Software                        => 17.1
//! CreateDate                      => 2023-11-02T12:01:02+00:00
//! Duration                        => 2795
//! ImageWidth                      => 1920
//! ImageHeight                     => 1440
//! GpsIso6709                      => +22.5797+113.9380+028.396/
//!
//! File: "testdata/compatible-brands-fail.heic"
//! ------------------------------------------------
//! Unrecognized file format, consider filing a bug @ https://github.com/mindeng/nom-exif.
//!
//! File: "testdata/webm_480.webm"
//! ------------------------------------------------
//! CreateDate                      => 2009-09-09T09:09:09+00:00
//! Duration                        => 30543
//! ImageWidth                      => 480
//! ImageHeight                     => 270
//!
//! File: "testdata/mka.mka"
//! ------------------------------------------------
//! Duration                        => 3422
//! ImageWidth                      => 0
//! ImageHeight                     => 0
//!
//! File: "testdata/exif-one-entry.heic"
//! ------------------------------------------------
//! Orientation                     => 1
//!
//! File: "testdata/no-exif.jpg"
//! ------------------------------------------------
//! Error: parse failed: Exif not found
//!
//! File: "testdata/exif.jpg"
//! ------------------------------------------------
//! ImageWidth                      => 3072
//! Model                           => vivo X90 Pro+
//! ImageHeight                     => 4096
//! ModifyDate                      => 2023-07-09T20:36:33+08:00
//! YCbCrPositioning                => 1
//! ExifOffset                      => 201
//! MakerNote                       => Undefined[0x30]
//! RecommendedExposureIndex        => 454
//! SensitivityType                 => 2
//! ISOSpeedRatings                 => 454
//! ExposureProgram                 => 2
//! FNumber                         => 175/100 (1.7500)
//! ExposureTime                    => 9997/1000000 (0.0100)
//! SensingMethod                   => 2
//! SubSecTimeDigitized             => 616
//! OffsetTimeOriginal              => +08:00
//! SubSecTimeOriginal              => 616
//! OffsetTime                      => +08:00
//! SubSecTime                      => 616
//! FocalLength                     => 8670/1000 (8.6700)
//! Flash                           => 16
//! LightSource                     => 21
//! MeteringMode                    => 1
//! SceneCaptureType                => 0
//! UserComment                     => filter: 0; fileterIntensity: 0.0; filterMask: 0; algolist: 0;
//! ...
//! ```

pub use parser::{MediaParser, MediaSource};
pub use video::{TrackInfo, TrackInfoTag};

#[cfg(feature = "async")]
pub use parser_async::{AsyncMediaParser, AsyncMediaSource};

pub use exif::{Exif, ExifIter, ExifTag, GPSInfo, LatLng, ParsedExifEntry};
pub use values::{EntryValue, URational};

#[allow(deprecated)]
pub use exif::parse_exif;
#[cfg(feature = "async")]
#[allow(deprecated)]
pub use exif::parse_exif_async;

#[allow(deprecated)]
pub use heif::parse_heif_exif;
#[allow(deprecated)]
pub use jpeg::parse_jpeg_exif;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) use skip::{Seekable, Unseekable};

#[allow(deprecated)]
pub use file::FileFormat;

#[allow(deprecated)]
pub use mov::{parse_metadata, parse_mov_metadata};

mod bbox;
mod buffer;
mod ebml;
mod error;
mod exif;
mod file;
mod heif;
mod jpeg;
mod loader;
mod mov;
mod parser;
#[cfg(feature = "async")]
mod parser_async;
mod partial_vec;
mod skip;
mod slice;
mod values;
mod video;

#[cfg(test)]
mod testkit;

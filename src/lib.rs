//! nom-exif is an Exif/metadata parsing library written in pure Rust with
//! [nom](https://github.com/rust-bakery/nom). Both images
//! (jpeg/heif/heic/jpg/tiff etc.) and videos/audios
//! (mov/mp4/3gp/webm/mkv/mka, etc.) are supported.
//!
//! Supporting both *sync* and *async* interfaces. The interface design is
//! simple and easy to use.
//!
//! ## Key Features
//!
//! - Ergonomic Design
//!
//!   - Media type auto-detecting: No need to check the file extensions!
//!     `nom-exif` can automatically detect supported file formats and parse
//!     them correctly.
//!
//!     To achieve this goal, the API has been carefully designed so that
//!     various types of multimedia files can be easily processed using the
//!     same set of processes.
//!
//!     Compared with the way the user judges the file name and then decides
//!     which parsing function to call (such as `parse_jpg`, `parse_mp4`,
//!     etc.), this work flow is simpler, more reliable, and more versatile
//!     (can be applied to non-file scenarios, such as `TcpStream`).
//!     
//!     The usage is demonstrated in the following examples.
//!     `examples/rexiftool` is also a good example.
//!
//!   - Two style APIs for Exif: *iterator* style ([`ExifIter`]) and *get*
//!     style ([`Exif`]). The former is parse-on-demand, and therefore, more
//!     detailed error information can be captured; the latter is simpler and
//!     easier to use.
//!   
//! - Performance
//!
//!   - *Zero-copy* when appropriate: Use borrowing and slicing instead of
//!     copying whenever possible.
//!     
//!   - Minimize I/O operations: When metadata is stored at the end/middle of a
//!     large file (such as a QuickTime file does), `Seek` rather than `Read`
//!     to quickly locate the location of the metadata (if only the reader
//!     support `Seek`, see [`parse_track_info`](crate::parse_track_info) for
//!     more information).
//!     
//!   - Pay as you go: When working with [`ExifIter`], all entries are
//!     lazy-parsed. That is, only when you iterate over [`ExifIter`] will the
//!     IFD entries be parsed one by one.
//!     
//! - Robustness and stability: Through long-term [Fuzz
//!   testing](https://github.com/rust-fuzz/afl.rs), and tons of crash issues
//!   discovered during testing have been fixed. Thanks to
//!   [@sigaloid](https://github.com/sigaloid) for [pointing this
//!   out](https://github.com/mindeng/nom-exif/pull/5)!
//!
//! - Supports both *sync* and *async* interfaces.
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
//! ## Media type auto-detecting
//!
//! ```rust
//! use nom_exif::*;
//!
//! fn main() -> Result<()> {
//!     let mut parser = MediaParser::new();
//!     
//!     // The file can be an image, a video, or an audio.
//!     let ms = MediaSource::file_path("./testdata/exif.heic")?;
//!     if ms.has_exif() {
//!         // Parse the file as an Exif-compatible file
//!         let mut iter: ExifIter = parser.parse(ms)?;
//!         let exif: Exif = iter.into();
//!         assert_eq!(exif.get(ExifTag::Make).unwrap().as_str().unwrap(), "Apple");
//!     } else if ms.has_track() {
//!         // Parse the file as a track
//!     }
//!
//!     let ms = MediaSource::file_path("./testdata/meta.mov")?;
//!     if ms.has_track() {
//!         // Parse the file as a track
//!         let info: TrackInfo = parser.parse(ms)?;
//!         assert_eq!(info.get(TrackInfoTag::Make), Some(&"Apple".into()));
//!         assert_eq!(info.get(TrackInfoTag::Model), Some(&"iPhone X".into()));
//!         assert_eq!(info.get(TrackInfoTag::GpsIso6709), Some(&"+27.1281+100.2508+000.000/".into()));
//!         assert_eq!(info.get_gps_info().unwrap().latitude_ref, 'N');
//!         assert_eq!(
//!             info.get_gps_info().unwrap().latitude,
//!             [(27, 1), (7, 1), (68, 100)].into(),
//!         );
//!     }
//!
//!     // `MediaSource` can also be created from any `Read`, such as a `TcpStream`:
//!     // let ms = MediaSource::tcp_stream(stream)?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Sync API Usage
//!
//! ```rust
//! use nom_exif::*;
//!
//! fn main() -> Result<()> {
//!     let mut parser = MediaParser::new();
//!     let ms = MediaSource::file_path("./testdata/exif.heic")?;
//!     let mut iter: ExifIter = parser.parse(ms)?;
//!
//!     // Use `next()` API
//!     let entry = iter.next().unwrap();
//!     assert_eq!(entry.ifd_index(), 0);
//!     assert_eq!(entry.tag().unwrap(), ExifTag::Make);
//!     assert_eq!(entry.tag_code(), 0x010f);
//!     assert_eq!(entry.get_value().unwrap().as_str().unwrap(), "Apple");
//!
//!     // You can also iterate it in a `for` loop. Clone it first so we won't
//!     // consume the original one.
//!     for entry in iter.clone_and_rewind() {
//!         if entry.tag().unwrap() == ExifTag::Make {
//!             assert_eq!(entry.get_result().unwrap().as_str().unwrap(), "Apple");
//!             break;
//!         }
//!     }
//!
//!     // filter, map & collect
//!     let tags = [ExifTag::Make, ExifTag::Model];
//!     let res: Vec<String> = iter
//!         .clone()
//!         .filter(|e| e.tag().is_some_and(|t| tags.contains(&t)))
//!         .filter(|e| e.has_value())
//!         .map(|e| format!("{} => {}", e.tag().unwrap(), e.get_value().unwrap()))
//!         .collect();
//!     assert_eq!(
//!         res.join(", "),
//!         "Make => Apple, Model => iPhone 12 Pro"
//!     );
//!     
//!     // An `ExifIter` can be easily converted to an `Exif`
//!     let exif: Exif = iter.into();
//!     assert_eq!(
//!         exif.get(ExifTag::Model).unwrap().as_str().unwrap(),
//!         "iPhone 12 Pro"
//!     );
//!     Ok(())
//! }
//! ```
//!
//! ## Async API Usage
//!
//! Enable `async` feature flag for nom-exif in your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! nom-exif = { version = "1", features = ["async"] }
//! ```
//!
//! For detailed usage, please refer to: [`AsyncMediaParser::parse`].
//!
//! ## GPS Info
//!
//! `ExifIter` provides a convenience method for parsing gps information.
//! (`Exif` also provides a `get_gps_info` mthod).
//!     
//! ```rust
//! use nom_exif::*;
//! use std::fs::File;
//!
//! fn main() -> Result<()> {
//!     let f = File::open("./testdata/exif.heic")?;
//!     let iter = parse_exif(f, None)?.unwrap();
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
//! ## Video
//!
//! Please refer to: [`parse_track_info`](crate::parse_track_info).
//!
//! For more usage details, please refer to the [API
//! documentation](https://docs.rs/nom-exif/latest/nom_exif/).

pub use parser::{MediaParser, MediaSource};
pub use video::{TrackInfo, TrackInfoTag};

#[cfg(feature = "async")]
pub use parser_async::{AsyncMediaParser, AsyncMediaSource};

#[allow(deprecated)]
pub use exif::parse_exif;
#[cfg(feature = "async")]
#[allow(deprecated)]
pub use exif::parse_exif_async;
pub use exif::{Exif, ExifIter, ExifTag, GPSInfo, LatLng, ParsedExifEntry};
pub use values::EntryValue;

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

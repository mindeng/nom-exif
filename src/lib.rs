//! nom-exif is an Exif/metadata parsing library written in pure Rust with
//! [nom](https://github.com/rust-bakery/nom), both JPEG/HEIF/HEIC images and
//! MOV/MP4 videos are supported.
//!
//! Supporting both *sync* and *async* interfaces. The interface design is
//! simple and easy to use.
//!
//! ## Key Features
//!
//! - Supports both *sync* and *async* interfaces.
//!
//! - Supports two style interfaces, *iterator* ([`ExifIter`]) and *get*
//!   ([`Exif`]). The former is fully functional and the latter is simple and
//!   easy to use.
//!   
//! - Performance
//!
//!   - *Zero-copy* when appropriate: Use borrowing and slicing instead of
//!     copying whenever possible.
//!     
//!   - Minimize I/O operations: When metadata is stored at the end/middle of a
//!     large file (such as a QuickTime file does), `Seek` rather than `Read`
//!     to quickly locate the location of the metadata.
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
//! ## Supported File Types
//!
//! - Images
//!   - JPEG
//!   - HEIF/HEIC
//! - Videos
//!   - MOV
//!   - MP4
//!
//! ## Sync API Usage
//!
//! ```rust
//! use nom_exif::*;
//! use std::fs::File;
//!
//! fn main() -> Result<()> {
//!     let f = File::open("./testdata/exif.heic")?;
//!     let mut iter = parse_exif(f, None)?.unwrap();
//!
//!     // Use `next()` API
//!     let entry = iter.next().unwrap();
//!     assert_eq!(entry.ifd_index(), 0);
//!     assert_eq!(entry.tag().unwrap(), ExifTag::Make);
//!     assert_eq!(entry.tag_code(), 0x010f);
//!     assert_eq!(entry.take_result()?.as_str().unwrap(), "Apple");
//!
//!     // You can also iterate it in a `for` loop. Clone it first so we won't
//!     // consume the original one. Note that the new cloned `ExifIter` will
//!     // always start from the first entry.
//!     for entry in iter.clone() {
//!         if entry.tag().unwrap() == ExifTag::Make {
//!             assert_eq!(entry.take_result()?.as_str().unwrap(), "Apple");
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
//!         .map(|e| format!("{} => {}", e.tag().unwrap(), e.take_value().unwrap()))
//!         .collect();
//!     assert_eq!(
//!         res.join(", "),
//!         "Make(0x010f) => Apple, Model(0x0110) => iPhone 12 Pro"
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
//! Since parsing process is a CPU-bound task, you may want to move the job to
//! a separated thread (better to use rayon crate). There is a simple example
//! below.
//!     
//! You can safely and cheaply clone an [`ExifIter`] in multiple tasks/threads
//! concurrently, since it use `Arc` to share the underlying memory.
//!
//! ```rust
//! #[cfg(feature = "async")]
//! use nom_exif::{parse_exif_async, ExifIter, Exif, ExifTag};
//! #[cfg(feature = "async")]
//! use tokio::task::spawn_blocking;
//! #[cfg(feature = "async")]
//! use tokio::fs::File;
//!
//! #[cfg(feature = "async")]
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut f = File::open("./testdata/exif.heic").await?;
//!     let mut iter = parse_exif_async(f, None).await?.unwrap();
//!
//!     for entry in iter.clone() {
//!         if entry.tag().unwrap() == ExifTag::Make {
//!             assert_eq!(entry.take_result()?.as_str().unwrap(), "Apple");
//!             break;
//!         }
//!     }
//!
//!     // Convert an `ExifIter` into an `Exif` in a separated thread.
//!     let exif = spawn_blocking(move || {
//!         let exif: Exif = iter.into();
//!         exif
//!     }).await?;
//!     
//!     assert_eq!(
//!         exif.get(ExifTag::Model).unwrap().to_string(),
//!         "iPhone 12 Pro"
//!     );
//!     Ok(())
//! }
//! ```
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
//!     Ok(())
//! }
//! ```
//!
//! For more usage details, please refer to the [API
//! documentation](https://docs.rs/nom-exif/latest/nom_exif/).

pub use heif::parse_heif_exif;
pub use jpeg::parse_jpeg_exif;
pub use mov::{parse_metadata, parse_mov_metadata};
pub use video::parse_video_info;

#[cfg(feature = "async")]
pub use exif::parse_exif_async;
pub use exif::{parse_exif, Exif, ExifIter, ExifTag, GPSInfo, LatLng, ParsedExifEntry};
pub use file::FileFormat;
pub use values::EntryValue;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;

mod bbox;
mod ebml;
mod error;
mod exif;
mod file;
mod heif;
mod input;
mod jpeg;
mod loader;
mod mov;
mod slice;
mod values;
mod video;

#[cfg(test)]
mod testkit;

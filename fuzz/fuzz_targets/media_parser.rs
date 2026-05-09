#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use nom_exif::{Exif, ExifIter, MediaParser, MediaSource, TrackInfo};

fuzz_target!(|data: &[u8]| {
    let mut parser = MediaParser::new();

    // Parse seekable
    if let Ok(ms) = MediaSource::seekable(Cursor::new(data)) {
        let iter: Result<ExifIter, _> = parser.parse_exif(ms);
        if let Ok(iter) = iter {
            let _ = iter.parse_gps();
            let _: Exif = iter.into();
        }
    }

    if let Ok(ms) = MediaSource::seekable(Cursor::new(data)) {
        let _: Result<TrackInfo, _> = parser.parse_track(ms);
    }

    // Parse unseekable
    if let Ok(ms) = MediaSource::unseekable(Cursor::new(data)) {
        let iter: Result<ExifIter, _> = parser.parse_exif(ms);
        if let Ok(iter) = iter {
            let _ = iter.parse_gps();
            let _: Exif = iter.into();
        }
    }

    if let Ok(ms) = MediaSource::unseekable(Cursor::new(data)) {
        let _: Result<TrackInfo, _> = parser.parse_track(ms);
    }
});

use std::io::Cursor;

use nom_exif::{Exif, ExifIter, MediaParser, MediaSource, TrackInfo};

fn main() {
    afl::fuzz!(|data: &[u8]| {
        let mut parser = MediaParser::new();

        // MediaSource

        let reader = Cursor::new(data);
        let _ = MediaSource::seekable(reader);
        let reader = Cursor::new(data);
        let _ = MediaSource::unseekable(reader);

        // Parse seekable

        let reader = Cursor::new(data);
        let Ok(ms) = MediaSource::seekable(reader) else {
            return;
        };
        let iter: Result<ExifIter, _> = parser.parse(ms);
        if let Ok(iter) = iter {
            let _ = iter.parse_gps_info();
            let _: Exif = iter.into();
        }

        let reader = Cursor::new(data);
        let Ok(ms) = MediaSource::seekable(reader) else {
            return;
        };
        let _: Result<TrackInfo, _> = parser.parse(ms);

        // Parse unseekable

        let reader = Cursor::new(data);
        let Ok(ms) = MediaSource::unseekable(reader) else {
            return;
        };
        let iter: Result<ExifIter, _> = parser.parse(ms);
        if let Ok(iter) = iter {
            let _ = iter.parse_gps_info();
            let _: Exif = iter.into();
        }

        let reader = Cursor::new(data);
        let Ok(ms) = MediaSource::unseekable(reader) else {
            return;
        };
        let _: Result<TrackInfo, _> = parser.parse(ms);
    });
}

use std::{
    fs::File,
    io::{self, Cursor},
};

use nom_exif::{Exif, ExifIter, MediaParser, MediaSource, TrackInfo};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Registry};

fn main() {
    if std::env::var("RUST_LOG").is_ok() {
        init_tracing().expect("init tracing failed");
    }

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

fn init_tracing() -> io::Result<()> {
    let stdout_log = tracing_subscriber::fmt::layer().pretty();
    let subscriber = Registry::default().with(stdout_log);

    let file = File::create("debug.log")?;
    let debug_log = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file);
    let subscriber = subscriber.with(debug_log);

    subscriber.init();

    Ok(())
}

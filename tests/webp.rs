//! Integration tests for WebP support, exercised against a real-world
//! `testdata/exif.webp` (a libwebp gallery image with an `EXIF` chunk
//! injected by exiftool — a genuine VP8X + `EXIF`-after-bitstream layout).

use nom_exif::{read_exif, ExifTag, MediaParser, MediaSource};

#[test]
fn read_exif_on_exif_webp_file() {
    let exif = read_exif("testdata/exif.webp").unwrap();
    assert_eq!(
        exif.get(ExifTag::Make).and_then(|v| v.as_str()),
        Some("Google")
    );
    assert_eq!(
        exif.get(ExifTag::Model).and_then(|v| v.as_str()),
        Some("Pixel WebP Sample")
    );
}

#[test]
fn read_exif_on_exif_webp_recovers_gps() {
    let exif = read_exif("testdata/exif.webp").unwrap();
    let gps = exif.gps_info().unwrap();
    // 37.80104 N, 122.42501 W (as written by exiftool).
    assert!((gps.latitude_decimal().unwrap() - 37.80104).abs() < 1e-4);
    assert!((gps.longitude_decimal().unwrap() - (-122.42501)).abs() < 1e-4);
}

#[test]
fn parse_exif_webp_from_memory() {
    let mut parser = MediaParser::new();
    let raw = std::fs::read("testdata/exif.webp").unwrap();
    let ms = MediaSource::from_memory(raw).unwrap();
    let iter = parser.parse_exif(ms).unwrap();
    let exif: nom_exif::Exif = iter.into();
    assert_eq!(
        exif.get(ExifTag::Make).and_then(|v| v.as_str()),
        Some("Google")
    );
}

#[test]
fn parse_exif_webp_seekable_stream() {
    // Drives the streaming path (seek + incremental fill) rather than a
    // single in-memory buffer, exercising the ClearAndSkip resume over the
    // VP8 bitstream that precedes the EXIF chunk.
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("testdata/exif.webp").unwrap();
    let iter = parser.parse_exif(ms).unwrap();
    let exif: nom_exif::Exif = iter.into();
    assert_eq!(
        exif.get(ExifTag::Make).and_then(|v| v.as_str()),
        Some("Google")
    );
}

#[cfg(feature = "tokio-fs")]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn parse_exif_webp_async() {
    use nom_exif::AsyncMediaSource;
    let mut parser = MediaParser::new();
    let ms = AsyncMediaSource::open("testdata/exif.webp").await.unwrap();
    let iter = parser.parse_exif_async(ms).await.unwrap();
    let exif: nom_exif::Exif = iter.into();
    assert_eq!(
        exif.get(ExifTag::Make).and_then(|v| v.as_str()),
        Some("Google")
    );
}

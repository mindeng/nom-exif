//! Integration tests for PNG support. Each fixture is exercised through
//! the full set of public entry points.

#[path = "png_fixtures.rs"]
mod png_fixtures;

use nom_exif::{read_exif, ExifTag, ImageFormatMetadata, MediaParser, MediaSource};

#[test]
fn read_exif_on_exif_png_file() {
    let exif = read_exif("testdata/exif.png").unwrap();
    assert!(exif.get(ExifTag::Make).is_some());
}

#[test]
fn read_exif_on_text_only_png_returns_exif_not_found() {
    let res = read_exif("testdata/text-only.png");
    assert!(matches!(res, Err(nom_exif::Error::ExifNotFound)));
}

#[test]
fn parse_image_metadata_exif_png_file() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("testdata/exif.png").unwrap();
    let img = parser.parse_image_metadata(ms).unwrap();
    assert!(img.exif.is_some());
    let format = img.format.expect("expected PNG format metadata");
    let ImageFormatMetadata::Png(text_chunks) = format else {
        panic!("expected Png format metadata variant");
    };
    assert_eq!(text_chunks.get("Title"), Some("PNG with EXIF"));
    assert_eq!(
        text_chunks.get("Software"),
        Some("nom-exif fixture builder")
    );
}

#[test]
fn parse_image_metadata_exif_png_from_memory() {
    let mut parser = MediaParser::new();
    let raw = std::fs::read("testdata/exif.png").unwrap();
    let ms = MediaSource::from_memory(raw).unwrap();
    let img = parser.parse_image_metadata(ms).unwrap();
    assert!(img.exif.is_some());
    assert!(img.format.is_some());
}

#[test]
fn parse_image_metadata_text_only_png_no_exif_but_format_present() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("testdata/text-only.png").unwrap();
    let img = parser.parse_image_metadata(ms).unwrap();
    assert!(img.exif.is_none());
    let format = img.format.expect("expected PNG format metadata");
    let ImageFormatMetadata::Png(text_chunks) = format else {
        panic!("expected Png format metadata variant");
    };
    assert_eq!(text_chunks.get("Title"), Some("Just text"));
}

#[cfg(feature = "tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn parse_image_metadata_async_exif_png() {
    use nom_exif::AsyncMediaSource;
    let mut parser = MediaParser::new();
    let ms = AsyncMediaSource::open("testdata/exif.png").await.unwrap();
    let img = parser.parse_image_metadata_async(ms).await.unwrap();
    assert!(img.exif.is_some());
    assert!(img.format.is_some());
}

#[test]
fn lazy_to_eager_conversion_works() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("testdata/exif.png").unwrap();
    let lazy = parser.parse_image_metadata(ms).unwrap();
    let eager: nom_exif::ImageMetadata = lazy.into();
    assert!(eager.exif.is_some());
    assert!(eager.format.is_some());
}

//! Runnable migration guide. Each test exercises the v3 side of one
//! migration row in `docs/MIGRATION.md`. Lives in `tests/` so it compiles
//! as a downstream crate would, validating the public API surface end-to-end.
//!
//! If you change the public surface and one of these breaks, **update the
//! corresponding row in `docs/MIGRATION.md` and the excerpt in CHANGELOG.md**
//! — the three artifacts are meant to stay in lock-step.
//!
//! Section names below preserve the historical `§5.x` ordering from the
//! original location of the migration table (V3_API_DESIGN.md §5); they map
//! 1:1 to the renumbered §1-§9 sections in `docs/MIGRATION.md`. §10 there
//! covers TrackInfo and is exercised by `crate::parser::tests::parse_track_info`
//! and friends in the unit tests.

use nom_exif::*;

// ─── §5.1 entry & parsing ──────────────────────────────────────────────────

#[test]
fn s5_1_media_source_open() {
    let ms = MediaSource::open("./testdata/exif.jpg").unwrap();
    assert_eq!(ms.kind(), MediaKind::Image);
}

#[test]
fn s5_1_top_level_read_exif() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    assert!(exif.get(ExifTag::Make).is_some());
}

#[test]
fn s5_1_parser_parse_exif() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("./testdata/exif.jpg").unwrap();
    let _iter = parser.parse_exif(ms).unwrap();
}

#[test]
fn s5_1_parser_parse_track() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("./testdata/meta.mov").unwrap();
    let _info: TrackInfo = parser.parse_track(ms).unwrap();
}

// ─── §5.2 errors ───────────────────────────────────────────────────────────

#[test]
fn s5_2_malformed_variant_pattern() {
    fn _classify(err: Error) -> &'static str {
        match err {
            Error::Malformed { .. } => "malformed",
            Error::UnexpectedEof { .. } => "eof",
            Error::UnsupportedFormat => "unsupported",
            Error::Io(_) => "io",
            Error::ExifNotFound => "no_exif",
            Error::TrackNotFound => "no_track",
            _ => "other",
        }
    }
}

#[test]
fn s5_2_malformed_kind_imports_from_top_level() {
    let _kind: MalformedKind = MalformedKind::IsoBmffBox;
}

// ─── §5.3 EntryValue accessors ─────────────────────────────────────────────

#[test]
fn s5_3_as_datetime_replaces_as_time_components() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    let dto = exif.get(ExifTag::DateTimeOriginal).unwrap();
    let _: Option<ExifDateTime> = dto.as_datetime();
}

#[test]
fn s5_3_as_u8_slice_replaces_as_u8array() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    if let Some(v) = exif.get(ExifTag::MakerNote) {
        let _: Option<&[u8]> = v.as_u8_slice();
    }
}

// ─── §5.4 ExifTag ──────────────────────────────────────────────────────────

#[test]
fn s5_4_exif_tag_from_code() {
    assert_eq!(ExifTag::from_code(0x010f), Some(ExifTag::Make));
    assert!(ExifTag::from_code(0xffff).is_none());
}

#[test]
fn s5_4_exif_tag_name_and_from_str() {
    use std::str::FromStr;
    assert_eq!(ExifTag::Make.name(), "Make");
    assert_eq!(ExifTag::Make.to_string(), "Make");
    assert_eq!(ExifTag::from_str("Make").unwrap(), ExifTag::Make);
    let err = ExifTag::from_str("Bogus").unwrap_err();
    assert!(matches!(err, ConvertError::UnknownTagName(_)));
}

// ─── §5.5 Exif / ExifIter ──────────────────────────────────────────────────

#[test]
fn s5_5_exif_gps_info() {
    let exif = read_exif("./testdata/exif.heic").unwrap();
    let _: Option<&GPSInfo> = exif.gps_info();
}

#[test]
fn s5_5_exif_get_by_code_and_get_in() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    let _ = exif.get_by_code(IfdIndex::MAIN, 0x0110);
    let _ = exif.get_in(IfdIndex::MAIN, ExifTag::Model);
}

#[test]
fn s5_5_exif_iter_yields_eager_entries() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    let n = exif.iter().filter(|e| e.ifd == IfdIndex::MAIN).count();
    assert!(n > 0);
}

#[test]
fn s5_5_exif_errors_accessor() {
    let exif = read_exif("./testdata/exif.jpg").unwrap();
    let _: &[(IfdIndex, TagOrCode, EntryError)] = exif.errors();
}

#[test]
fn s5_5_exif_iter_entry_into_result() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("./testdata/exif.jpg").unwrap();
    for entry in parser.parse_exif(ms).unwrap() {
        let _tag: TagOrCode = entry.tag();
        let _ = entry.into_result();
    }
}

#[test]
fn s5_5_exif_iter_clone_rewound_and_parse_gps() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("./testdata/exif.heic").unwrap();
    let iter = parser.parse_exif(ms).unwrap();
    let _gps: Option<GPSInfo> = iter.parse_gps().unwrap();
    let _twin = iter.clone_rewound();
}

#[test]
fn s5_5_has_embedded_media() {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open("./testdata/exif.heic").unwrap();
    let iter = parser.parse_exif(ms).unwrap();
    assert!(iter.has_embedded_media(), "HEIC carries embedded media");
    let exif: Exif = iter.into();
    assert!(exif.has_embedded_media());
}

// ─── §5.6 GPSInfo ──────────────────────────────────────────────────────────

#[test]
fn s5_6_lat_ref_enum_pattern() {
    let exif = read_exif("./testdata/exif.heic").unwrap();
    if let Some(g) = exif.gps_info() {
        let _ = matches!(g.latitude_ref, LatRef::North | LatRef::South);
        let _ = matches!(
            g.altitude,
            Altitude::AboveSeaLevel(_) | Altitude::BelowSeaLevel(_)
        );
    }
}

// ─── §5.7 Rational ─────────────────────────────────────────────────────────

#[test]
fn s5_7_rational_constructor_and_to_f64() {
    let r = URational::new(1, 2);
    assert_eq!(r.numerator(), 1);
    assert_eq!(r.denominator(), 2);
    assert_eq!(r.to_f64().unwrap(), 0.5);
}

#[test]
fn s5_7_irational_to_urational_conversion() {
    let pos: IRational = IRational::new(3, 4);
    let _u: URational = pos.try_into().unwrap();

    let neg: IRational = IRational::new(-3, 4);
    let err = URational::try_from(neg).unwrap_err();
    assert!(matches!(err, ConvertError::NegativeRational));
}

#[test]
fn s5_7_lat_lng_try_from_decimal_degrees() {
    let _ok = LatLng::try_from_decimal_degrees(43.5).unwrap();
    let err = LatLng::try_from_decimal_degrees(f64::NAN).unwrap_err();
    assert!(matches!(err, ConvertError::InvalidDecimalDegrees(_)));
}

// ─── §5.8 Async ────────────────────────────────────────────────────────────

#[cfg(feature = "tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn s5_8_async_top_level_helper() {
    let exif = read_exif_async("./testdata/exif.jpg").await.unwrap();
    assert!(exif.get(ExifTag::Make).is_some());
}

#[cfg(feature = "tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn s5_8_async_media_parser_method() {
    let mut parser = MediaParser::new();
    let ms = AsyncMediaSource::open("./testdata/exif.jpg").await.unwrap();
    let _iter = parser.parse_exif_async(ms).await.unwrap();
}

// ─── §5.9 Cargo features ───────────────────────────────────────────────────

#[cfg(feature = "serde")]
#[test]
fn s5_9_serde_derives_compile() {
    fn _is_serialize<T: serde::Serialize>() {}
    _is_serialize::<EntryValue>();
}

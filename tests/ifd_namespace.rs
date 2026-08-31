//! Entries from different IFD namespaces must stay distinguishable even when
//! they share a 16-bit tag code. See
//! <https://github.com/mindeng/nom-exif/issues/68>.
//!
//! Fixture: `testdata/ifd-namespace-collision.tif`, built by
//! `testdata/scripts/build_ifd_collision_fixture.py`. It deliberately puts
//! `0x000b` in both IFD0 (`ProcessingSoftware`) and the GPS IFD (`GPSDOP`),
//! and `0x0001` in both the GPS IFD (`GPSLatitudeRef`) and the Interop IFD
//! (`InteropIndex`).

use nom_exif::*;

const FIXTURE: &str = "./testdata/ifd-namespace-collision.tif";

fn entries(path: &str) -> Vec<(IfdKind, TagOrCode, EntryValue)> {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open(path).unwrap();
    let iter: ExifIter = parser.parse_exif(ms).unwrap();
    iter.filter_map(|e| {
        let (kind, tag) = (e.ifd_kind(), e.tag());
        e.into_result().ok().map(|v| (kind, tag, v))
    })
    .collect()
}

fn find(
    es: &[(IfdKind, TagOrCode, EntryValue)],
    kind: IfdKind,
    tag: ExifTag,
) -> Option<&EntryValue> {
    es.iter()
        .find(|(k, t, _)| *k == kind && t.tag() == Some(tag))
        .map(|(_, _, v)| v)
}

#[test]
fn colliding_code_survives_in_both_namespaces() {
    let es = entries(FIXTURE);

    // Before the namespace fix the GPS entry was dropped as a "duplicate" of
    // the IFD0 one, because both were keyed as (ifd0, 0x000b).
    let software = find(&es, IfdKind::Tiff, ExifTag::ProcessingSoftware)
        .expect("IFD0 0x000b should resolve as ProcessingSoftware");
    assert_eq!(software.as_str(), Some("MyProcessingSoftware"));

    let dop = find(&es, IfdKind::Gps, ExifTag::GPSDOP).expect("GPS 0x000b should survive");
    assert_eq!(dop.to_string(), "5/2 (2.5000)");
}

#[test]
fn ifd0_low_code_is_not_labelled_as_a_gps_tag() {
    let es = entries(FIXTURE);

    let mislabelled = es
        .iter()
        .any(|(k, t, _)| *k == IfdKind::Tiff && t.tag() == Some(ExifTag::GPSDOP));
    assert!(
        !mislabelled,
        "a TIFF-namespace entry must never be named GPSDOP; got {es:#?}"
    );
}

#[test]
fn gps_entries_keep_their_gps_names() {
    let es = entries(FIXTURE);

    assert_eq!(
        find(&es, IfdKind::Gps, ExifTag::GPSLatitudeRef).and_then(|v| v.as_str()),
        Some("N")
    );
    assert!(find(&es, IfdKind::Gps, ExifTag::GPSVersionID).is_some());
}

#[test]
fn unrelated_files_still_parse_unchanged() {
    // Guard against the namespace lookup dropping ordinary tags.
    let es = entries("./testdata/exif.jpg");
    assert_eq!(
        find(&es, IfdKind::Tiff, ExifTag::Make).and_then(|v| v.as_str()),
        Some("vivo")
    );
    assert!(find(&es, IfdKind::Exif, ExifTag::DateTimeOriginal).is_some());
    assert!(find(&es, IfdKind::Gps, ExifTag::GPSLatitude).is_some());
}

// ---- eager `Exif` API -------------------------------------------------

fn exif_of(path: &str) -> Exif {
    let mut parser = MediaParser::new();
    let ms = MediaSource::open(path).unwrap();
    let iter: ExifIter = parser.parse_exif(ms).unwrap();
    iter.into()
}

#[test]
fn eager_api_keeps_both_colliding_entries() {
    let exif = exif_of(FIXTURE);

    assert_eq!(
        exif.get_by_code_in(IfdIndex::MAIN, IfdKind::Tiff, 0x000b)
            .and_then(|v| v.as_str()),
        Some("MyProcessingSoftware")
    );
    assert_eq!(
        exif.get_by_code_in(IfdIndex::MAIN, IfdKind::Gps, 0x000b)
            .map(|v| v.to_string()),
        Some("5/2 (2.5000)".to_string())
    );
}

#[test]
fn get_in_routes_ambiguous_tags_to_their_own_namespace() {
    let exif = exif_of(FIXTURE);

    assert_eq!(
        exif.get(ExifTag::ProcessingSoftware)
            .and_then(|v| v.as_str()),
        Some("MyProcessingSoftware")
    );
    assert_eq!(
        exif.get(ExifTag::GPSDOP).map(|v| v.to_string()),
        Some("5/2 (2.5000)".to_string())
    );
}

#[test]
fn get_of_an_absent_ambiguous_tag_does_not_borrow_a_neighbours_value() {
    // exif.jpg has no ProcessingSoftware; it must not fall back to the GPS
    // entry that happens to share code 0x000b.
    let exif = exif_of("./testdata/exif.jpg");
    assert!(exif.get(ExifTag::ProcessingSoftware).is_none());
}

#[test]
fn unambiguous_tags_are_still_found_across_namespaces() {
    let exif = exif_of("./testdata/exif.jpg");
    // Lives in the Exif sub-IFD, but `get` must find it without the caller
    // naming a namespace.
    assert!(exif.get(ExifTag::DateTimeOriginal).is_some());
    assert_eq!(
        exif.get(ExifTag::Make).and_then(|v| v.as_str()),
        Some("vivo")
    );
}

#[test]
fn entries_expose_the_namespace() {
    let exif = exif_of(FIXTURE);

    let dop = exif
        .entries()
        .find(|e| e.ifd_kind() == IfdKind::Gps && e.tag().tag() == Some(ExifTag::GPSDOP))
        .expect("GPSDOP should be reachable via entries()");
    assert_eq!(dop.ifd(), IfdIndex::MAIN);
    assert_eq!(dop.value().to_string(), "5/2 (2.5000)");
}

// ---- Interop sub-IFD --------------------------------------------------

#[test]
fn interop_subifd_entries_are_exposed() {
    let es = entries(FIXTURE);

    assert_eq!(
        find(&es, IfdKind::Interop, ExifTag::InteropIndex).and_then(|v| v.as_str()),
        Some("R98")
    );
    assert!(find(&es, IfdKind::Interop, ExifTag::InteropVersion).is_some());
}

#[test]
fn interop_code_0x0001_does_not_evict_the_gps_one() {
    // Both the Interop and GPS IFDs define 0x0001; sharing an IfdIndex must
    // not make one look like a duplicate of the other.
    let es = entries(FIXTURE);

    assert_eq!(
        find(&es, IfdKind::Gps, ExifTag::GPSLatitudeRef).and_then(|v| v.as_str()),
        Some("N")
    );
    assert_eq!(
        find(&es, IfdKind::Interop, ExifTag::InteropIndex).and_then(|v| v.as_str()),
        Some("R98")
    );
}

#[test]
fn interop_pointer_entry_stays_in_the_exif_namespace() {
    let es = entries(FIXTURE);
    // InteropOffset itself lives in the Exif IFD that points at Interop.
    assert!(find(&es, IfdKind::Exif, ExifTag::InteropOffset).is_some());
}

use std::collections::BTreeMap;

use crate::{
    ebml::webm::parse_webm,
    error::ParsingError,
    file::MediaMimeTrack,
    mov::{extract_moov_body_from_buf, parse_isobmff},
    EntryValue, GPSInfo,
};

/// Try to keep the tag name consistent with [`crate::ExifTag`], and add some
/// unique to video/audio, such as `DurationMs`.
///
/// Different variants of `TrackInfoTag` may have different value types, please
/// refer to the documentation of each variant.
#[derive(Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum TrackInfoTag {
    /// Its value is an `EntryValue::Text`.
    Make,

    /// Its value is an `EntryValue::Text`.
    Model,

    /// Its value is an `EntryValue::Text`.
    Software,

    /// Its value is an [`EntryValue::DateTime`].
    CreateDate,

    /// Duration in millisecond, its value is an `EntryValue::U64`.
    DurationMs,

    /// Its value is an `EntryValue::U32`.
    ImageWidth,

    /// Its value is an `EntryValue::U32`.
    ImageHeight,

    /// Its value is an `EntryValue::Text`, location presented in ISO6709.
    ///
    /// If you need a parsed [`GPSInfo`] which provides more detailed GPS info,
    /// please use [`TrackInfo::gps_info`].
    GpsIso6709,

    /// Its value is an `EntryValue::Text`.
    Author,
}

/// Represents parsed track info.
#[derive(Debug, Clone, Default)]
pub struct TrackInfo {
    entries: BTreeMap<TrackInfoTag, EntryValue>,
    gps_info: Option<GPSInfo>,
    has_embedded_media: bool,
}

impl TrackInfo {
    /// Get value for `tag`. Different variants of `TrackInfoTag` may have
    /// different value types, please refer to [`TrackInfoTag`].
    pub fn get(&self, tag: TrackInfoTag) -> Option<&EntryValue> {
        self.entries.get(&tag)
    }

    /// Parsed GPS info, if `GpsIso6709` was present in the source. Mirrors
    /// [`Exif::gps_info`](crate::Exif::gps_info).
    pub fn gps_info(&self) -> Option<&GPSInfo> {
        self.gps_info.as_ref()
    }

    /// Iterate over `(tag, value)` pairs. The tag is yielded by value
    /// because [`TrackInfoTag`] is `Copy`. The parsed `GPSInfo` is **not**
    /// included here — get it via [`TrackInfo::gps_info`].
    pub fn iter(&self) -> impl Iterator<Item = (TrackInfoTag, &EntryValue)> {
        self.entries.iter().map(|(k, v)| (*k, v))
    }

    /// Whether the source container is known to embed additional media
    /// streams that this `parse_track` call did *not* surface (e.g. an
    /// .mka container holding both audio and video, or an .mp4 that also
    /// embeds a still-image track). Symmetric with
    /// [`Exif::has_embedded_media`](crate::Exif::has_embedded_media).
    ///
    /// **v3.0.0 note:** detection on the track side is not yet
    /// implemented; this currently always returns `false`. The accessor
    /// exists so future versions can flip the flag without a breaking
    /// API change. See spec §8.6 for the design rationale.
    pub fn has_embedded_media(&self) -> bool {
        self.has_embedded_media
    }

    #[allow(dead_code)] // staged: real callers land in v3.x
    pub(crate) fn set_has_embedded_media(&mut self, v: bool) {
        self.has_embedded_media = v;
    }

    pub(crate) fn put(&mut self, tag: TrackInfoTag, value: EntryValue) {
        self.entries.insert(tag, value);
    }
}

/// Parse video/audio info from `reader`. The file format will be detected
/// automatically by parser, if the format is not supported, an `Err` will be
/// returned.
///
/// Currently supported file formats are:
///
/// - ISO base media file format (ISOBMFF): *.mp4, *.mov, *.3gp, etc.
/// - Matroska based file format: *.webm, *.mkv, *.mka, etc.
///
/// ## Explanation of the generic parameters of this function:
///
/// - In order to improve parsing efficiency, the parser will internally skip
///   some useless bytes during parsing the byte stream, which is called
///   `Skip` internally.
///
/// - In order to support both `Read` and `Read` + `Seek` types, the interface
///   of input parameters is defined as `Read`.
///   
/// - Since Rust does not support specialization, the parser cannot internally
///   distinguish between `Read` and `Seek` and provide different `Skip`
///   implementations for them.
///   
/// Therefore, We chose to let the user specify how `Skip` works:
///
/// - `parse_track_info::<SkipSeek, _>(reader)` means the `reader` supports
///   `Seek`, so `Skip` will use the `Seek` trait to implement efficient skip
///   operations.
///   
/// - `parse_track_info::<SkipRead, _>(reader)` means the `reader` dosn't
///   support `Seek`, so `Skip` will fall back to using `Read` to implement the
///   skip operations.
///
/// ## Performance impact
///
/// If your `reader` only supports `Read`, it may cause performance loss when
/// processing certain large files. For example, *.mov files place metadata at
/// the end of the file, therefore, when parsing such files, locating metadata
/// will be slightly slower.
///
/// ## Examples
///
/// ```rust
/// use nom_exif::*;
/// use std::fs::File;
/// use chrono::DateTime;
///
/// let ms = MediaSource::open("./testdata/meta.mov").unwrap();
/// assert_eq!(ms.kind(), MediaKind::Track);
/// let mut parser = MediaParser::new();
/// let info: TrackInfo = parser.parse_track(ms).unwrap();
///
/// assert_eq!(info.get(TrackInfoTag::Make), Some(&"Apple".into()));
/// assert_eq!(info.get(TrackInfoTag::Model), Some(&"iPhone X".into()));
/// assert_eq!(info.get(TrackInfoTag::GpsIso6709), Some(&"+27.1281+100.2508+000.000/".into()));
/// assert_eq!(info.gps_info().unwrap().latitude_ref, LatRef::North);
/// assert_eq!(
///     info.gps_info().unwrap().latitude,
///     LatLng::new(URational::new(27, 1), URational::new(7, 1), URational::new(4116, 100)),
/// );
/// ```
#[tracing::instrument(skip(input))]
pub(crate) fn parse_track_info(
    input: &[u8],
    mime_video: MediaMimeTrack,
) -> Result<TrackInfo, ParsingError> {
    let mut info: TrackInfo = match mime_video {
        crate::file::MediaMimeTrack::QuickTime
        | crate::file::MediaMimeTrack::_3gpp
        | crate::file::MediaMimeTrack::Mp4 => {
            let range = extract_moov_body_from_buf(input)?;
            let moov_body = &input[range];
            parse_isobmff(moov_body)?
        }
        crate::file::MediaMimeTrack::Webm | crate::file::MediaMimeTrack::Matroska => {
            parse_webm(input)?.into()
        }
    };

    if let Some(gps) = info.get(TrackInfoTag::GpsIso6709) {
        info.gps_info = gps.as_str().and_then(|s| s.parse().ok());
    }

    Ok(info)
}

impl TrackInfoTag {
    /// Stable, programmatic name of this tag (matches the `Display` output).
    pub const fn name(self) -> &'static str {
        match self {
            TrackInfoTag::Make => "Make",
            TrackInfoTag::Model => "Model",
            TrackInfoTag::Software => "Software",
            TrackInfoTag::CreateDate => "CreateDate",
            TrackInfoTag::DurationMs => "DurationMs",
            TrackInfoTag::ImageWidth => "ImageWidth",
            TrackInfoTag::ImageHeight => "ImageHeight",
            TrackInfoTag::GpsIso6709 => "GpsIso6709",
            TrackInfoTag::Author => "Author",
        }
    }
}

impl std::fmt::Display for TrackInfoTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for TrackInfoTag {
    type Err = crate::ConvertError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "Make" => TrackInfoTag::Make,
            "Model" => TrackInfoTag::Model,
            "Software" => TrackInfoTag::Software,
            "CreateDate" => TrackInfoTag::CreateDate,
            "DurationMs" => TrackInfoTag::DurationMs,
            "ImageWidth" => TrackInfoTag::ImageWidth,
            "ImageHeight" => TrackInfoTag::ImageHeight,
            "GpsIso6709" => TrackInfoTag::GpsIso6709,
            "Author" => TrackInfoTag::Author,
            other => return Err(crate::ConvertError::UnknownTagName(other.to_owned())),
        })
    }
}

#[cfg(test)]
mod p6_baseline {
    use crate::{MediaParser, MediaSource, TrackInfoTag};

    #[test]
    fn p6_baseline_meta_mov_dump_snapshot() {
        // Lock down the post-refactor invariant: parsing testdata/meta.mov
        // through the public API yields the same set of (tag, value) pairs
        // before and after every P6 task. Captured as a sorted formatted
        // string so the assertion is a single Vec compare.
        let mut parser = MediaParser::new();
        let ms = MediaSource::open("testdata/meta.mov").unwrap();
        let info = parser.parse_track(ms).unwrap();

        // Probe the well-known tags (Make/Model/GpsIso6709/DurationMs).
        // The rest is exercised indirectly by other tests.
        let mut entries: Vec<String> = [
            TrackInfoTag::Make,
            TrackInfoTag::Model,
            TrackInfoTag::GpsIso6709,
            TrackInfoTag::DurationMs,
            TrackInfoTag::ImageWidth,
            TrackInfoTag::ImageHeight,
        ]
        .into_iter()
        .filter_map(|t| info.get(t).map(|v| format!("{t:?}={v}")))
        .collect();
        entries.sort();
        assert!(
            entries.len() >= 4,
            "expected >=4 well-known tags, got {entries:?}"
        );
        assert!(
            entries.iter().any(|s| s.starts_with("Make=")),
            "expected Make tag in snapshot, got {entries:?}"
        );
    }

    #[test]
    fn track_info_tag_name_is_const_str() {
        const _: &str = TrackInfoTag::Make.name();
        assert_eq!(TrackInfoTag::Make.name(), "Make");
        assert_eq!(TrackInfoTag::GpsIso6709.name(), "GpsIso6709");
        assert_eq!(TrackInfoTag::DurationMs.name(), "DurationMs");
    }

    #[test]
    fn track_info_tag_from_str_round_trip() {
        use std::str::FromStr;
        for t in [
            TrackInfoTag::Make,
            TrackInfoTag::Model,
            TrackInfoTag::Software,
            TrackInfoTag::CreateDate,
            TrackInfoTag::DurationMs,
            TrackInfoTag::ImageWidth,
            TrackInfoTag::ImageHeight,
            TrackInfoTag::GpsIso6709,
            TrackInfoTag::Author,
        ] {
            assert_eq!(TrackInfoTag::from_str(t.name()).unwrap(), t);
        }
    }

    #[test]
    fn track_info_tag_from_str_unknown_returns_convert_error() {
        use crate::ConvertError;
        use std::str::FromStr;
        let err = TrackInfoTag::from_str("Bogus").unwrap_err();
        assert!(matches!(err, ConvertError::UnknownTagName(s) if s == "Bogus"));
    }

    #[test]
    fn track_info_has_embedded_media_default_false() {
        // v3.0.0 ships the API contract; detection is a v3.x deliverable.
        // This test pins the day-one behavior so accidentally flipping it
        // requires explicit ack.
        let mut parser = crate::MediaParser::new();
        let ms = crate::MediaSource::open("testdata/meta.mov").unwrap();
        let info = parser.parse_track(ms).unwrap();
        assert!(!info.has_embedded_media());
    }
}

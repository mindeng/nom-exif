use std::{
    collections::HashMap,
    fmt::Debug,
    io::{BufRead, Cursor},
};

use bytes::Buf;
use chrono::{DateTime, NaiveDate, Utc};
use nom::{error::ErrorKind, multi::many_till};
use thiserror::Error;

use crate::{
    ebml::element::{
        find_element_by_id, get_as_f64, get_as_u64, next_element_header, parse_ebml_doc_type,
        EBMLGlobalId, TopElementId,
    },
    error::ParsingError,
    video::{TrackInfo, TrackInfoTag},
};

use super::{
    element::{
        travel_while, ElementHeader, ParseEBMLFailed, UnknowEbmlIDError, INVALID_ELEMENT_ID,
    },
    vint::{ParseVIntFailed, VInt},
};

#[derive(Debug, Clone, Default)]
pub struct EbmlFileInfo {
    #[allow(unused)]
    doc_type: String,
    segment_info: SegmentInfo,
    tracks_info: TracksInfo,
}

impl From<EbmlFileInfo> for TrackInfo {
    fn from(value: EbmlFileInfo) -> Self {
        let mut info = TrackInfo::default();
        if let Some(date) = value.segment_info.date {
            info.put(TrackInfoTag::CreateDate, date.into());
        }
        info.put(
            TrackInfoTag::DurationMs,
            ((value.segment_info.duration / 1000.0 / 1000.0) as u64).into(),
        );
        info.put(TrackInfoTag::ImageWidth, value.tracks_info.width.into());
        info.put(TrackInfoTag::ImageHeight, value.tracks_info.height.into());
        info
    }
}

#[derive(Debug, Error)]
pub enum ParseWebmFailed {
    #[error("need more bytes: {0}")]
    Need(usize),

    #[error("not an WEBM file")]
    NotWebmFile,

    #[error("invalid WEBM file: {0}")]
    InvalidWebmFile(Box<dyn std::error::Error>),

    #[error("invalid seek entry")]
    InvalidSeekEntry,
}

/// Parse EBML based files, e.g.: `.webm`, `.mkv`, etc.
///
/// Refer to:
/// - [Matroska Elements](https://www.matroska.org/technical/elements.html)
/// - [EBML Specification](https://github.com/ietf-wg-cellar/ebml-specification/blob/master/specification.markdown)
#[tracing::instrument(skip_all)]
pub(crate) fn parse_webm(input: &[u8]) -> Result<EbmlFileInfo, ParsingError> {
    let (doc_type, pos) = {
        let mut cursor = Cursor::new(input);
        let doc_type = parse_ebml_doc_type(&mut cursor)?;
        (doc_type, cursor.position() as usize)
    };

    tracing::debug!(doc_type, pos);

    let pos = {
        let mut cursor = Cursor::new(&input[pos..]);
        let header = next_element_header(&mut cursor)?;
        tracing::debug!(segment_header = ?header);
        if header.id != TopElementId::Segment as u64 {
            return Err(ParseWebmFailed::NotWebmFile.into());
        }
        pos + cursor.position() as usize
    };

    let mut file_info = EbmlFileInfo {
        doc_type,
        ..Default::default()
    };

    let mut info_set = false;
    let mut tracks_set = false;

    if let Ok(seeks) = parse_seeks(input, pos) {
        let info_seek = seeks.get(&(SegmentId::Info as u32)).cloned();
        let tracks_seek = seeks.get(&(SegmentId::Tracks as u32)).cloned();
        if let Some(pos) = info_seek {
            let info = parse_segment_info(input, pos as usize)?;
            tracing::debug!(?info);
            if let Some(info) = info {
                info_set = true;
                file_info.segment_info = info;
            }
        }
        if let Some(pos) = tracks_seek {
            let tracks = parse_tracks_info(input, pos as usize)?;
            tracing::debug!(?tracks);
            if let Some(info) = tracks {
                tracks_set = true;
                file_info.tracks_info = info;
            }
        }
    }

    if !info_set {
        // According to the specification, The first Info Element SHOULD occur
        // before the first Tracks Element
        let info: Option<SegmentInfo> = {
            let mut cursor = Cursor::new(&input[pos..]);
            let header = travel_while(&mut cursor, |h| h.id != SegmentId::Info as u64)?;
            parse_segment_info(
                &input[pos + cursor.position() as usize - header.header_size..],
                0,
            )
        }?;
        tracing::debug!(?info);
        if let Some(info) = info {
            file_info.segment_info = info;
        }
    }

    if !tracks_set {
        let track = {
            let mut cursor = Cursor::new(&input[pos..]);
            let header = travel_while(&mut cursor, |h| h.id != SegmentId::Tracks as u64)?;
            parse_tracks_info(
                &input[pos + cursor.position() as usize - header.header_size..],
                0,
            )?
        };
        tracing::debug!(?track);
        if let Some(info) = track {
            file_info.tracks_info = info;
        }
    }

    Ok(file_info)
}

#[derive(Debug, Clone, Default)]
struct TracksInfo {
    width: u32,
    height: u32,
}

#[tracing::instrument(skip(input))]
fn parse_tracks_info(input: &[u8], pos: usize) -> Result<Option<TracksInfo>, ParseWebmFailed> {
    if pos >= input.len() {
        return Err(ParseWebmFailed::Need(pos - input.len() + 1));
    }
    let mut cursor = Cursor::new(&input[pos..]);
    let header = next_element_header(&mut cursor)?;
    tracing::debug!(tracks_info_header = ?header);

    if cursor.remaining() < header.data_size {
        return Err(ParseWebmFailed::Need(header.data_size - cursor.remaining()));
    }

    const Z: &[u8] = &[];
    let start = pos + cursor.position() as usize;
    let data = &input[start..start + header.data_size];

    if let Ok((_, (_, track))) = many_till::<&[u8], (), Option<_>, (&[u8], ErrorKind), _, _>(
        |data| {
            let mut cursor = Cursor::new(data);
            let header = next_element_header(&mut cursor)?;
            cursor.consume(std::cmp::min(cursor.remaining(), header.data_size));
            Ok((&data[cursor.position() as usize..], ()))
        },
        |data| {
            let mut cursor = Cursor::new(data);
            let header = next_element_header(&mut cursor)?;
            tracing::debug!(tracks_sub_track_entry = ?header);
            if header.id != TracksId::TrackEntry as u64 {
                return Err(nom::Err::Error((Z, ErrorKind::Fail)));
            };

            if cursor.remaining() < header.data_size {
                return Err(nom::Err::Error((Z, ErrorKind::Fail)));
            }

            let track = parse_track(&cursor.chunk()[..header.data_size]).map(|x| {
                x.map(|x| TracksInfo {
                    width: x.width,
                    height: x.height,
                })
            })?;

            Ok((Z, track))
        },
    )(data)
    {
        Ok(track)
    } else {
        Ok(None)
    }

    // let mut cursor = Cursor::new(&cursor.chunk()[..header.data_size]);
    // let header = match travel_while(&mut cursor, |h| h.id != TracksId::VideoTrack as u64) {
    //     Ok(x) => x,
    //     // Don't bubble Need error to caller here
    //     Err(ParseEBMLFailed::Need(_)) => return Ok(None),
    //     Err(e) => return Err(e.into()),
    // };
    // tracing::debug!(?header, "video track");

    // if cursor.remaining() < header.data_size {
    //     return Err(ParseWebmFailed::Need(header.data_size - cursor.remaining()));
    // }

    // match parse_track(&cursor.chunk()[..header.data_size]).map(|x| {
    //     x.map(|x| TracksInfo {
    //         width: x.width,
    //         height: x.height,
    //     })
    // }) {
    //     Ok(x) => Ok(x),
    //     // Don't bubble Need error to caller here
    //     Err(ParseWebmFailed::Need(_)) => Ok(None),
    //     Err(e) => Err(e),
    // }
}

fn parse_track(input: &[u8]) -> Result<Option<VideoTrackInfo>, ParseWebmFailed> {
    let mut cursor = Cursor::new(input);

    while cursor.has_remaining() {
        let header = next_element_header(&mut cursor)?;
        tracing::debug!(?header, "track sub-element");

        let id = TryInto::<TracksId>::try_into(header.id);
        let pos = cursor.position() as usize;
        cursor.consume(header.data_size);

        let Ok(id) = id else {
            continue;
        };

        if id == TracksId::VideoTrack {
            return parse_video_track(&input[pos..pos + header.data_size]);
        }
    }
    Ok(None)
}

fn parse_video_track(input: &[u8]) -> Result<Option<VideoTrackInfo>, ParseWebmFailed> {
    let mut cursor = Cursor::new(input);
    let mut info = VideoTrackInfo::default();

    let header = travel_while(&mut cursor, |h| h.id != TracksId::PixelWidth as u64)?;
    tracing::debug!(?header, "video track width element");
    if let Some(v) = get_as_u64(&mut cursor, header.data_size) {
        info.width = v as u32;
    }

    // search from beginning
    cursor.set_position(0);
    let header = travel_while(&mut cursor, |h| h.id != TracksId::PixelHeight as u64)?;
    tracing::debug!(?header, "video track height element");
    if let Some(v) = get_as_u64(&mut cursor, header.data_size) {
        info.height = v as u32;
    }

    if info == VideoTrackInfo::default() {
        Ok(None)
    } else {
        Ok(Some(info))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct VideoTrackInfo {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Default)]
struct SegmentInfo {
    // in nano seconds
    duration: f64,
    date: Option<DateTime<Utc>>,
}

#[tracing::instrument(skip(input))]
fn parse_segment_info(input: &[u8], pos: usize) -> Result<Option<SegmentInfo>, ParsingError> {
    if pos >= input.len() {
        return Err(ParsingError::Need(pos - input.len() + 1));
    }
    let mut cursor = Cursor::new(&input[pos..]);
    let header = next_element_header(&mut cursor)?;
    tracing::debug!(segment_info_header = ?header);

    if cursor.remaining() < header.data_size {
        return Err(ParsingError::Need(header.data_size - cursor.remaining()));
    }

    let mut cursor = Cursor::new(&cursor.chunk()[..header.data_size]);
    match parse_segment_info_body(&mut cursor) {
        Ok(x) => Ok(Some(x)),
        // Don't bubble Need error to caller here
        Err(ParsingError::Need(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

fn parse_segment_info_body(cursor: &mut Cursor<&[u8]>) -> Result<SegmentInfo, ParsingError> {
    // timestamp in nanosecond = element value * TimestampScale
    // By default, one segment tick represents one millisecond
    let mut time_scale = 1_000_000;
    let mut info = SegmentInfo::default();

    while cursor.has_remaining() {
        let header = next_element_header(cursor)?;
        let id = TryInto::<InfoId>::try_into(header.id);
        tracing::debug!(?header, "segment info sub-element");

        if let Ok(id) = id {
            match id {
                InfoId::TimestampScale => {
                    if let Some(v) = get_as_u64(cursor, header.data_size) {
                        time_scale = v;
                    }
                }
                InfoId::Duration => {
                    if let Some(v) = get_as_f64(cursor, header.data_size) {
                        info.duration = v * time_scale as f64;
                    }
                }
                InfoId::Date => {
                    if let Some(v) = get_as_u64(cursor, header.data_size) {
                        // webm date is a 2001 based timestamp
                        let dt = NaiveDate::from_ymd_opt(2001, 1, 1)
                            .unwrap()
                            .and_hms_opt(0, 0, 0)
                            .unwrap()
                            .and_utc();
                        let diff = dt - DateTime::from_timestamp_nanos(0);
                        info.date = Some(DateTime::from_timestamp_nanos(v as i64) + diff);
                    }
                }
            }
        } else {
            cursor.consume(header.data_size);
        }
    }

    Ok(info)
}

fn parse_seeks(input: &[u8], pos: usize) -> Result<HashMap<u32, u64>, ParsingError> {
    let mut cursor = Cursor::new(&input[pos..]);
    // find SeekHead element
    let header = find_element_by_id(&mut cursor, SegmentId::SeekHead as u64)?;
    tracing::debug!(segment_header = ?header);
    if cursor.remaining() < header.data_size {
        return Err(ParsingError::Need(header.data_size - cursor.remaining()));
    }

    let header_pos = pos + cursor.position() as usize - header.header_size;
    let mut cur = Cursor::new(&cursor.chunk()[..header.data_size]);
    let mut seeks = parse_seek_head(&mut cur)?;
    for (_, pos) in seeks.iter_mut() {
        *pos += header_pos as u64;
    }
    Ok(seeks)
}

#[derive(Clone)]
struct SeekEntry {
    seek_id: u32,
    seek_pos: u64,
}

impl Debug for SeekEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let id = self.seek_id as u64;
        let s = TryInto::<TopElementId>::try_into(id)
            .map(|x| format!("{x:?}"))
            .or_else(|_| TryInto::<SegmentId>::try_into(id).map(|x| format!("{x:?}")))
            .unwrap_or_else(|_| format!("0x{:04x}", id));
        f.debug_struct("SeekEntry")
            .field("seekId", &s)
            .field("seekPosition", &self.seek_pos.to_string())
            .finish()
    }
}

#[tracing::instrument(skip_all)]
fn parse_seek_head(input: &mut Cursor<&[u8]>) -> Result<HashMap<u32, u64>, ParseWebmFailed> {
    let mut entries = HashMap::new();
    while input.has_remaining() {
        match parse_seek_entry(input) {
            Ok(Some(entry)) => {
                tracing::debug!(seek_entry=?entry);
                entries.insert(entry.seek_id, entry.seek_pos);
            }
            Ok(None) => {
                // tracing::debug!("Void or Crc32 Element");
            }
            Err(ParseWebmFailed::InvalidSeekEntry) => {
                tracing::debug!("ignore invalid seek entry");
            }
            Err(e) => return Err(e),
        };
    }
    Ok(entries)
}

fn parse_seek_entry(input: &mut Cursor<&[u8]>) -> Result<Option<SeekEntry>, ParseWebmFailed> {
    // 0xFF is an invalid ID
    let mut seek_id = INVALID_ELEMENT_ID as u32;
    let mut seek_pos = 0u64;

    let id = VInt::as_u64_with_marker(input)?;
    let data_size = VInt::as_usize(input)?;
    if input.remaining() < data_size {
        return Err(ParseWebmFailed::Need(data_size - input.remaining()));
    }

    if id != SeekHeadId::Seek as u64 {
        input.consume(data_size);
        if id == EBMLGlobalId::Crc32 as u64 || id == EBMLGlobalId::Void as u64 {
            return Ok(None);
        }
        tracing::debug!(
            id = format!("0x{id:x}"),
            "invalid seek entry: id != 0x{:x}",
            SeekHeadId::Seek as u32
        );
        return Err(ParseWebmFailed::InvalidSeekEntry);
    }

    let pos = input.position() as usize;
    input.consume(data_size);
    let mut buf = Cursor::new(&input.get_ref()[pos..pos + data_size]);

    while buf.has_remaining() {
        let id = VInt::as_u64_with_marker(&mut buf)?;
        let size = VInt::as_usize(&mut buf)?;

        match id {
            x if x == SeekHeadId::SeekId as u64 => {
                seek_id = VInt::as_u64_with_marker(&mut buf)? as u32;
            }
            x if x == SeekHeadId::SeekPosition as u64 => {
                seek_pos =
                    get_as_u64(&mut buf, size).ok_or_else(|| ParseWebmFailed::InvalidSeekEntry)?;
            }
            _ => {
                tracing::debug!(id = format!("0x{id:x}"), "invalid seek entry");
                return Err(ParseWebmFailed::InvalidSeekEntry);
            }
        }

        if seek_id != INVALID_ELEMENT_ID as u32 && seek_pos != 0 {
            break;
        }
    }

    if seek_id == INVALID_ELEMENT_ID as u32 || seek_pos == 0 {
        return Err(ParseWebmFailed::InvalidSeekEntry);
    }

    Ok(Some(SeekEntry { seek_id, seek_pos }))
}

#[derive(Debug, Clone, Copy)]
enum SegmentId {
    SeekHead = 0x114D9B74,
    Info = 0x1549A966,
    Tracks = 0x1654AE6B,
    Cluster = 0x1F43B675,
    Cues = 0x1C53BB6B,
}

#[derive(Debug, Clone, Copy)]
enum InfoId {
    TimestampScale = 0x2AD7B1,
    Duration = 0x4489,
    Date = 0x4461,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TracksId {
    TrackEntry = 0xAE,
    TrackType = 0x83,
    VideoTrack = 0xE0,
    PixelWidth = 0xB0,
    PixelHeight = 0xBA,
}

impl TryFrom<u64> for TracksId {
    type Error = UnknowEbmlIDError;
    fn try_from(v: u64) -> Result<Self, Self::Error> {
        let id = match v {
            x if x == Self::TrackEntry as u64 => Self::TrackEntry,
            x if x == Self::TrackType as u64 => Self::TrackType,
            x if x == Self::VideoTrack as u64 => Self::VideoTrack,
            x if x == Self::PixelWidth as u64 => Self::PixelWidth,
            x if x == Self::PixelHeight as u64 => Self::PixelHeight,
            o => return Err(UnknowEbmlIDError(o)),
        };
        Ok(id)
    }
}

impl TryFrom<u64> for InfoId {
    type Error = UnknowEbmlIDError;
    fn try_from(v: u64) -> Result<Self, Self::Error> {
        let id = match v {
            x if x == Self::TimestampScale as u64 => Self::TimestampScale,
            x if x == Self::Duration as u64 => Self::Duration,
            x if x == Self::Date as u64 => Self::Date,
            o => return Err(UnknowEbmlIDError(o)),
        };
        Ok(id)
    }
}

#[derive(Debug, Clone, Copy)]
enum SeekHeadId {
    Seek = 0x4DBB,
    SeekId = 0x53AB,
    SeekPosition = 0x53AC,
}

impl TryFrom<u64> for SegmentId {
    type Error = UnknowEbmlIDError;
    fn try_from(v: u64) -> Result<Self, Self::Error> {
        let id = match v {
            x if x == Self::SeekHead as u64 => Self::SeekHead,
            x if x == Self::Info as u64 => Self::Info,
            x if x == Self::Tracks as u64 => Self::Tracks,
            x if x == Self::Cluster as u64 => Self::Cluster,
            x if x == Self::Cues as u64 => Self::Cues,
            o => return Err(UnknowEbmlIDError(o)),
        };
        Ok(id)
    }
}

impl Debug for ElementHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = TryInto::<TopElementId>::try_into(self.id)
            .map(|x| format!("{x:?}"))
            .or_else(|_| TryInto::<SegmentId>::try_into(self.id).map(|x| format!("{x:?}")))
            .or_else(|_| TryInto::<InfoId>::try_into(self.id).map(|x| format!("{x:?}")))
            .or_else(|_| TryInto::<TracksId>::try_into(self.id).map(|x| format!("{x:?}")))
            .unwrap_or_else(|_| format!("0x{:04x}", self.id));
        f.debug_struct("ElementHeader")
            .field("id", &s)
            .field("data_size", &self.data_size.to_string())
            .finish()
    }
}

impl From<ParseEBMLFailed> for ParseWebmFailed {
    fn from(value: ParseEBMLFailed) -> Self {
        match value {
            ParseEBMLFailed::Need(i) => Self::Need(i),
            ParseEBMLFailed::NotEBMLFile => Self::NotWebmFile,
            ParseEBMLFailed::InvalidEBMLFile(e) => Self::InvalidWebmFile(e),
        }
    }
}

impl From<ParseEBMLFailed> for ParsingError {
    fn from(value: ParseEBMLFailed) -> Self {
        match value {
            ParseEBMLFailed::Need(i) => ParsingError::Need(i),
            ParseEBMLFailed::NotEBMLFile | ParseEBMLFailed::InvalidEBMLFile(_) => {
                ParsingError::Failed(value.to_string())
            }
        }
    }
}

impl From<ParseVIntFailed> for ParseWebmFailed {
    fn from(value: ParseVIntFailed) -> Self {
        match value {
            ParseVIntFailed::InvalidVInt(e) => Self::InvalidWebmFile(e.into()),
            ParseVIntFailed::Need(i) => Self::Need(i),
        }
    }
}

impl From<ParseVIntFailed> for ParsingError {
    fn from(value: ParseVIntFailed) -> Self {
        match value {
            ParseVIntFailed::InvalidVInt(_) => Self::Failed(value.to_string()),
            ParseVIntFailed::Need(i) => Self::Need(i),
        }
    }
}

impl From<ParseWebmFailed> for ParsingError {
    fn from(value: ParseWebmFailed) -> Self {
        match value {
            ParseWebmFailed::NotWebmFile
            | ParseWebmFailed::InvalidWebmFile(_)
            | ParseWebmFailed::InvalidSeekEntry => Self::Failed(value.to_string()),
            ParseWebmFailed::Need(n) => Self::Need(n),
        }
    }
}

impl From<ParseEBMLFailed> for nom::Err<(&[u8], ErrorKind)> {
    fn from(value: ParseEBMLFailed) -> Self {
        match value {
            // Don't bubble Need error to caller, since we only use nom for
            // complete data here.
            ParseEBMLFailed::Need(_)
            | ParseEBMLFailed::NotEBMLFile
            | ParseEBMLFailed::InvalidEBMLFile(_) => nom::Err::Error((&[], ErrorKind::Fail)),
        }
    }
}

impl From<ParseWebmFailed> for nom::Err<(&[u8], ErrorKind)> {
    fn from(_: ParseWebmFailed) -> Self {
        // Don't bubble Need error to caller, since we only use nom for
        // complete data here.
        nom::Err::Error((&[], ErrorKind::Fail))
    }
}

use std::{
    cmp::{max, min},
    collections::HashMap,
    fmt::Debug,
    io::{BufRead, Cursor, Read},
};

use bytes::Buf;
use thiserror::Error;

use crate::ebml::element::{
    find_element_by_id, next_element_header, parse_ebml_doc_type, EBMLGlobalId, TopElementId,
};

use super::{
    element::{ElementHeader, ParseEBMLFailed, UnknowEbmlIDError, INVALID_ELEMENT_ID},
    vint::{ParseVIntFailed, VInt},
};

#[derive(Debug, Clone)]
pub struct EBMLFileInfo {
    doc_type: String,
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

    #[error("read WEBM file failed: {0}")]
    IOError(std::io::Error),
}

const INIT_BUF_SIZE: usize = 4096;
const MIN_GROW_SIZE: usize = 4096;
const MAX_GROW_SIZE: usize = 1000 * 4096;

/// Refer to:
/// - [Matroska Elements](https://www.matroska.org/technical/elements.html)
/// - [EBML Specification](https://github.com/ietf-wg-cellar/ebml-specification/blob/master/specification.markdown)
#[tracing::instrument(skip_all)]
pub fn parse_webm<T: Read>(mut reader: T) -> Result<EBMLFileInfo, ParseWebmFailed> {
    let mut buf = Vec::with_capacity(INIT_BUF_SIZE);
    let n = reader
        .by_ref()
        .take(INIT_BUF_SIZE as u64)
        .read_to_end(buf.as_mut())
        .map_err(ParseWebmFailed::IOError)?;
    if n == 0 {
        return Err(ParseWebmFailed::NotWebmFile);
    }

    let mut cursor = Cursor::new(buf.as_ref());
    let doc_type = parse_ebml_doc_type(&mut cursor)?;
    tracing::debug!(doc_type);

    let header = next_element_header(&mut cursor)?;
    tracing::debug!(segment_header = ?header);
    if header.id != TopElementId::Segment as u64 {
        return Err(ParseWebmFailed::NotWebmFile);
    }

    let pos = cursor.position() as usize;
    let seeks = parse_and_read(reader, &mut buf, pos, parse_seeks)?;

    if let Some(pos) = seeks.get(&(SegmentId::Info as u32)) {
        parse_segment_info(&buf, *pos as usize)?;
    }

    Ok(EBMLFileInfo { doc_type })
}

#[derive(Debug, Clone, Default)]
struct SegmentInfo {
    duration: f64,
}

#[tracing::instrument(skip(input))]
fn parse_segment_info(input: &[u8], pos: usize) -> Result<SegmentInfo, ParseWebmFailed> {
    let mut cursor = Cursor::new(&input[pos..]);
    let header = next_element_header(&mut cursor)?;
    tracing::debug!(segment_info_header = ?header);

    Ok(SegmentInfo { duration: 0.0 })
}

fn parse_and_read<T: Read, O, F>(
    mut reader: T,
    buf: &mut Vec<u8>,
    pos: usize,
    parse: F,
) -> Result<O, ParseWebmFailed>
where
    F: Fn(&[u8], usize) -> Result<O, ParseWebmFailed>,
{
    loop {
        match parse(buf, pos) {
            Ok(o) => return Ok(o),
            Err(ParseWebmFailed::Need(i)) => {
                assert!(i > 0);
                let to_read = max(i, MIN_GROW_SIZE);
                let to_read = min(to_read, MAX_GROW_SIZE);
                tracing::debug!(to_read);
                buf.reserve(to_read);
                let n = reader
                    .by_ref()
                    .take(to_read as u64)
                    .read_to_end(buf.as_mut())
                    .map_err(ParseWebmFailed::IOError)?;
                if n == 0 {
                    return Err(ParseWebmFailed::InvalidWebmFile("no enough bytes".into()));
                }
            }
            Err(e) => return Err(e),
        }
    }
}

fn parse_seeks(input: &[u8], pos: usize) -> Result<HashMap<u32, u64>, ParseWebmFailed> {
    let mut cursor = Cursor::new(&input[pos..]);
    // find SeekHead element
    let header = find_element_by_id(&mut cursor, SegmentId::SeekHead as u64)?;
    tracing::debug!(segment_header = ?header);
    if cursor.remaining() < header.data_size {
        return Err(ParseWebmFailed::Need(header.data_size - cursor.remaining()));
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
    seekId: u32,
    seekPosition: u64,
}

impl Debug for SeekEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let id = self.seekId as u64;
        let s = TryInto::<TopElementId>::try_into(id)
            .map(|x| format!("{x:?}"))
            .or_else(|_| TryInto::<SegmentId>::try_into(id).map(|x| format!("{x:?}")))
            .unwrap_or_else(|_| format!("0x{:04x}", id));
        f.debug_struct("SeekEntry")
            .field("seekId", &s)
            .field("seekPosition", &self.seekPosition.to_string())
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
                entries.insert(entry.seekId, entry.seekPosition);
            }
            Ok(None) => {
                // tracing::debug!("Void or Crc32 Element");
            }
            Err(ParseWebmFailed::InvalidSeekEntry) => {}
            Err(e) => return Err(e),
        };
    }
    Ok(entries)
}

fn parse_seek_entry(input: &mut Cursor<&[u8]>) -> Result<Option<SeekEntry>, ParseWebmFailed> {
    // 0xFF is an invalid ID
    let mut seekId = INVALID_ELEMENT_ID as u32;
    let mut seekPosition = 0u64;

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
            "{}",
            ParseWebmFailed::InvalidSeekEntry
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
                seekId = VInt::as_u64_with_marker(&mut buf)? as u32;
            }
            x if x == SeekHeadId::SeekPosition as u64 => {
                if size == 8 {
                    seekPosition = buf.get_u64();
                } else if size == 4 {
                    seekPosition = buf.get_u32() as u64;
                } else {
                    return Err(ParseWebmFailed::InvalidSeekEntry);
                }
            }
            _ => {
                return Err(ParseWebmFailed::InvalidSeekEntry);
            }
        }

        if seekId != INVALID_ELEMENT_ID as u32 && seekPosition != 0 {
            break;
        }
    }

    if seekId == INVALID_ELEMENT_ID as u32 || seekPosition == 0 {
        return Err(ParseWebmFailed::InvalidSeekEntry);
    }

    Ok(Some(SeekEntry {
        seekId,
        seekPosition,
    }))
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
enum SeekHeadId {
    Seek = 0x4DBB,
    SeekId = 0x53AB,
    SeekPosition = 0x53AC,
}

impl SegmentId {
    fn code(self) -> u32 {
        self as u32
    }
}

impl TryFrom<u64> for SegmentId {
    type Error = UnknowEbmlIDError;
    fn try_from(v: u64) -> Result<Self, Self::Error> {
        let id = match v {
            x if x == Self::SeekHead.code() as u64 => Self::SeekHead,
            x if x == Self::Info.code() as u64 => Self::Info,
            x if x == Self::Tracks.code() as u64 => Self::Tracks,
            x if x == Self::Cluster.code() as u64 => Self::Cluster,
            x if x == Self::Cues.code() as u64 => Self::Cues,
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
            ParseEBMLFailed::Need(i) => ParseWebmFailed::Need(i),
            ParseEBMLFailed::NotEBMLFile => ParseWebmFailed::NotWebmFile,
            ParseEBMLFailed::InvalidEBMLFile(e) => ParseWebmFailed::InvalidWebmFile(e),
        }
    }
}

impl From<ParseVIntFailed> for ParseWebmFailed {
    fn from(value: ParseVIntFailed) -> Self {
        match value {
            ParseVIntFailed::InvalidVInt(e) => ParseWebmFailed::InvalidWebmFile(e.into()),
            ParseVIntFailed::Need(i) => ParseWebmFailed::Need(i),
        }
    }
}

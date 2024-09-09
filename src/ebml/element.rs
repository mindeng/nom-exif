use std::{
    fmt::Debug,
    io::{BufRead, Cursor, Read},
};

use bytes::Buf;
use thiserror::Error;

use crate::ebml::vint::VInt;

use super::vint::ParseVIntFailed;

#[derive(Debug, Error)]
pub enum ParseEBMLFailed {
    #[error("need more bytes: {0}")]
    Need(usize),

    #[error("not an EBML file")]
    NotEBMLFile,

    #[error("invalid EBML file: {0}")]
    InvalidEBMLFile(Box<dyn std::error::Error + Send + Sync>),
}

impl From<ParseEBMLFailed> for crate::Error {
    fn from(e: ParseEBMLFailed) -> Self {
        match e {
            ParseEBMLFailed::Need(_) => Self::ParseFailed("no enough bytes".into()),
            ParseEBMLFailed::NotEBMLFile => Self::ParseFailed(e.into()),
            ParseEBMLFailed::InvalidEBMLFile(e) => Self::ParseFailed(e),
        }
    }
}

impl From<ParseVIntFailed> for ParseEBMLFailed {
    fn from(value: ParseVIntFailed) -> Self {
        match value {
            ParseVIntFailed::InvalidVInt(e) => ParseEBMLFailed::InvalidEBMLFile(e.into()),
            ParseVIntFailed::Need(i) => ParseEBMLFailed::Need(i),
        }
    }
}

pub(crate) const INVALID_ELEMENT_ID: u8 = 0xFF;

#[derive(Debug, Clone, Copy)]
pub(crate) enum TopElementId {
    Ebml = 0x1A45DFA3,
    Segment = 0x18538067,
}

impl TopElementId {
    fn code(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Error)]
#[error("unknown ebml ID: {0}")]
pub struct UnknowEbmlIDError(pub u64);

impl TryFrom<u64> for TopElementId {
    type Error = UnknowEbmlIDError;
    fn try_from(v: u64) -> Result<Self, Self::Error> {
        let id = match v {
            x if x == TopElementId::Ebml.code() as u64 => TopElementId::Ebml,
            x if x == TopElementId::Segment.code() as u64 => TopElementId::Segment,
            o => return Err(UnknowEbmlIDError(o)),
        };
        Ok(id)
    }
}

#[allow(unused)]
#[derive(Debug, Clone, Copy)]
enum EBMLHeaderId {
    Version = 0x4286,
    ReadVersion = 0x42F7,
    MaxIdlength = 0x42F2,
    MaxSizeLength = 0x42F3,
    DocType = 0x4282,
    DocTypeVersion = 0x4287,
    DocTypeReadVersion = 0x4285,
    DocTypeExtension = 0x4281,
    DocTypeExtensionName = 0x4283,
    DocTypeExtensionVersion = 0x4284,
}

/// These extra elements apply only to the EBML Body, not the EBML Header.
pub(crate) enum EBMLGlobalId {
    Crc32 = 0xBF,
    Void = 0xEC,
}

/// Refer to [EBML header
/// elements](https://github.com/ietf-wg-cellar/ebml-specification/blob/master/specification.markdown#ebml-header-elements)
pub(crate) fn parse_ebml_doc_type(cursor: &mut Cursor<&[u8]>) -> Result<String, ParseEBMLFailed> {
    let header = next_element_header(cursor)?;
    tracing::debug!(ebml_header = ?header);

    if header.id != TopElementId::Ebml as u64 {
        return Err(ParseEBMLFailed::NotEBMLFile);
    }

    if cursor.remaining() < header.data_size {
        return Err(ParseEBMLFailed::Need(header.data_size - cursor.remaining()));
    }

    let pos = cursor.position() as usize;
    // consume all header data
    cursor.consume(header.data_size);

    // get doc type
    match parse_ebml_head_data(&cursor.get_ref()[pos..pos + header.data_size]) {
        Ok(x) => Ok(x),
        // Don't bubble Need error to caller here
        Err(ParseEBMLFailed::Need(_)) => Err(ParseEBMLFailed::NotEBMLFile),
        Err(e) => Err(e),
    }
}

fn parse_ebml_head_data(input: &[u8]) -> Result<String, ParseEBMLFailed> {
    let mut cur = Cursor::new(input);
    while cur.has_remaining() {
        let h = next_element_header(&mut cur)?;

        if h.id == EBMLHeaderId::DocType as u64 {
            let s = get_cstr(&mut cur, h.data_size)
                .ok_or_else(|| ParseEBMLFailed::Need(h.data_size - cur.remaining()))?;
            return Ok(s);
        }
        if cur.remaining() < h.data_size {
            return Err(ParseEBMLFailed::Need(h.data_size - cur.remaining()));
        }
        cur.consume(h.data_size);
    }
    Err(ParseEBMLFailed::NotEBMLFile)
}

pub(crate) fn find_element_by_id(
    cursor: &mut Cursor<&[u8]>,
    target_id: u64,
) -> Result<ElementHeader, ParseEBMLFailed> {
    while cursor.has_remaining() {
        let header = next_element_header(cursor)?;
        if header.id == target_id {
            return Ok(header);
        }
        if cursor.remaining() < header.data_size {
            return Err(ParseEBMLFailed::Need(header.data_size - cursor.remaining()));
        }

        cursor.consume(header.data_size);
    }
    Err(ParseEBMLFailed::Need(1))
}

pub(crate) fn travel_while<F>(
    cursor: &mut Cursor<&[u8]>,
    mut predict: F,
) -> Result<ElementHeader, ParseEBMLFailed>
where
    F: FnMut(&ElementHeader) -> bool,
{
    while cursor.has_remaining() {
        let header = next_element_header(cursor)?;
        if !predict(&header) {
            return Ok(header);
        }
        if cursor.remaining() < header.data_size {
            return Err(ParseEBMLFailed::Need(header.data_size - cursor.remaining()));
        }

        cursor.consume(header.data_size);
    }
    Err(ParseEBMLFailed::Need(1))
}

#[derive(Clone)]
pub(crate) struct ElementHeader {
    pub id: u64,
    pub data_size: usize,
    pub header_size: usize,
}

pub(crate) fn next_element_header(
    cursor: &mut Cursor<&[u8]>,
) -> Result<ElementHeader, ParseEBMLFailed> {
    let pos = cursor.position() as usize;
    let id = VInt::as_u64_with_marker(cursor)?;
    let data_size = VInt::as_usize(cursor)?;
    let header_size = cursor.position() as usize - pos;

    Ok(ElementHeader {
        id,
        data_size,
        header_size,
    })
}

fn get_cstr(cursor: &mut Cursor<&[u8]>, size: usize) -> Option<String> {
    if cursor.remaining() < size {
        return None;
    }
    let it = Iterator::take(cursor.chunk().iter(), size);
    let s = it
        .take_while(|b| **b != 0)
        .map(|b| (*b) as char)
        .collect::<String>();
    cursor.consume(size);
    Some(s)
}

pub(crate) fn get_as_u64(cursor: &mut Cursor<&[u8]>, size: usize) -> Option<u64> {
    if cursor.remaining() < size {
        return None;
    }

    let n = match size {
        1 => cursor.get_u8() as u64,
        2 => cursor.get_u16() as u64,
        3 => {
            let bytes = [0, cursor.get_u8(), cursor.get_u8(), cursor.get_u8()];
            u32::from_be_bytes(bytes) as u64
        }
        4 => cursor.get_u32() as u64,
        5..=8 => {
            let mut buf = [0u8; 8];
            cursor.read_exact(&mut buf[8 - size..]).ok()?;
            u64::from_be_bytes(buf)
        }
        _ => return None,
    };

    Some(n)
}

pub(crate) fn get_as_f64(cursor: &mut Cursor<&[u8]>, size: usize) -> Option<f64> {
    if cursor.remaining() < size {
        return None;
    }

    let n = match size {
        4 => {
            let buf = [0u8; 4];
            f32::from_be_bytes(buf) as f64
        }
        5..=8 => {
            let mut buf = [0u8; 8];
            cursor.read_exact(&mut buf[8 - size..]).ok()?;
            f64::from_be_bytes(buf)
        }
        _ => return None,
    };

    Some(n)
}

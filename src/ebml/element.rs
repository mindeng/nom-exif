use std::{fmt::Debug, io::Cursor};

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
    InvalidEBMLFile(Box<dyn std::error::Error>),
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
#[tracing::instrument(skip_all)]
pub(crate) fn parse_ebml_doc_type(mut input: &[u8]) -> Result<(&[u8], String), ParseEBMLFailed> {
    let (remain, header) = next_element_header(input)?;
    input.advance(input.remaining() - remain.len());
    tracing::debug!(?header);

    if header.id != TopElementId::Ebml as u64 {
        return Err(ParseEBMLFailed::NotEBMLFile);
    }

    if input.remaining() < header.data_size {
        return Err(ParseEBMLFailed::Need(header.data_size - input.remaining()));
    }

    let data = &remain[..header.data_size];
    // consume header data
    input.advance(header.data_size);

    // get doc type
    while !data.is_empty() {
        let (remain, h) = next_element_header(data)?;

        if remain.len() < h.data_size {
            return Err(ParseEBMLFailed::Need(h.data_size - remain.len()));
        }

        if h.id == EBMLHeaderId::DocType as u64 {
            let s = as_cstr(&remain[..h.data_size]);
            return Ok((input, s));
        }
    }

    Err(ParseEBMLFailed::NotEBMLFile)
}

struct Element<'a> {
    id: u64,
    data: &'a [u8],
}

pub(crate) fn find_element_by_id(
    mut input: &[u8],
    target_id: u64,
) -> Result<(&[u8], ElementHeader), ParseEBMLFailed> {
    while !input.is_empty() {
        let (remain, header) = next_element_header(input)?;
        if header.id == target_id {
            return Ok((remain, header));
        }
        if input.remaining() < header.data_size {
            return Err(ParseEBMLFailed::Need(header.data_size - input.remaining()));
        }
        input.advance(header.data_size);
    }
    Err(ParseEBMLFailed::Need(1))
}

#[derive(Clone)]
pub(crate) struct ElementHeader {
    pub id: u64,
    pub data_size: usize,
}

pub(crate) fn next_element_header(
    mut input: &[u8],
) -> Result<(&[u8], ElementHeader), ParseEBMLFailed> {
    if input.is_empty() {
        return Err(ParseEBMLFailed::Need(1));
    }

    let mut cursor = Cursor::new(input);
    let id = VInt::as_u64_with_marker(&mut cursor)?;
    input.advance(cursor.position() as usize);

    if input.is_empty() {
        return Err(ParseEBMLFailed::Need(1));
    }

    let mut cursor = Cursor::new(input);
    let data_size = VInt::as_usize(&mut cursor)?;
    input.advance(cursor.position() as usize);

    Ok((input, ElementHeader { id, data_size }))
}

fn as_cstr(buf: &[u8]) -> String {
    buf.iter()
        .take_while(|b| **b != 0)
        .map(|b| (*b) as char)
        .collect::<String>()
}

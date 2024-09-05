use std::io::Read;

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

    #[error("read EBML file failed: {0}")]
    IOError(std::io::Error),
}

#[derive(Debug, Clone)]
pub struct EBMLFileInfo {
    doc_type: String,
}

pub fn parse_ebml<T: Read>(mut reader: T) -> Result<EBMLFileInfo, ParseEBMLFailed> {
    const INIT_BUF_SIZE: usize = 4096;
    const MIN_GROW_SIZE: usize = 4096;
    const MAX_GROW_SIZE: usize = 1000 * 4096;

    let mut buf = Vec::with_capacity(INIT_BUF_SIZE);
    let n = reader
        .by_ref()
        .take(INIT_BUF_SIZE as u64)
        .read_to_end(buf.as_mut())
        .map_err(ParseEBMLFailed::IOError)?;
    if n == 0 {
        Err(ParseEBMLFailed::NotEBMLFile)?;
    }

    let doc_type = parse_ebml_doc_type(&buf)?;

    Ok(EBMLFileInfo { doc_type })
}

const ID_EBML: u32 = 0x1A45DFA3;

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
enum EBMLGlobalId {
    Crc32 = 0xBF,
    Void = 0xEC,
}

fn parse_ebml_doc_type(mut buf: &[u8]) -> Result<String, ParseEBMLFailed> {
    if buf.remaining() < 4 {
        return Err(ParseEBMLFailed::Need(4 - buf.len()));
    }
    let id = buf.get_u32();
    if id != ID_EBML {
        return Err(ParseEBMLFailed::NotEBMLFile);
    }

    // get doc type
    while buf.remaining() >= 2 {
        let id = buf.get_u16();
        if id == EBMLHeaderId::DocType as u16 {
            if buf.remaining() == 0 {
                return Err(ParseEBMLFailed::Need(1));
            }

            let (buf, size) = match VInt::parse_unsigned(buf.chunk()) {
                Ok(size) => size,
                Err(ParseVIntFailed::InvalidVInt(e)) => {
                    return Err(ParseEBMLFailed::InvalidEBMLFile(e.into()))
                }
                Err(ParseVIntFailed::Need(i)) => return Err(ParseEBMLFailed::Need(i)),
            };

            let size = size as usize;
            if buf.remaining() >= size {
                let s = as_cstr(&buf[..size]);
                return Ok(s);
            } else {
                return Err(ParseEBMLFailed::Need(size - buf.remaining()));
            }
        }
    }

    return Err(ParseEBMLFailed::NotEBMLFile);
}

fn as_cstr(buf: &[u8]) -> String {
    buf.iter()
        .take_while(|b| **b != 0)
        .map(|b| (*b) as char)
        .collect::<String>()
}

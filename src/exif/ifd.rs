use std::collections::HashMap;
use thiserror::Error;

use crate::EntryValue;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("Failed to parse IFD entry; size/offset is overflow")]
    Overflow,

    #[error("Failed to parse IFD entry; invalid data: {0}")]
    InvalidData(String),

    #[error("Failed to parse IFD entry; unsupported: {0}")]
    Unsupported(String),
}

impl From<Error> for crate::Error {
    fn from(value: Error) -> Self {
        Self::InvalidEntry(value.into())
    }
}

/// https://www.media.mit.edu/pia/Research/deepview/exif.html
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParsedImageFileDirectory {
    pub entries: HashMap<u16, ParsedIdfEntry>,
}

impl ParsedImageFileDirectory {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParsedIdfEntry {
    pub value: EntryValue,
}

impl ParsedImageFileDirectory {
    pub(crate) fn get(&self, tag: u16) -> Option<&EntryValue> {
        self.entries.get(&tag).map(|x| &x.value)
    }

    pub(crate) fn put(&mut self, code: u16, v: EntryValue) {
        self.entries.insert(code, ParsedIdfEntry { value: v });
    }
}

impl From<chrono::ParseError> for Error {
    fn from(value: chrono::ParseError) -> Self {
        Error::InvalidData(format!("invalid time format: {value}"))
    }
}

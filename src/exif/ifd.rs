use crate::EntryValue;
use std::collections::HashMap;

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

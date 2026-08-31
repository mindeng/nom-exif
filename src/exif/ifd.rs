use crate::{EntryValue, IfdKind};
use std::collections::HashMap;

/// <https://www.media.mit.edu/pia/Research/deepview/exif.html>
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParsedImageFileDirectory {
    /// Keyed by `(namespace, code)` rather than by code alone: sub-IFDs share
    /// their parent's index, so two directories under the same index can
    /// legitimately both define e.g. `0x000b`.
    entries: HashMap<(IfdKind, u16), ParsedIdfEntry>,
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
    pub(crate) fn get(&self, kind: IfdKind, tag: u16) -> Option<&EntryValue> {
        self.entries.get(&(kind, tag)).map(|x| &x.value)
    }

    /// First hit for `tag` scanning namespaces in [`Self::KINDS`] order. For
    /// contested codes the caller should name the namespace instead.
    pub(crate) fn get_any(&self, tag: u16) -> Option<&EntryValue> {
        Self::KINDS.iter().find_map(|&k| self.get(k, tag))
    }

    pub(crate) fn put(&mut self, kind: IfdKind, code: u16, v: EntryValue) {
        self.entries
            .insert((kind, code), ParsedIdfEntry { value: v });
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (IfdKind, u16, &EntryValue)> {
        self.entries
            .iter()
            .map(|(&(kind, code), e)| (kind, code, &e.value))
    }

    /// Namespace scan order for code-only lookups.
    pub(crate) const KINDS: [IfdKind; 4] =
        [IfdKind::Tiff, IfdKind::Exif, IfdKind::Gps, IfdKind::Interop];
}

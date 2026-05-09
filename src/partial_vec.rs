use crate::slice::SubsliceRange as _;

use std::borrow::Borrow;
use std::fmt::Debug;
use std::ops::Deref;
use std::ops::Range;

use bytes::Bytes;

/// Owning view into a shared byte buffer.
///
/// **Transitional type — being phased out in P4.5.** New code should
/// pass `bytes::Bytes` directly. This struct exists for the duration
/// of the refactor to bridge old call sites that still construct
/// `PartialVec` while consumers migrate to `Bytes`.
#[derive(Clone, PartialEq, Eq, Default)]
pub(crate) struct PartialVec {
    pub(crate) data: Bytes,
    pub(crate) range: Range<usize>,
}

impl Debug for PartialVec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PartialVec")
            .field("data len", &self.data.len())
            .field("range", &self.range)
            .finish()
    }
}

impl PartialVec {
    pub(crate) fn new(data: Bytes, range: Range<usize>) -> PartialVec {
        assert!(range.end <= data.len());
        PartialVec { data, range }
    }

    pub(crate) fn from_vec(vec: Vec<u8>) -> PartialVec {
        let range = 0..vec.len();
        PartialVec {
            data: Bytes::from(vec),
            range,
        }
    }

    pub(crate) fn from_vec_range(vec: Vec<u8>, range: Range<usize>) -> PartialVec {
        assert!(range.end <= vec.len());
        PartialVec {
            data: Bytes::from(vec),
            range,
        }
    }

    pub(crate) fn partial(&self, subslice: &[u8]) -> AssociatedInput {
        let range = self
            .data
            .subslice_in_range(subslice)
            .expect("subslice should be a sub slice of self");
        PartialVec {
            data: self.data.clone(),
            range,
        }
    }

    /// Convert this view into a standalone `Bytes` (the sliced view, not
    /// the full backing allocation). Used during the P4.5 migration to
    /// hand off byte ownership to `Bytes`-typed consumers.
    pub(crate) fn into_bytes(self) -> Bytes {
        self.data.slice(self.range)
    }
}

impl From<Vec<u8>> for PartialVec {
    fn from(value: Vec<u8>) -> Self {
        PartialVec::from_vec(value)
    }
}

impl From<(Vec<u8>, Range<usize>)> for PartialVec {
    fn from(value: (Vec<u8>, Range<usize>)) -> Self {
        let (data, range) = value;
        PartialVec::from_vec_range(data, range)
    }
}

impl Deref for PartialVec {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.data[self.range.clone()]
    }
}

impl AsRef<[u8]> for PartialVec {
    fn as_ref(&self) -> &[u8] {
        &self.data[self.range.clone()]
    }
}

impl Borrow<[u8]> for PartialVec {
    fn borrow(&self) -> &[u8] {
        &self.data[self.range.clone()]
    }
}

pub(crate) type AssociatedInput = PartialVec;

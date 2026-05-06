use crate::slice::SubsliceRange as _;

use std::borrow::Borrow;
use std::fmt::Debug;
use std::ops::Deref;
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq, Default)]
pub(crate) struct PartialVec {
    pub(crate) data: Arc<Vec<u8>>,
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
    pub(crate) fn new(vec: Arc<Vec<u8>>, range: Range<usize>) -> PartialVec {
        assert!(range.end <= vec.len());
        PartialVec { data: vec, range }
    }

    pub(crate) fn from_vec(vec: Vec<u8>) -> PartialVec {
        let range = 0..vec.len();
        Self::from_vec_range(vec, range)
    }

    // pub(crate) fn to_vec(&self) -> Vec<u8> {
    //     Vec::from(self.data.clone())
    // }

    pub(crate) fn from_arc_vec_slice(vec: Arc<Vec<u8>>, subslice: &[u8]) -> PartialVec {
        let range = vec
            .subslice_in_range(subslice)
            .expect("subslice should be a sub slice of self");
        Self::new(vec, range)
    }

    pub(crate) fn from_vec_range(vec: Vec<u8>, range: Range<usize>) -> PartialVec {
        assert!(range.end <= vec.len());
        Self::new(Arc::new(vec), range)
    }

    pub(crate) fn partial(&self, subslice: &[u8]) -> AssociatedInput {
        let range = self
            .data
            .subslice_in_range(subslice)
            .expect("subslice should be a sub slice of self");

        AssociatedInput::new(self.data.clone(), range)
    }
}

impl From<Vec<u8>> for PartialVec {
    fn from(value: Vec<u8>) -> Self {
        PartialVec::from_vec(value)
    }
}

impl From<(Arc<Vec<u8>>, &[u8])> for PartialVec {
    fn from(value: (Arc<Vec<u8>>, &[u8])) -> Self {
        Self::from_arc_vec_slice(value.0, value.1)
    }
}

impl From<(Arc<Vec<u8>>, Range<usize>)> for PartialVec {
    fn from(value: (Arc<Vec<u8>>, Range<usize>)) -> Self {
        Self::new(value.0, value.1)
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

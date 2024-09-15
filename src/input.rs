use crate::slice::SubsliceRange as _;

use std::borrow::Borrow;
use std::ops::Deref;
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(crate) struct Input {
    pub(crate) data: Arc<Vec<u8>>,
    pub(crate) range: Range<usize>,
}

impl Input {
    pub(crate) fn new(vec: Arc<Vec<u8>>, range: Range<usize>) -> Input {
        assert!(range.end <= vec.len());
        Input { data: vec, range }
    }

    pub(crate) fn from_vec(vec: Vec<u8>) -> Input {
        let range = 0..vec.len();
        Self::from_vec_range(vec, range)
    }

    // pub(crate) fn to_vec(&self) -> Vec<u8> {
    //     Vec::from(self.data.clone())
    // }

    pub(crate) fn from_arc_vec_slice(vec: Arc<Vec<u8>>, subslice: &[u8]) -> Input {
        let range = vec
            .subslice_range(subslice)
            .expect("subslice should be a sub slice of self");
        Self::new(vec, range)
    }

    pub(crate) fn from_vec_slice(vec: Vec<u8>, subslice: &[u8]) -> Input {
        Self::from_arc_vec_slice(Arc::new(vec), subslice)
    }

    pub(crate) fn from_vec_range(vec: Vec<u8>, range: Range<usize>) -> Input {
        assert!(range.end <= vec.len());
        Self::new(Arc::new(vec), range)
    }

    pub(crate) fn make_associated(&self, subslice: &[u8]) -> AssociatedInput {
        let range = self
            .data
            .subslice_range(subslice)
            .expect("subslice should be a sub slice of self");

        AssociatedInput::new(self.data.clone(), range)
    }
}

// impl<'a> From<&'a [u8]> for Input<'a> {
//     fn from(data: &'a [u8]) -> Self {
//         Input {
//             data: Cow::Borrowed(data),
//             range: Range {
//                 start: 0,
//                 end: data.len(),
//             },
//         }
//     }
// }

impl From<Vec<u8>> for Input {
    fn from(value: Vec<u8>) -> Self {
        Input::from_vec(value)
    }
}

impl From<(Vec<u8>, &[u8])> for Input {
    fn from(value: (Vec<u8>, &[u8])) -> Self {
        Input::from_vec_slice(value.0, value.1)
    }
}

impl From<(Arc<Vec<u8>>, &[u8])> for Input {
    fn from(value: (Arc<Vec<u8>>, &[u8])) -> Self {
        Self::from_arc_vec_slice(value.0, value.1)
    }
}

impl From<(Arc<Vec<u8>>, Range<usize>)> for Input {
    fn from(value: (Arc<Vec<u8>>, Range<usize>)) -> Self {
        Self::new(value.0, value.1)
    }
}

impl From<(Vec<u8>, Range<usize>)> for Input {
    fn from(value: (Vec<u8>, Range<usize>)) -> Self {
        let (data, range) = value;
        Input::from_vec_range(data, range)
    }
}

impl Deref for Input {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.data[self.range.clone()]
    }
}

impl AsRef<[u8]> for Input {
    fn as_ref(&self) -> &[u8] {
        &self.data[self.range.clone()]
    }
}

impl Borrow<[u8]> for Input {
    fn borrow(&self) -> &[u8] {
        &self.data[self.range.clone()]
    }
}

pub(crate) type AssociatedInput = Input;

// #[derive(Clone, Debug, PartialEq, Eq)]
// pub struct AssociatedInput {
//     data: Arc<Vec<u8>>,
//     range: Range<usize>,
//     // pub(crate) ptr: *const u8,
//     // pub(crate) len: usize,
// }

// // Since we only use `AssociatedInput` in Exif, it's safe to impl `Send` &
// // `Sync` here.
// unsafe impl Send for AssociatedInput {}
// unsafe impl Sync for AssociatedInput {}

// impl AssociatedInput {
//     pub(crate) fn make_associated(&self, subslice: &[u8]) -> AssociatedInput {
//         let _ = self
//             .subslice_range(subslice)
//             .expect("subslice should be a sub slice of self");

//         AssociatedInput::new(subslice)
//     }
// }

// impl Deref for AssociatedInput {
//     type Target = [u8];

//     fn deref(&self) -> &Self::Target {
//         &self.data[self.range.clone()]
//     }
// }

// impl AsRef<[u8]> for AssociatedInput {
//     fn as_ref(&self) -> &[u8] {
//         self
//     }
// }

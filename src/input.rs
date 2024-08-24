use crate::slice::SubsliceRange as _;

use std::borrow::Borrow;
use std::borrow::Cow;
use std::ops::Deref;
use std::ops::Range;
use std::slice;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input<'a> {
    pub(crate) data: Cow<'a, [u8]>,
    pub(crate) range: Range<usize>,
}

impl Input<'_> {
    pub fn from_vec(data: Vec<u8>, range: Range<usize>) -> Input<'static> {
        assert!(range.end <= data.len());
        Input {
            data: Cow::Owned(data),
            range,
        }
    }

    pub fn make_associated(&self, subslice: &[u8]) -> AssociatedInput {
        let _ = self
            .subslice_range(subslice)
            .expect("subslice should be a sub slice of self");

        AssociatedInput::new(subslice)
    }
}

impl<'a> From<&'a [u8]> for Input<'a> {
    fn from(data: &'a [u8]) -> Self {
        Input {
            data: Cow::Borrowed(data),
            range: Range {
                start: 0,
                end: data.len(),
            },
        }
    }
}

impl From<(Vec<u8>, Range<usize>)> for Input<'static> {
    fn from(value: (Vec<u8>, Range<usize>)) -> Self {
        let (data, range) = value;
        Input::from_vec(data, range)
    }
}

impl<'a> Deref for Input<'a> {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.data[self.range.clone()]
    }
}

impl<'a> AsRef<[u8]> for Input<'a> {
    fn as_ref(&self) -> &[u8] {
        &self.data[self.range.clone()]
    }
}

impl<'a> Borrow<[u8]> for Input<'a> {
    fn borrow(&self) -> &[u8] {
        &self.data[self.range.clone()]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssociatedInput {
    pub(crate) ptr: *const u8,
    pub(crate) len: usize,
}

impl AssociatedInput {
    pub fn new(input: &[u8]) -> Self {
        let data = input.as_ptr();
        Self {
            ptr: data,
            len: input.len(),
        }
    }
}

impl Deref for AssociatedInput {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl AsRef<[u8]> for AssociatedInput {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

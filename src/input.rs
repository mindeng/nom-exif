use std::borrow::Borrow;

use std::ops::Deref;

use std::ops::Range;

use std::borrow::Cow;

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

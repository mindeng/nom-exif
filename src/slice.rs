use std::ops::Range;

pub trait SubsliceOffset {
    fn subslice_offset(&self, inner: &Self) -> Option<usize>;
}

pub trait SubsliceRange {
    fn subslice_range(&self, inner: &Self) -> Option<Range<usize>>;
}

impl<T> SubsliceOffset for [T] {
    fn subslice_offset(&self, inner: &Self) -> Option<usize> {
        let start = self.as_ptr() as usize;
        let inner_start = inner.as_ptr() as usize;
        if inner_start < start || inner_start > start.wrapping_add(self.len()) {
            None
        } else {
            inner_start.checked_sub(start)
        }
    }
}

impl<T> SubsliceRange for [T]
where
    [T]: SubsliceOffset,
{
    fn subslice_range(&self, inner: &Self) -> Option<Range<usize>> {
        let offset = self.subslice_offset(inner)?;
        let end = offset.checked_add(inner.len())?;
        let start = self.as_ptr() as usize;
        if end > start + self.len() {
            None
        } else {
            Some(Range { start: offset, end })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SubsliceOffset;

    #[test]
    fn subslice_offset() {
        let a = &[0u8];
        let v: Vec<u8> = vec![0, 1, 2, 3, 4, 5];
        let b = &[0u8];

        assert_eq!(v.subslice_offset(&v).unwrap(), 0);
        assert_eq!(v.subslice_offset(&v[1..2]).unwrap(), 1);
        assert_eq!(v.subslice_offset(&v[1..]).unwrap(), 1);
        assert_eq!(v.subslice_offset(&v[2..]).unwrap(), 2);
        assert_eq!(v.subslice_offset(&v[3..]).unwrap(), 3);
        assert_eq!(v.subslice_offset(&v[5..]).unwrap(), 5);

        assert!(v.subslice_offset(a).is_none());
        assert!(v.subslice_offset(b).is_none());
    }
}

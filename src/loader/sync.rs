use std::{io::Read, marker::PhantomData};

use crate::skip::Skip;

use super::{BufLoad, Load, INIT_BUF_SIZE};

pub(crate) struct BufLoader<S, R> {
    inner: Inner<S, R>,
}

impl<S: Skip<R>, R: Read> Load for BufLoader<S, R> {
    #[inline]
    fn read_buf(&mut self, to_read: usize) -> std::io::Result<usize> {
        self.inner.read_buf(to_read)
    }

    #[inline]
    fn skip(&mut self, n: usize) -> std::io::Result<()> {
        if S::skip_by_seek(&mut self.inner.read, n as u64)? {
            Ok(())
        } else {
            self.inner.skip_by_read(n)
        }
    }
}

impl<S, R: Read> BufLoad for BufLoader<S, R> {
    #[inline]
    fn into_vec(self) -> Vec<u8> {
        self.inner.buf
    }

    #[inline]
    fn buf(&self) -> &Vec<u8> {
        &self.inner.buf
    }

    #[inline]
    fn buf_mut(&mut self) -> &mut Vec<u8> {
        &mut self.inner.buf
    }
}

impl<Idx, S, R> std::ops::Index<Idx> for BufLoader<S, R>
where
    Idx: std::slice::SliceIndex<[u8]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.inner.buf[index]
    }
}

impl<S, R> BufLoader<S, R> {
    pub fn new(reader: R) -> Self {
        Self {
            inner: Inner::<S, R>::new(reader),
        }
    }
}

pub(crate) struct Inner<S, R> {
    buf: Vec<u8>,
    read: R,
    phantom: PhantomData<S>,
}

impl<S, T> Inner<S, T> {
    pub fn new(reader: T) -> Self {
        Self {
            buf: Vec::with_capacity(INIT_BUF_SIZE),
            read: reader,
            phantom: PhantomData,
        }
    }
}

impl<S, T> Inner<S, T>
where
    T: Read,
{
    #[inline]
    fn read_buf(&mut self, to_read: usize) -> std::io::Result<usize> {
        self.buf.reserve(to_read);

        let n = self
            .read
            .by_ref()
            .take(to_read as u64)
            .read_to_end(self.buf.as_mut())?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        Ok(n)
    }

    #[inline]
    fn skip_by_read(&mut self, n: usize) -> std::io::Result<()> {
        self.buf.reserve(n);
        assert!(self.buf.capacity() - self.buf.len() >= n);
        let start = self.buf.len();
        self.read.read_exact(&mut self.buf[start..start + n])
    }
}

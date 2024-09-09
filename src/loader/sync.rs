use std::{io::Read, marker::PhantomData};

use crate::skip::Skip;

use super::{BufLoad, Load, INIT_BUF_SIZE};

/// Loads bytes from `R` using an internally maintained buffer.
///
/// Since Rust doesn't currently support
/// [specialization](https://rust-lang.github.io/rfcs/1210-impl-specialization.html)
/// , so the struct have to let user to tell it if the reader supports `Seek`,
/// in the following way:
///
/// - `let loader = BufLoader::<SkipRead, _>::new(reader);` means the `reader`
///   doesn't support `Seek`.
///   
/// - `let loader = BufLoader::<SkipSeek, _>::new(reader);` means the `reader`
///   supports `Seek`.
///
/// Performance impact:
///
/// - If the reader supports `Seek`, the parser will use `Seek` to achieve
///   efficient positioning operations in the byte stream.
///
/// - Otherwise, the parser will fallback to skip certain bytes through Read.
///   This may have a certain impact on performance when processing certain large
///   files. For example, *.mov files place metadata at the end of the file.
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
            // S::skip(&mut self.inner.read, n as u64)
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
    fn buf(&self) -> &[u8] {
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
        match (&mut self.read).take(n as u64).read_to_end(&mut self.buf) {
            Ok(x) => {
                if x == n {
                    self.buf.clear();
                    Ok(())
                } else {
                    Err(std::io::ErrorKind::UnexpectedEof.into())
                }
            }
            Err(e) => Err(e),
        }
    }
}

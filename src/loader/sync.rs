use std::io::{Read, Seek};

use super::{BufLoad, Load, INIT_BUF_SIZE};

pub(crate) struct BufLoader<T> {
    inner: Inner<T>,
}

impl<T: Read> Load for BufLoader<T> {
    #[inline]
    fn read_buf(&mut self, to_read: usize) -> std::io::Result<usize> {
        self.inner.read_buf(to_read)
    }

    #[inline]
    fn skip(&mut self, n: usize) -> std::io::Result<()> {
        println!("read to skip");
        match std::io::copy(
            &mut self.inner.read.by_ref().take(n as u64),
            &mut std::io::sink(),
        ) {
            Ok(x) => {
                if x == n as u64 {
                    Ok(())
                } else {
                    Err(std::io::ErrorKind::UnexpectedEof.into())
                }
            }
            Err(e) => Err(e),
        }
    }
}

impl<T: Read> BufLoad for BufLoader<T> {
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

impl<Idx, T> std::ops::Index<Idx> for BufLoader<T>
where
    Idx: std::slice::SliceIndex<[u8]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.inner.buf[index]
    }
}

impl<T> BufLoader<T> {
    pub fn new(reader: T) -> Self {
        Self {
            inner: Inner::new(reader),
        }
    }
}

pub(crate) struct SeekBufLoader<T> {
    inner: Inner<T>,
}

impl<T: Read + Seek> Load for SeekBufLoader<T> {
    #[inline]
    fn read_buf(&mut self, to_read: usize) -> std::io::Result<usize> {
        self.inner.read_buf(to_read)
    }

    #[inline]
    fn skip(&mut self, n: usize) -> std::io::Result<()> {
        println!("seek to skip");
        self.inner
            .read
            .seek(std::io::SeekFrom::Current(n as i64))
            .map(|_| ())
    }
}

impl<T: Read + Seek> BufLoad for SeekBufLoader<T> {
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

impl<Idx, T> std::ops::Index<Idx> for SeekBufLoader<T>
where
    Idx: std::slice::SliceIndex<[u8]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.inner.buf[index]
    }
}

impl<T> SeekBufLoader<T> {
    pub fn new(reader: T) -> Self {
        Self {
            inner: Inner::new(reader),
        }
    }
}

pub(crate) struct Inner<T> {
    buf: Vec<u8>,
    read: T,
}

impl<T> Inner<T> {
    pub fn new(reader: T) -> Self {
        Self {
            buf: Vec::with_capacity(INIT_BUF_SIZE),
            read: reader,
        }
    }
}

impl<T> Inner<T>
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
}

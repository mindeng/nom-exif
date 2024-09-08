use std::{
    cmp::{max, min},
    io::{Cursor, Read, Seek},
};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt};

use crate::error::{ParsedError, ParsingError};

pub(crate) trait BufLoader {
    fn buf(&self) -> &Vec<u8>;
    fn buf_mut(&mut self) -> &mut Vec<u8>;
    fn into_vec(self) -> Vec<u8>;
    fn cursor<Idx>(&self, idx: Idx) -> Cursor<&[u8]>
    where
        Idx: std::slice::SliceIndex<[u8], Output = [u8]>,
    {
        Cursor::new(&self.buf()[idx])
    }

    fn clear(&mut self) {
        self.buf_mut().clear();
    }
}

pub(crate) trait Loader: BufLoader {
    fn read_buf(&mut self, n: usize) -> std::io::Result<usize>;
    fn skip(&mut self, n: usize) -> std::io::Result<()>;

    fn load_and_parse<P, O>(&mut self, mut parse: P) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8]) -> Result<O, ParsingError>,
    {
        self.load_and_parse_at(|x, _| parse(x), 0)
    }

    #[tracing::instrument(skip_all)]
    fn load_and_parse_at<P, O>(&mut self, mut parse: P, at: usize) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8], usize) -> Result<O, ParsingError>,
    {
        if at >= self.buf().len() {
            self.read_buf(INIT_BUF_SIZE)?;
        }

        loop {
            match parse(self.buf().as_ref(), at) {
                Ok(o) => return Ok(o),
                Err(ParsingError::ClearAndSkip(n)) => {
                    tracing::debug!(n, "clear and skip bytes");
                    self.clear();
                    self.skip(n)?;
                    self.read_buf(INIT_BUF_SIZE)?;
                }
                Err(ParsingError::Need(i)) => {
                    tracing::debug!(need = i, "need more bytes");
                    let to_read = max(i, MIN_GROW_SIZE);
                    let to_read = min(to_read, MAX_GROW_SIZE);

                    let n = self.read_buf(to_read)?;
                    if n == 0 {
                        return Err(ParsedError::NoEnoughBytes);
                    }
                }
                Err(ParsingError::Failed(s)) => return Err(ParsedError::Failed(s)),
            }
        }
    }
}

pub(crate) struct SeekReadLoader<T> {
    inner: Inner<T>,
}

impl<T: Read + Seek> Loader for SeekReadLoader<T> {
    #[inline]
    fn read_buf(&mut self, n: usize) -> std::io::Result<usize> {
        self.buf_mut().reserve(n);
        self.inner.read_buf(n)
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

impl<T: Read + Seek> BufLoader for SeekReadLoader<T> {
    #[inline]
    fn into_vec(self) -> Vec<u8> {
        self.into_vec()
    }

    #[inline]
    fn buf(&self) -> &Vec<u8> {
        self.buf()
    }

    #[inline]
    fn buf_mut(&mut self) -> &mut Vec<u8> {
        self.buf_mut()
    }
}

impl<Idx, T> std::ops::Index<Idx> for SeekReadLoader<T>
where
    Idx: std::slice::SliceIndex<[u8]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.buf()[index]
    }
}

impl<T> SeekReadLoader<T> {
    pub fn new(reader: T) -> Self {
        Self {
            inner: Inner::new(reader),
        }
    }

    #[inline]
    fn buf(&self) -> &Vec<u8> {
        &self.inner.buf
    }

    #[inline]
    fn buf_mut(&mut self) -> &mut Vec<u8> {
        &mut self.inner.buf
    }

    #[inline]
    fn into_vec(self) -> Vec<u8> {
        self.inner.buf
    }
}

pub(crate) struct ReadLoader<T> {
    inner: Inner<T>,
}

impl<T: Read> Loader for ReadLoader<T> {
    #[inline]
    fn read_buf(&mut self, n: usize) -> std::io::Result<usize> {
        self.buf_mut().reserve(n);
        self.inner.read_buf(n)
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

impl<T: Read> BufLoader for ReadLoader<T> {
    #[inline]
    fn into_vec(self) -> Vec<u8> {
        self.into_vec()
    }

    #[inline]
    fn buf(&self) -> &Vec<u8> {
        self.buf()
    }

    #[inline]
    fn buf_mut(&mut self) -> &mut Vec<u8> {
        self.buf_mut()
    }
}

impl<Idx, T> std::ops::Index<Idx> for ReadLoader<T>
where
    Idx: std::slice::SliceIndex<[u8]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.buf()[index]
    }
}

impl<T> ReadLoader<T> {
    pub fn new(reader: T) -> Self {
        Self {
            inner: Inner::new(reader),
        }
    }

    #[inline]
    fn buf(&self) -> &Vec<u8> {
        &self.inner.buf
    }

    #[inline]
    fn buf_mut(&mut self) -> &mut Vec<u8> {
        &mut self.inner.buf
    }

    #[inline]
    fn into_vec(self) -> Vec<u8> {
        self.inner.buf
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

const INIT_BUF_SIZE: usize = 4096;
const MIN_GROW_SIZE: usize = 2 * 4096;
const MAX_GROW_SIZE: usize = 10 * 4096;

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

pub(crate) struct InnerAsync<T> {
    buf: Vec<u8>,
    read: T,
}

impl<T> InnerAsync<T> {
    pub fn new(reader: T) -> Self {
        Self {
            buf: Vec::with_capacity(INIT_BUF_SIZE),
            read: reader,
        }
    }
}

impl<T> InnerAsync<T> where T: AsyncRead + Unpin {}

use super::{AsyncLoad, BufLoad, INIT_BUF_SIZE};
use crate::skip::AsyncSkip;

use std::marker::PhantomData;
use tokio::io::{AsyncRead, AsyncReadExt};

pub(crate) struct AsyncBufLoader<S, R> {
    inner: Inner<S, R>,
}

impl<S: AsyncSkip<T>, T: AsyncRead + Unpin> AsyncLoad for AsyncBufLoader<S, T> {
    #[inline]
    async fn read_buf(&mut self, n: usize) -> std::io::Result<usize> {
        self.inner.buf.reserve(n);
        self.inner.read_buf(n).await
    }

    async fn skip(&mut self, n: usize) -> std::io::Result<()> {
        if S::skip_by_seek(&mut self.inner.read, n as u64).await? {
            Ok(())
        } else {
            self.inner.skip_by_read(n).await
        }
    }
}

impl<S, T> BufLoad for AsyncBufLoader<S, T> {
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

impl<Idx, S, T> std::ops::Index<Idx> for AsyncBufLoader<S, T>
where
    Idx: std::slice::SliceIndex<[u8]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.inner.buf[index]
    }
}

impl<S, T> AsyncBufLoader<S, T> {
    pub fn new(reader: T) -> Self {
        Self {
            inner: Inner::new(reader),
        }
    }
}

struct Inner<S, T> {
    buf: Vec<u8>,
    read: T,
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
    T: AsyncRead + Unpin,
{
    #[inline]
    async fn read_buf(&mut self, to_read: usize) -> std::io::Result<usize> {
        self.buf.reserve(to_read);

        let n = self.read.read_buf(&mut self.buf).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        Ok(n)
    }

    #[inline]
    async fn skip_by_read(&mut self, n: usize) -> std::io::Result<()> {
        self.buf.resize(n, 0);
        match self.read.read_exact(&mut self.buf[..n]).await {
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

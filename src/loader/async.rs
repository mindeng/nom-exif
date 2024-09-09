#[cfg(feature = "async")]
use tokio::io::{AsyncRead, AsyncReadExt};

use super::{AsyncLoad, BufLoad, INIT_BUF_SIZE};

pub(crate) struct AsyncBufLoader<T> {
    inner: Inner<T>,
}

#[cfg(feature = "async")]
impl<T: AsyncRead + Unpin> AsyncLoad for AsyncBufLoader<T> {
    #[inline]
    async fn read_buf(&mut self, n: usize) -> std::io::Result<usize> {
        self.inner.buf.reserve(n);
        self.inner.read_buf(n).await
    }

    async fn skip(&mut self, n: usize) -> std::io::Result<()> {
        self.inner.skip_by_read(n).await
    }
}

impl<T> BufLoad for AsyncBufLoader<T> {
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

impl<Idx, T> std::ops::Index<Idx> for AsyncBufLoader<T>
where
    Idx: std::slice::SliceIndex<[u8]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.inner.buf[index]
    }
}

impl<T> AsyncBufLoader<T> {
    pub fn new(reader: T) -> Self {
        Self {
            inner: Inner::new(reader),
        }
    }
}

struct Inner<T> {
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

#[cfg(feature = "async")]
impl<T> Inner<T>
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
        self.buf.reserve(n);
        let start = self.buf.len();
        match self.read.read_exact(&mut self.buf[start..start + n]).await {
            Ok(x) => {
                if x == n {
                    Ok(())
                } else {
                    Err(std::io::ErrorKind::UnexpectedEof.into())
                }
            }
            Err(e) => Err(e),
        }
    }
}

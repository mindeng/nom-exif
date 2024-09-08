use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt};

use super::{AsyncLoad, BufLoad, INIT_BUF_SIZE};

pub(crate) struct AsyncBufLoader<T> {
    inner: Inner<T>,
}

impl<T: AsyncRead + Unpin> AsyncLoad for AsyncBufLoader<T> {
    #[inline]
    async fn read_buf(&mut self, n: usize) -> std::io::Result<usize> {
        self.inner.buf.reserve(n);
        self.inner.read_buf(n).await
    }

    async fn skip(&mut self, n: usize) -> std::io::Result<()> {
        self.inner.skip(n).await
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

pub(crate) struct AsyncSeekBufLoader<T> {
    inner: Inner<T>,
}

impl<T: AsyncRead + AsyncSeek + Unpin> AsyncLoad for AsyncSeekBufLoader<T> {
    #[inline]
    async fn read_buf(&mut self, n: usize) -> std::io::Result<usize> {
        self.inner.read_buf(n).await
    }

    #[inline]
    async fn skip(&mut self, n: usize) -> std::io::Result<()> {
        println!("seek to skip");
        self.inner
            .read
            .seek(std::io::SeekFrom::Current(n as i64))
            .await
            .map(|_| ())
    }
}

impl<T> BufLoad for AsyncSeekBufLoader<T> {
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

impl<Idx, T> std::ops::Index<Idx> for AsyncSeekBufLoader<T>
where
    Idx: std::slice::SliceIndex<[u8]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.buf()[index]
    }
}

impl<T> AsyncSeekBufLoader<T> {
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
    async fn skip(&mut self, n: usize) -> std::io::Result<()> {
        println!("read to skip");
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

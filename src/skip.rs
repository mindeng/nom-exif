use std::io::{self, BufRead, Read, Seek};

#[cfg(feature = "async")]
use tokio::io::{AsyncRead, AsyncSeek, AsyncSeekExt};

/// Seekable represents a *seek-able* `Read`, e.g. a `File`.
///
/// Use `Seekable` as a generic parameter to tell the parser to use `Seek` to
/// implement [`Skip`] operations. For more information, please refer to:
/// [`parse_track_info`](crate::parse_track_info).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Copy)]
pub struct Seekable(());

/// Use `SkipRead` as a generic parameter for some interfaces, so tell the
/// parser to use `Read` to implement [`Skip`] operations. For more
/// information, please refer to:
/// [`parse_track_info`](crate::parse_track_info).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Copy)]
pub struct Unseekable(());

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Copy)]
pub struct SkipBufRead(());

/// Abstracts the operation of skipping some bytes.
///
/// The user specifies the parser's `Skip` behavior using [`SkipSeek`] or
/// [`SkipRead`].
///
/// For more information, please refer to:
/// [`parse_track_info`](crate::parse_track_info).
#[allow(unused)]
pub trait Skip<R> {
    /// Skip the given number of bytes.
    fn skip(reader: &mut R, skip: u64) -> io::Result<()>;

    /// Skip the given number of bytes. If seek is not implemented by `reader`,
    /// `false` will be returned.
    ///
    /// Therefore, the caller can implement the skip function by himself,
    /// thereby reusing the caller's own buffer.
    fn skip_by_seek(reader: &mut R, skip: u64) -> io::Result<bool>;
}

#[cfg(feature = "async")]
pub(crate) trait AsyncSkip<R> {
    /// Skip the given number of bytes. If seek is not implemented by `reader`,
    /// `false` will be returned.
    ///
    /// Therefore, the caller can implement the skip function by himself,
    /// thereby reusing the caller's own buffer.
    async fn skip_by_seek(reader: &mut R, skip: u64) -> io::Result<bool>;
}

impl<R: Read> Skip<R> for Unseekable {
    #[inline]
    fn skip(reader: &mut R, skip: u64) -> io::Result<()> {
        // println!("unseekable...");
        match std::io::copy(&mut reader.by_ref().take(skip), &mut std::io::sink()) {
            Ok(x) => {
                if x == skip {
                    Ok(())
                } else {
                    Err(std::io::ErrorKind::UnexpectedEof.into())
                }
            }
            Err(e) => Err(e),
        }
    }

    #[inline]
    fn skip_by_seek(_: &mut R, _: u64) -> io::Result<bool> {
        Ok(false)
    }
}

impl<R: Seek> Skip<R> for Seekable {
    #[inline]
    fn skip(reader: &mut R, skip: u64) -> io::Result<()> {
        // println!("seekable...");
        reader.seek_relative(skip.try_into().unwrap())
    }

    #[inline]
    fn skip_by_seek(reader: &mut R, skip: u64) -> io::Result<bool> {
        reader.seek_relative(skip.try_into().unwrap())?;
        Ok(true)
    }
}

#[cfg(feature = "async")]
impl<R: AsyncRead> AsyncSkip<R> for Unseekable {
    #[inline]
    async fn skip_by_seek(_: &mut R, _: u64) -> io::Result<bool> {
        Ok(false)
    }
}

#[cfg(feature = "async")]
impl<R: AsyncSeek + Unpin> AsyncSkip<R> for Seekable {
    #[inline]
    async fn skip_by_seek(reader: &mut R, skip: u64) -> io::Result<bool> {
        match reader.seek(std::io::SeekFrom::Current(skip as i64)).await {
            Ok(n) => {
                if n == skip {
                    Ok(true)
                } else {
                    Err(std::io::ErrorKind::UnexpectedEof.into())
                }
            }
            Err(e) => Err(e),
        }
    }
}

impl<R: BufRead> Skip<R> for SkipBufRead {
    fn skip(reader: &mut R, mut skip: u64) -> io::Result<()> {
        while skip > 0 {
            let buffer = reader.fill_buf()?;
            if buffer.is_empty() {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let consume = u64::try_from(buffer.len()).expect("should fit").min(skip);
            reader.consume(usize::try_from(consume).expect("must fit"));
            skip -= consume;
        }
        Ok(())
    }

    fn skip_by_seek(_: &mut R, _: u64) -> io::Result<bool> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use io::{repeat, Cursor};

    fn parse<S: Skip<R>, R: Read>(reader: &mut R) -> io::Result<bool> {
        S::skip_by_seek(reader, 2)
    }

    #[cfg(feature = "async")]
    async fn parse_async<S: AsyncSkip<R>, R: AsyncRead + Unpin>(
        reader: &mut R,
    ) -> io::Result<bool> {
        S::skip_by_seek(reader, 2).await
    }

    #[test]
    fn skip() {
        let mut buf = Cursor::new([0u8, 3]);
        assert!(!parse::<Unseekable, _>(&mut buf).unwrap());
        assert!(parse::<Seekable, _>(&mut buf).unwrap());

        let mut r = repeat(0);
        assert!(!parse::<Unseekable, _>(&mut r).unwrap());
    }

    #[cfg(feature = "async")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn skip_async() {
        let mut buf = Cursor::new([0u8, 3]);
        assert!(!parse_async::<Unseekable, _>(&mut buf).await.unwrap());
        assert!(parse_async::<Seekable, _>(&mut buf).await.unwrap());

        let mut r = tokio::io::repeat(1);
        assert!(!parse_async::<Unseekable, _>(&mut r).await.unwrap());
    }
}

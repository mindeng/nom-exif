#[cfg(feature = "tokio")]
use std::{fmt::Debug, io};

#[cfg(feature = "tokio")]
use tokio::io::{AsyncRead, AsyncSeek, AsyncSeekExt};

/// Seekable represents a *seek-able* reader, e.g. a `File`.
///
/// Used as the `S` phantom parameter on `AsyncMediaSource` to select the
/// `AsyncSkip<R>` impl. The sync side now stores a fn pointer instead.
/// Both ZSTs are deleted in Task 7 once async drops `S` too.
#[cfg(feature = "tokio")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Copy)]
pub struct Seekable(());

/// Counterpart to [`Seekable`] for readers that only impl `AsyncRead`
/// (no seek).
#[cfg(feature = "tokio")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Copy)]
pub struct Unseekable(());

#[cfg(feature = "tokio")]
pub trait AsyncSkip<R> {
    /// Skip the given number of bytes. If seek is not implemented by `reader`,
    /// `false` will be returned.
    ///
    /// Therefore, the caller can implement the skip function by himself,
    /// thereby reusing the caller's own buffer.
    fn skip_by_seek(
        reader: &mut R,
        skip: u64,
    ) -> impl std::future::Future<Output = io::Result<bool>> + Send;

    fn debug() -> impl Debug;
}

#[cfg(feature = "tokio")]
impl<R: AsyncRead + Unpin + Send> AsyncSkip<R> for Unseekable {
    #[inline]
    async fn skip_by_seek(_: &mut R, _: u64) -> io::Result<bool> {
        Ok(false)
    }

    fn debug() -> impl Debug {
        "async unseekable"
    }
}

#[cfg(feature = "tokio")]
impl<R: AsyncSeek + Unpin + Send> AsyncSkip<R> for Seekable {
    #[inline]
    async fn skip_by_seek(reader: &mut R, skip: u64) -> io::Result<bool> {
        match reader.seek(std::io::SeekFrom::Current(skip as i64)).await {
            Ok(_) => Ok(true),
            Err(e) => Err(e),
        }
    }

    fn debug() -> impl Debug {
        "async seekable"
    }
}

#[cfg(all(test, feature = "tokio"))]
mod tests {
    use super::*;
    use std::io::Cursor;

    async fn parse_async<S: AsyncSkip<R>, R: AsyncRead + Unpin>(
        reader: &mut R,
    ) -> io::Result<bool> {
        S::skip_by_seek(reader, 2).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn skip_async() {
        let mut buf = Cursor::new([0u8, 3]);
        assert!(!parse_async::<Unseekable, _>(&mut buf).await.unwrap());
        assert!(parse_async::<Seekable, _>(&mut buf).await.unwrap());

        let mut r = tokio::io::repeat(1);
        assert!(!parse_async::<Unseekable, _>(&mut r).await.unwrap());
    }
}

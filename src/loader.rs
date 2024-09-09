use std::{
    cmp::{max, min},
    io::Cursor,
};

use crate::error::{ParsedError, ParsingError};

mod sync;
pub(crate) use sync::BufLoader;

#[cfg(feature = "async")]
mod r#async;
#[cfg(feature = "async")]
pub(crate) use r#async::AsyncBufLoader;

const INIT_BUF_SIZE: usize = 4096;
const MIN_GROW_SIZE: usize = 2 * 4096;
const MAX_GROW_SIZE: usize = 10 * 4096;

pub(crate) trait BufLoad {
    fn buf(&self) -> &[u8];
    fn buf_mut(&mut self) -> &mut Vec<u8>;
    fn into_vec(self) -> Vec<u8>;
    #[allow(unused)]
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

pub(crate) trait Load: BufLoad {
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
            match parse(self.buf(), at) {
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
                    tracing::debug!(actual_read = n, "has been read");
                }
                Err(ParsingError::Failed(s)) => return Err(ParsedError::Failed(s)),
            }
        }
    }
}

#[cfg(feature = "async")]
pub(crate) trait AsyncLoad: BufLoad {
    async fn read_buf(&mut self, n: usize) -> std::io::Result<usize>;
    async fn skip(&mut self, n: usize) -> std::io::Result<()>;

    async fn load_and_parse<P, O>(&mut self, mut parse: P) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8]) -> Result<O, ParsingError>,
    {
        self.load_and_parse_at(|x, _| parse(x), 0).await
    }

    #[tracing::instrument(skip_all)]
    async fn load_and_parse_at<P, O>(&mut self, mut parse: P, at: usize) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8], usize) -> Result<O, ParsingError>,
    {
        if at >= self.buf().len() {
            self.read_buf(INIT_BUF_SIZE).await?;
        }

        loop {
            match parse(self.buf(), at) {
                Ok(o) => return Ok(o),
                Err(ParsingError::ClearAndSkip(n)) => {
                    tracing::debug!(n, "clear and skip bytes");
                    self.clear();
                    self.skip(n).await?;
                    self.read_buf(INIT_BUF_SIZE).await?;
                }
                Err(ParsingError::Need(i)) => {
                    tracing::debug!(need = i, "need more bytes");
                    let to_read = max(i, MIN_GROW_SIZE);
                    let to_read = min(to_read, MAX_GROW_SIZE);

                    let n = self.read_buf(to_read).await?;
                    if n == 0 {
                        return Err(ParsedError::NoEnoughBytes);
                    }
                }
                Err(ParsingError::Failed(s)) => return Err(ParsedError::Failed(s)),
            }
        }
    }
}

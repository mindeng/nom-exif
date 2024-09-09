use std::io::{self, Read, Seek};

pub(crate) trait Skip<R> {
    /// Skip the given number of bytes.
    fn skip(reader: &mut R, skip: u64) -> io::Result<()>;
}

pub struct SkipRead;
pub struct SkipSeek;

impl<R: Read> Skip<R> for SkipRead {
    #[inline]
    fn skip(reader: &mut R, skip: u64) -> io::Result<()> {
        tracing::debug!("read to skip");
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
}

impl<R: Seek> Skip<R> for SkipSeek {
    #[inline]
    fn skip(reader: &mut R, skip: u64) -> io::Result<()> {
        tracing::debug!("seek to skip");
        reader.seek_relative(skip.try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use io::{repeat, Cursor};
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Registry};

    use super::*;

    #[test]
    fn skip() {
        let stdout_log = tracing_subscriber::fmt::layer().pretty();
        let subscriber = Registry::default().with(stdout_log);
        subscriber.init();

        pub fn parse<R: Read, S: Skip<R>>(reader: &mut R) -> io::Result<()> {
            S::skip(reader, 2)
        }

        let mut buf = Cursor::new([0u8, 3]);
        parse::<_, SkipRead>(&mut buf).unwrap();
        parse::<_, SkipSeek>(&mut buf).unwrap();

        let mut r = repeat(0);
        parse::<_, SkipRead>(&mut r).unwrap();
    }
}

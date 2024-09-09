use std::{
    cmp::{max, min},
    fs::File,
    io::{self, Read, Seek},
    marker::PhantomData,
    net::TcpStream,
    path::Path,
};

use crate::{
    error::{ParsedError, ParsingError},
    file::Mime,
    mov::extract_moov_body_from_buf,
    skip::Skip,
    Exif, ExifIter, MediaType, Seekable, SkipRead, TrackInfo,
};

/// MediaSource represents a media data source.
pub struct MediaSource<R, S = Seekable> {
    read: R,
    phantom: PhantomData<S>,
}

impl<R: Read, S> MediaSource<R, S> {}

impl<R: Read + Seek> MediaSource<R> {
    pub fn seekable(read: R) -> Self {
        Self {
            read,
            phantom: PhantomData,
        }
    }
}

impl<R: Read> MediaSource<R, SkipRead> {
    pub fn unseekable(read: R) -> Self {
        Self {
            read,
            phantom: PhantomData,
        }
    }
}

impl<R: Read, S> MediaSource<R, S> {
    fn from_read(read: R) -> Self {
        Self {
            read,
            phantom: PhantomData,
        }
    }
}

impl MediaSource<File, Seekable> {
    pub fn file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Ok(Self::from_read(File::open(path)?))
    }
}

impl MediaSource<TcpStream, SkipRead> {
    pub fn tcp_stream(stream: TcpStream) -> Self {
        Self::from_read(stream)
    }
}

const INIT_BUF_SIZE: usize = 4096;
const MIN_GROW_SIZE: usize = 2 * 4096;
const MAX_GROW_SIZE: usize = 10 * 4096;

trait BufLoad {
    fn load_and_parse<R: Read, S: Skip<R>, P, O>(
        &mut self,
        media: &mut MediaSource<R, S>,
        mut parse: P,
    ) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8]) -> Result<O, ParsingError>,
    {
        self.load_and_parse_with_offset(media, |data, _| parse(data), 0)
    }

    #[tracing::instrument(skip_all)]
    fn load_and_parse_with_offset<R: Read, S: Skip<R>, P, O>(
        &mut self,
        media: &mut MediaSource<R, S>,
        mut parse: P,
        offset: usize,
    ) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8], usize) -> Result<O, ParsingError>,
    {
        let read = &mut media.read;
        if offset >= self.buffer().len() {
            self.fill_buf(read, INIT_BUF_SIZE)?;
        }

        loop {
            match parse(self.buffer(), offset) {
                Ok(o) => return Ok(o),
                Err(ParsingError::ClearAndSkip(n)) => {
                    tracing::debug!(n, "clear and skip bytes");
                    self.clear();
                    // self.skip(read, n)?;
                    S::skip(read, n.try_into().unwrap())?;
                    self.fill_buf(read, INIT_BUF_SIZE)?;
                }
                Err(ParsingError::Need(i)) => {
                    tracing::debug!(need = i, "need more bytes");
                    let to_read = max(i, MIN_GROW_SIZE);
                    let to_read = min(to_read, MAX_GROW_SIZE);

                    let n = self.fill_buf(read, to_read)?;
                    if n == 0 {
                        return Err(ParsedError::NoEnoughBytes);
                    }
                    tracing::debug!(actual_read = n, "has been read");
                }
                Err(ParsingError::Failed(s)) => return Err(ParsedError::Failed(s)),
            }
        }
    }

    fn buffer(&self) -> &[u8];
    fn fill_buf<R: Read>(&mut self, read: &mut R, size: usize) -> io::Result<usize>;
    fn clear(&mut self);
}

impl BufLoad for MediaParser {
    fn buffer(&self) -> &[u8] {
        &self.buf
    }

    fn fill_buf<R: Read>(&mut self, read: &mut R, size: usize) -> io::Result<usize> {
        self.buf.reserve(size);

        let n = read.take(size as u64).read_to_end(self.buf.as_mut())?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }

        Ok(n)
    }

    fn clear(&mut self) {
        self.buf.clear();
    }
}

impl MediaParser {
    fn detect<R: Read, S>(&mut self, mut src: MediaSource<R, S>) -> crate::Result<Mime> {
        self.clear();
        self.fill_buf(src.read.by_ref(), INIT_BUF_SIZE)?;
        self.buffer().try_into()
    }

    fn parse<R, S, O: ParseOutput<R, S>>(&mut self, mut mf: MediaSource<R, S>) -> crate::Result<O> {
        // let mf: MediaSource<SkipRead, R> = MediaSource::<S, R>::from_read(src);
        // FromMediaFile::from_media_file(mf)
        ParseOutput::parse(self, &mut mf)
    }
}

trait ParseOutput<R, S>: Sized {
    fn parse(parser: &mut MediaParser, mf: &mut MediaSource<R, S>) -> crate::Result<Self>;
}
impl<R: Read, S: Skip<R>> ParseOutput<R, S> for TrackInfo {
    fn parse(parser: &mut MediaParser, mf: &mut MediaSource<R, S>) -> crate::Result<Self> {
        Ok(parser.load_and_parse(mf, |data| {
            extract_moov_body_from_buf(data)?;
            Ok(TrackInfo::default())
        })?)
    }
}

// 通过 Parser 复用内存
pub(crate) struct MediaParser {
    buf: Vec<u8>,
}

impl MediaParser {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(INIT_BUF_SIZE),
        }
    }
}

// impl<L: Load> MediaFile<L> {
//     fn from_loader(loader: L) -> Self {
//         Self { loader }
//     }
// }

// impl<L: Load> From<File> for MediaFile<L> {
//     fn from(file: File) -> Self {
//         let loader = BufLoader::<SkipSeek, _>::new(file);
//         Self { loader }
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    use io::repeat;
    use test_case::case;

    #[case("meta.mov")]
    fn detect(path: &str) {
        let mf = MediaSource::file(path).unwrap();
        // let mf = MediaSource::unseekable(repeat(0));
        let mut parser = MediaParser::new();
        let ti: TrackInfo = parser.parse(mf).unwrap();
    }

    #[case("meta.mov")]
    fn detect_and_parse(path: &str) {
        let mf = MediaSource::file(path).unwrap();
        // let mf = MediaSource::<SkipRead, _>::from_read(f);
        let mut parser = MediaParser::new();
        let ti: TrackInfo = parser.parse(mf).unwrap();
    }
}

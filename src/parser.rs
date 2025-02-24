use std::{
    cmp::{max, min},
    fmt::{Debug, Display},
    fs::File,
    io::{self, Read, Seek},
    marker::PhantomData,
    net::TcpStream,
    ops::Range,
    path::Path,
};

use crate::{
    buffer::Buffers,
    error::{ParsedError, ParsingError, ParsingErrorState},
    exif::{parse_exif_iter, TiffHeader},
    file::Mime,
    partial_vec::PartialVec,
    skip::Skip,
    video::parse_track_info,
    ExifIter, Seekable, TrackInfo, Unseekable,
};

/// `MediaSource` represents a media data source that can be parsed by
/// [`MediaParser`].
///
/// - Use `MediaSource::file_path(path)` or `MediaSource::file(file)` to create
///   a MediaSource from a file
///
/// - Use `MediaSource::tcp_stream(stream)` to create a MediaSource from a `TcpStream`
/// - In other cases:
///
///   - Use `MediaSource::seekable(reader)` to create a MediaSource from a `Read + Seek`
///   
///   - Use `MediaSource::unseekable(reader)` to create a MediaSource from a
///     reader that only impl `Read`
///   
/// `seekable` is preferred to `unseekable`, since the former is more efficient
/// when the parser needs to skip a large number of bytes.
///
/// Passing in a `BufRead` should be avoided because [`MediaParser`] comes with
/// its own buffer management and the buffers can be shared between multiple
/// parsing tasks, thus avoiding frequent memory allocations.
pub struct MediaSource<R, S = Seekable> {
    pub(crate) reader: R,
    pub(crate) buf: Vec<u8>,
    pub(crate) mime: Mime,
    phantom: PhantomData<S>,
}

impl<R, S: Skip<R>> Debug for MediaSource<R, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaSource")
            // .field("reader", &self.reader)
            .field("mime", &self.mime)
            .field("seekable", &S::debug())
            .finish_non_exhaustive()
    }
}

// Should be enough for parsing header
const HEADER_PARSE_BUF_SIZE: usize = 128;

impl<R: Read, S: Skip<R>> MediaSource<R, S> {
    #[tracing::instrument(skip(reader))]
    fn build(mut reader: R) -> crate::Result<Self> {
        // TODO: reuse MediaParser to parse header
        let mut buf = Vec::with_capacity(HEADER_PARSE_BUF_SIZE);
        reader
            .by_ref()
            .take(HEADER_PARSE_BUF_SIZE as u64)
            .read_to_end(&mut buf)?;
        let mime: Mime = buf.as_slice().try_into()?;
        tracing::debug!(?mime);
        Ok(Self {
            reader,
            buf,
            mime,
            phantom: PhantomData,
        })
    }

    pub fn has_track(&self) -> bool {
        match self.mime {
            Mime::Image(_) => false,
            Mime::Video(_) => true,
        }
    }

    pub fn has_exif(&self) -> bool {
        match self.mime {
            Mime::Image(_) => true,
            Mime::Video(_) => false,
        }
    }
}

impl<R: Read + Seek> MediaSource<R, Seekable> {
    pub fn seekable(reader: R) -> crate::Result<Self> {
        Self::build(reader)
    }
}

impl<R: Read> MediaSource<R, Unseekable> {
    pub fn unseekable(reader: R) -> crate::Result<Self> {
        Self::build(reader)
    }
}

impl MediaSource<File, Seekable> {
    pub fn file_path<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::seekable(File::open(path)?)
    }

    pub fn file(file: File) -> crate::Result<Self> {
        Self::seekable(file)
    }
}

impl MediaSource<TcpStream, Unseekable> {
    pub fn tcp_stream(stream: TcpStream) -> crate::Result<Self> {
        Self::unseekable(stream)
    }
}

// Keep align with 4K
pub(crate) const INIT_BUF_SIZE: usize = 4096;
pub(crate) const MIN_GROW_SIZE: usize = 4096;
// Max size of APP1 is 0xFFFF
pub(crate) const MAX_GROW_SIZE: usize = 63 * 1024;
// Set a reasonable upper limit for single buffer allocation.
pub(crate) const MAX_ALLOC_SIZE: usize = 100 * 1024 * 1024;

pub(crate) trait Buf {
    fn buffer(&self) -> &[u8];
    fn clear(&mut self);

    fn set_position(&mut self, pos: usize);
    #[allow(unused)]
    fn position(&self) -> usize;
}

#[derive(Debug, Clone)]
pub(crate) enum ParsingState {
    TiffHeader(TiffHeader),
    HeifExifSize(usize),
}

impl Display for ParsingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParsingState::TiffHeader(h) => Display::fmt(&format!("ParsingState: {h:?})"), f),
            ParsingState::HeifExifSize(n) => Display::fmt(&format!("ParsingState: {n}"), f),
        }
    }
}

pub(crate) trait BufParser: Buf + Debug {
    fn fill_buf<R: Read>(&mut self, reader: &mut R, size: usize) -> io::Result<usize>;
    fn load_and_parse<R: Read, S: Skip<R>, P, O>(
        &mut self,
        reader: &mut R,
        mut parse: P,
    ) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8], Option<ParsingState>) -> Result<O, ParsingErrorState>,
    {
        self.load_and_parse_with_offset::<R, S, _, _>(
            reader,
            |data, _, state| parse(data, state),
            0,
        )
    }

    #[tracing::instrument(skip_all)]
    fn load_and_parse_with_offset<R: Read, S: Skip<R>, P, O>(
        &mut self,
        reader: &mut R,
        mut parse: P,
        offset: usize,
    ) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8], usize, Option<ParsingState>) -> Result<O, ParsingErrorState>,
    {
        if offset >= self.buffer().len() {
            self.fill_buf(reader, MIN_GROW_SIZE)?;
        }

        let mut parsing_state: Option<ParsingState> = None;
        loop {
            let res = parse(self.buffer(), offset, parsing_state.take());
            match res {
                Ok(o) => return Ok(o),
                Err(es) => {
                    tracing::debug!(?es);
                    parsing_state = es.state;

                    match es.err {
                        ParsingError::ClearAndSkip(n) => {
                            self.clear_and_skip::<R, S>(reader, n)?;
                        }
                        ParsingError::Need(i) => {
                            tracing::debug!(need = i, "need more bytes");
                            let to_read = max(i, MIN_GROW_SIZE);
                            let to_read = min(to_read, MAX_GROW_SIZE);

                            let n = self.fill_buf(reader, to_read)?;
                            if n == 0 {
                                return Err(ParsedError::NoEnoughBytes);
                            }
                            tracing::debug!(n, "actual read");
                        }
                        ParsingError::Failed(s) => return Err(ParsedError::Failed(s)),
                    }
                }
            }
        }
    }

    #[tracing::instrument(skip(reader))]
    fn clear_and_skip<R: Read, S: Skip<R>>(
        &mut self,
        reader: &mut R,
        n: usize,
    ) -> Result<(), ParsedError> {
        tracing::debug!("ClearAndSkip");
        if n <= self.buffer().len() {
            tracing::debug!(n, "skip by set_position");
            self.set_position(n);
            return Ok(());
        }

        let skip_n = n - self.buffer().len();
        tracing::debug!(skip_n, "clear and skip bytes");
        self.clear();

        let done = S::skip_by_seek(reader, skip_n.try_into().unwrap())?;
        if !done {
            tracing::debug!(skip_n, "skip by using our buffer");
            let mut skipped = 0;
            while skipped < skip_n {
                let mut to_skip = skip_n - skipped;
                to_skip = min(to_skip, MAX_ALLOC_SIZE);
                let n = self.fill_buf(reader, to_skip)?;
                skipped += n;
                if skipped <= skip_n {
                    self.clear();
                } else {
                    let remain = skipped - skip_n;
                    self.set_position(self.buffer().len() - remain);
                    break;
                }
            }
        } else {
            tracing::debug!(skip_n, "skip with seek");
        }

        if self.buffer().is_empty() {
            self.fill_buf(reader, MIN_GROW_SIZE)?;
        }
        Ok(())
    }
}

impl BufParser for MediaParser {
    #[tracing::instrument(skip(self, reader))]
    fn fill_buf<R: Read>(&mut self, reader: &mut R, size: usize) -> io::Result<usize> {
        if size > MAX_ALLOC_SIZE {
            tracing::error!(?size, "the requested buffer size is too big");
            return Err(io::ErrorKind::Unsupported.into());
        }
        self.buf_mut().reserve_exact(size);

        let n = reader.take(size as u64).read_to_end(self.buf_mut())?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }

        Ok(n)
    }
}

impl Buf for MediaParser {
    fn buffer(&self) -> &[u8] {
        &self.buf()[self.position..]
    }

    fn clear(&mut self) {
        self.buf_mut().clear();
    }

    fn set_position(&mut self, pos: usize) {
        self.position = pos;
    }

    fn position(&self) -> usize {
        self.position
    }
}

pub trait ParseOutput<R, S>: Sized {
    fn parse(parser: &mut MediaParser, ms: MediaSource<R, S>) -> crate::Result<Self>;
}

impl<R: Read, S: Skip<R>> ParseOutput<R, S> for ExifIter {
    fn parse(parser: &mut MediaParser, mut ms: MediaSource<R, S>) -> crate::Result<Self> {
        if !ms.has_exif() {
            return Err(crate::Error::ParseFailed("no Exif data here".into()));
        }
        parse_exif_iter::<R, S>(parser, ms.mime.unwrap_image(), &mut ms.reader)
    }
}

impl<R: Read, S: Skip<R>> ParseOutput<R, S> for TrackInfo {
    fn parse(parser: &mut MediaParser, mut ms: MediaSource<R, S>) -> crate::Result<Self> {
        if !ms.has_track() {
            return Err(crate::Error::ParseFailed("no track info here".into()));
        }
        let out = parser.load_and_parse::<R, S, _, _>(ms.reader.by_ref(), |data, _| {
            parse_track_info(data, ms.mime.unwrap_video())
                .map_err(|e| ParsingErrorState::new(e, None))
        })?;
        Ok(out)
    }
}

/// A `MediaParser`/`AsyncMediaParser` can parse media info from a
/// [`MediaSource`].
///
/// `MediaParser`/`AsyncMediaParser` manages inner parse buffers that can be
/// shared between multiple parsing tasks, thus avoiding frequent memory
/// allocations.
///
/// Therefore:
///
/// - Try to reuse a `MediaParser`/`AsyncMediaParser` instead of creating a new
///   one every time you need it.
///   
/// - `MediaSource` should be created directly from `Read`, not from `BufRead`.
///
/// ## Example
///
/// ```rust
/// use nom_exif::*;
/// use chrono::DateTime;
///
/// let mut parser = MediaParser::new();
///
/// // ------------------- Parse Exif Info
/// let ms = MediaSource::file_path("./testdata/exif.heic").unwrap();
/// assert!(ms.has_exif());
/// let mut iter: ExifIter = parser.parse(ms).unwrap();
///
/// let entry = iter.next().unwrap();
/// assert_eq!(entry.tag().unwrap(), ExifTag::Make);
/// assert_eq!(entry.get_value().unwrap().as_str().unwrap(), "Apple");
///
/// // Convert `ExifIter` into an `Exif`. Clone it before converting, so that
/// // we can start the iteration from the beginning.
/// let exif: Exif = iter.clone().into();
/// assert_eq!(exif.get(ExifTag::Make).unwrap().as_str().unwrap(), "Apple");
///
/// // ------------------- Parse Track Info
/// let ms = MediaSource::file_path("./testdata/meta.mov").unwrap();
/// assert!(ms.has_track());
/// let info: TrackInfo = parser.parse(ms).unwrap();
///
/// assert_eq!(info.get(TrackInfoTag::Make), Some(&"Apple".into()));
/// assert_eq!(info.get(TrackInfoTag::Model), Some(&"iPhone X".into()));
/// assert_eq!(info.get(TrackInfoTag::GpsIso6709), Some(&"+27.1281+100.2508+000.000/".into()));
/// assert_eq!(info.get_gps_info().unwrap().latitude_ref, 'N');
/// assert_eq!(
///     info.get_gps_info().unwrap().latitude,
///     [(27, 1), (7, 1), (68, 100)].into(),
/// );
/// ```
pub struct MediaParser {
    bb: Buffers,
    buf: Option<Vec<u8>>,
    position: usize,
}

impl Debug for MediaParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaParser")
            .field("buffers", &self.bb)
            .field("buf len", &self.buf.as_ref().map(|x| x.len()))
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl Default for MediaParser {
    fn default() -> Self {
        Self {
            bb: Buffers::new(),
            buf: None,
            position: 0,
        }
    }
}

pub(crate) trait ShareBuf {
    fn share_buf(&mut self, range: Range<usize>) -> PartialVec;
}

impl ShareBuf for MediaParser {
    fn share_buf(&mut self, mut range: Range<usize>) -> PartialVec {
        let buf = self.buf.take().unwrap();
        let vec = self.bb.release_to_share(buf);
        range.start += self.position;
        range.end += self.position;
        PartialVec::new(vec, range)
    }
}

impl MediaParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// `MediaParser`/`AsyncMediaParser` comes with its own buffer management,
    /// so that buffers can be reused during multiple parsing processes to
    /// avoid frequent memory allocations. Therefore, try to reuse a
    /// `MediaParser` instead of creating a new one every time you need it.
    ///     
    /// **Note**:
    ///
    /// - For [`ExifIter`] as parse output, Please avoid holding the `ExifIter`
    ///   object all the time and drop it immediately after use. Otherwise, the
    ///   parsing buffer referenced by the `ExifIter` object will not be reused
    ///   by [`MediaParser`], resulting in repeated memory allocation in the
    ///   subsequent parsing process.
    ///
    ///   If you really need to retain some data, please take out the required
    ///   Entry values ​​and save them, or convert the `ExifIter` into an
    ///   [`crate::Exif`] object to retain all Entry values.
    ///
    /// - For [`TrackInfo`] as parse output, you don't need to worry about
    ///   this, because `TrackInfo` dosn't reference the parsing buffer.
    pub fn parse<R: Read, S, O: ParseOutput<R, S>>(
        &mut self,
        mut ms: MediaSource<R, S>,
    ) -> crate::Result<O> {
        self.reset();
        self.acquire_buf();

        self.buf_mut().append(&mut ms.buf);
        let res = self.do_parse(ms);

        self.reset();
        res
    }

    fn do_parse<R: Read, S, O: ParseOutput<R, S>>(
        &mut self,
        mut ms: MediaSource<R, S>,
    ) -> Result<O, crate::Error> {
        self.fill_buf(&mut ms.reader, INIT_BUF_SIZE)?;
        let res = ParseOutput::parse(self, ms)?;
        Ok(res)
    }

    fn reset(&mut self) {
        // Ensure buf has been released
        if let Some(buf) = self.buf.take() {
            self.bb.release(buf);
        }

        // Reset position
        self.set_position(0);
    }

    pub(crate) fn buf(&self) -> &Vec<u8> {
        match self.buf.as_ref() {
            Some(b) => b,
            None => panic!("no buf here"),
        }
    }

    fn buf_mut(&mut self) -> &mut Vec<u8> {
        match self.buf.as_mut() {
            Some(b) => b,
            None => panic!("no buf here"),
        }
    }

    fn acquire_buf(&mut self) {
        assert!(self.buf.is_none());
        self.buf = Some(self.bb.acquire());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{LazyLock, Mutex, MutexGuard};

    use super::*;
    use test_case::case;

    enum TrackExif {
        Track,
        Exif,
        NoData,
        Invalid,
    }
    use TrackExif::*;

    static PARSER: LazyLock<Mutex<MediaParser>> = LazyLock::new(|| Mutex::new(MediaParser::new()));
    fn parser() -> MutexGuard<'static, MediaParser> {
        PARSER.lock().unwrap()
    }

    #[case("3gp_640x360.3gp", Track)]
    #[case("broken.jpg", Exif)]
    #[case("compatible-brands-fail.heic", Invalid)]
    #[case("compatible-brands-fail.mov", Invalid)]
    #[case("compatible-brands.heic", NoData)]
    #[case("compatible-brands.mov", NoData)]
    #[case("embedded-in-heic.mov", Track)]
    #[case("exif.heic", Exif)]
    #[case("exif.jpg", Exif)]
    #[case("fujifilm_x_t1_01.raf.meta", Exif)]
    #[case("meta.mov", Track)]
    #[case("meta.mp4", Track)]
    #[case("mka.mka", Track)]
    #[case("mkv_640x360.mkv", Track)]
    #[case("exif-one-entry.heic", Exif)]
    #[case("no-exif.jpg", NoData)]
    #[case("tif.tif", Exif)]
    #[case("ramdisk.img", Invalid)]
    #[case("webm_480.webm", Track)]
    fn parse_media(path: &str, te: TrackExif) {
        let mut parser = parser();
        let ms = MediaSource::file_path(Path::new("testdata").join(path));
        match te {
            Track => {
                let ms = ms.unwrap();
                // println!("path: {path} mime: {:?}", ms.mime);
                assert!(ms.has_track());
                let _: TrackInfo = parser.parse(ms).unwrap();
            }
            Exif => {
                let ms = ms.unwrap();
                // println!("path: {path} mime: {:?}", ms.mime);
                assert!(ms.has_exif());
                let mut it: ExifIter = parser.parse(ms).unwrap();
                let _ = it.parse_gps_info();

                if path.contains("one-entry") {
                    assert!(it.next().is_some());
                    assert!(it.next().is_none());

                    let exif: crate::Exif = it.clone_and_rewind().into();
                    assert!(exif.get(ExifTag::Orientation).is_some());
                } else {
                    let _: crate::Exif = it.clone_and_rewind().into();
                }
            }
            NoData => {
                let ms = ms.unwrap();
                // println!("path: {path} mime: {:?}", ms.mime);
                if ms.has_exif() {
                    let res: Result<ExifIter, _> = parser.parse(ms);
                    res.unwrap_err();
                } else if ms.has_track() {
                    let res: Result<TrackInfo, _> = parser.parse(ms);
                    res.unwrap_err();
                }
            }
            Invalid => {
                ms.unwrap_err();
            }
        }
    }

    use crate::testkit::open_sample;
    use crate::{EntryValue, ExifTag, TrackInfoTag};
    use chrono::DateTime;
    use test_case::test_case;

    use crate::video::TrackInfoTag::*;

    #[test_case("mkv_640x360.mkv", ImageWidth, 640_u32.into())]
    #[test_case("mkv_640x360.mkv", ImageHeight, 360_u32.into())]
    #[test_case("mkv_640x360.mkv", DurationMs, 13346_u64.into())]
    #[test_case("mkv_640x360.mkv", CreateDate, DateTime::parse_from_str("2008-08-08T08:08:08Z", "%+").unwrap().into())]
    #[test_case("meta.mov", Make, "Apple".into())]
    #[test_case("meta.mov", Model, "iPhone X".into())]
    #[test_case("meta.mov", GpsIso6709, "+27.1281+100.2508+000.000/".into())]
    #[test_case("meta.mp4", ImageWidth, 1920_u32.into())]
    #[test_case("meta.mp4", ImageHeight, 1080_u32.into())]
    #[test_case("meta.mp4", DurationMs, 1063_u64.into())]
    #[test_case("meta.mp4", GpsIso6709, "+27.2939+112.6932/".into())]
    #[test_case("meta.mp4", CreateDate, DateTime::parse_from_str("2024-02-03T07:05:38Z", "%+").unwrap().into())]
    #[test_case("udta.auth.mp4", Author, "UÄReplayKitRecording".into(); "udta author")]
    #[test_case("auth.mov", Author, "ReplayKitRecording".into(); "mov author")]
    fn parse_track_info(path: &str, tag: TrackInfoTag, v: EntryValue) {
        let mut parser = parser();

        let mf = MediaSource::file(open_sample(path).unwrap()).unwrap();
        let info: TrackInfo = parser.parse(mf).unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);

        let mf = MediaSource::unseekable(open_sample(path).unwrap()).unwrap();
        let info: TrackInfo = parser.parse(mf).unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);
    }
}

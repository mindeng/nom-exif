use std::{
    cmp::{max, min},
    fmt::{Debug, Display},
    fs::File,
    io::{self, Read, Seek},
    net::TcpStream,
    ops::Range,
    path::Path,
};

use crate::{
    buffer::Buffers,
    error::{ParsedError, ParsingError, ParsingErrorState},
    exif::{parse_exif_iter, TiffHeader},
    file::MediaMime,
    partial_vec::PartialVec,
    video::parse_track_info,
    ExifIter, TrackInfo,
};

/// A function that tries to skip `n` bytes of `reader` by seeking. Returns
/// `Ok(true)` on success, `Ok(false)` if the reader does not support seek
/// (so the caller should fall back to reading-and-discarding), or
/// `Err(io::Error)` if seek itself failed (e.g. truncated file handle).
///
/// This is captured at construction time by `MediaSource::seekable` /
/// `unseekable`, replacing the v2 `S: Skip<R>` phantom parameter with a
/// runtime fn pointer.
pub(crate) type SkipBySeekFn<R> = fn(&mut R, u64) -> io::Result<bool>;

/// `MediaSource` represents a media data source that can be parsed by
/// [`MediaParser`].
///
/// - Use [`MediaSource::file_path`] or [`MediaSource::file`] to create
///   a MediaSource from a file
///
/// - Use [`MediaSource::tcp_stream`] to create a MediaSource from a `TcpStream`
///
/// - In other cases:
///
///   - Use [`MediaSource::seekable`] to create a MediaSource from a `Read + Seek`
///
///   - Use [`MediaSource::unseekable`] to create a MediaSource from a
///     reader that only impl `Read`
///
/// *Note*: Please use [`MediaSource::seekable`] in preference to [`MediaSource::unseekable`],
/// since the former is more efficient when the parser needs to skip a large number of bytes.
///
/// Passing in a `BufRead` should be avoided because [`MediaParser`] comes with
/// its own buffer management and the buffers can be shared between multiple
/// parsing tasks, thus avoiding frequent memory allocations.
pub struct MediaSource<R> {
    pub(crate) reader: R,
    pub(crate) buf: Vec<u8>,
    pub(crate) mime: MediaMime,
    pub(crate) skip_by_seek: SkipBySeekFn<R>,
}

/// Top-level classification of a media source.
///
/// `Image` files carry EXIF metadata (parse with `MediaParser::parse_exif`);
/// `Track` files are time-based containers — video, audio, or both — and
/// carry track-info metadata (parse with `MediaParser::parse_track`). Pure
/// audio containers like `.mka` are classified as `Track`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Track,
}

impl<R> Debug for MediaSource<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaSource")
            .field("mime", &self.mime)
            .finish_non_exhaustive()
    }
}

// Should be enough for parsing header
const HEADER_PARSE_BUF_SIZE: usize = 128;

impl<R: Read> MediaSource<R> {
    fn build(mut reader: R, skip_by_seek: SkipBySeekFn<R>) -> crate::Result<Self> {
        let mut buf = Vec::with_capacity(HEADER_PARSE_BUF_SIZE);
        reader
            .by_ref()
            .take(HEADER_PARSE_BUF_SIZE as u64)
            .read_to_end(&mut buf)?;
        let mime: MediaMime = buf.as_slice().try_into()?;
        Ok(Self {
            reader,
            buf,
            mime,
            skip_by_seek,
        })
    }

    pub fn kind(&self) -> MediaKind {
        match self.mime {
            MediaMime::Image(_) => MediaKind::Image,
            MediaMime::Track(_) => MediaKind::Track,
        }
    }

    // Legacy alongside; deleted in Task 13.
    pub fn has_track(&self) -> bool {
        matches!(self.mime, MediaMime::Track(_))
    }

    pub fn has_exif(&self) -> bool {
        matches!(self.mime, MediaMime::Image(_))
    }

    /// Use [`MediaSource::unseekable`] to create a MediaSource from a
    /// reader that only impl `Read`
    ///
    /// *Note*: Please use [`MediaSource::seekable`] in preference to [`MediaSource::unseekable`],
    /// since the former is more efficient when the parser needs to skip a large number of bytes.
    pub fn unseekable(reader: R) -> crate::Result<Self> {
        Self::build(reader, |_, _| Ok(false))
    }
}

impl<R: Read + Seek> MediaSource<R> {
    /// Use [`MediaSource::seekable`] to create a MediaSource from a `Read + Seek`
    ///
    /// *Note*: Please use [`MediaSource::seekable`] in preference to [`MediaSource::unseekable`],
    /// since the former is more efficient when the parser needs to skip a large number of bytes.
    pub fn seekable(reader: R) -> crate::Result<Self> {
        Self::build(reader, |r, n| {
            let signed: i64 = n
                .try_into()
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
            r.seek_relative(signed)?;
            Ok(true)
        })
    }
}

impl MediaSource<File> {
    /// Open a file at `path` and parse its header to detect the media format.
    ///
    /// This is the v3-preferred entry point for the common case of "I have a
    /// path on disk". For an already-open file handle use [`Self::from_file`];
    /// for a generic `Read + Seek` source use [`Self::seekable`].
    pub fn open<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::seekable(File::open(path)?)
    }

    /// Wrap an already-open `File` and parse its header.
    pub fn from_file(file: File) -> crate::Result<Self> {
        Self::seekable(file)
    }

    // v2-shape constructors (deleted at end of P3; tests still use them).
    pub fn file_path<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::open(path)
    }

    pub fn file(file: File) -> crate::Result<Self> {
        Self::from_file(file)
    }
}

impl MediaSource<TcpStream> {
    pub fn tcp_stream(stream: TcpStream) -> crate::Result<Self> {
        Self::unseekable(stream)
    }
}

// Keep align with 4K
pub(crate) const INIT_BUF_SIZE: usize = 4096;
pub(crate) const MIN_GROW_SIZE: usize = 4096;
// Max size of APP1 is 0xFFFF
// pub(crate) const MAX_GROW_SIZE: usize = 63 * 1024;
// Set a reasonable upper limit for single buffer allocation.
pub(crate) const MAX_ALLOC_SIZE: usize = 1024 * 1024 * 1024;

pub(crate) trait Buf {
    fn buffer(&self) -> &[u8];
    fn clear(&mut self);

    fn set_position(&mut self, pos: usize);
    #[allow(unused)]
    fn position(&self) -> usize;
}

/// Buffer-management state shared between `MediaParser` and `AsyncMediaParser`.
#[derive(Debug, Default)]
pub(crate) struct BufferedParserState {
    bb: Buffers,
    buf: Option<Vec<u8>>,
    position: usize,
}

impl BufferedParserState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn reset(&mut self) {
        if let Some(buf) = self.buf.take() {
            self.bb.release(buf);
        }
        self.position = 0;
    }

    pub(crate) fn acquire_buf(&mut self) {
        debug_assert!(self.buf.is_none());
        self.buf = Some(self.bb.acquire());
    }

    pub(crate) fn buf(&self) -> &Vec<u8> {
        self.buf.as_ref().expect("no buf here")
    }

    pub(crate) fn buf_mut(&mut self) -> &mut Vec<u8> {
        self.buf.as_mut().expect("no buf here")
    }
}

impl Buf for BufferedParserState {
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

impl ShareBuf for BufferedParserState {
    fn share_buf(&mut self, mut range: Range<usize>) -> PartialVec {
        let buf = self.buf.take().expect("no buf to share");
        let vec = self.bb.release_to_share(buf);
        range.start += self.position;
        range.end += self.position;
        PartialVec::new(vec, range)
    }
}

/// What `clear_and_skip` should do, given the current buffer state and
/// the requested skip count.
pub(crate) enum SkipPlan {
    /// Skip is fully within the current buffer; just advance position.
    AdvanceOnly,
    /// Buffer must be cleared and `extra` bytes skipped from the reader.
    ClearAndSkip { extra: usize },
}

pub(crate) fn clear_and_skip_decide(buffer_len: usize, n: usize) -> SkipPlan {
    if n <= buffer_len {
        SkipPlan::AdvanceOnly
    } else {
        SkipPlan::ClearAndSkip { extra: n - buffer_len }
    }
}

pub(crate) fn check_fill_size(existing_len: usize, requested: usize) -> io::Result<()> {
    if requested.saturating_add(existing_len) > MAX_ALLOC_SIZE {
        tracing::error!(?requested, "the requested buffer size is too big");
        return Err(io::ErrorKind::Unsupported.into());
    }
    Ok(())
}

pub(crate) enum LoopAction<O> {
    /// Parse succeeded; return this value to the caller.
    Done(O),
    /// Need more bytes — call `fill_buf(reader, n)` then re-step.
    NeedFill(usize),
    /// Need to skip bytes — call `clear_and_skip(reader, n)` then re-step.
    Skip(usize),
    /// Parse failed permanently.
    Failed(String),
}

/// Closure type passed to [`parse_loop_step`].
pub(crate) type ParseFn<'a, O> =
    dyn FnMut(&[u8], usize, Option<ParsingState>) -> Result<O, ParsingErrorState> + 'a;

/// Drives one iteration of the parse-loop algorithm. Pure (no I/O).
pub(crate) fn parse_loop_step<O>(
    buffer: &[u8],
    offset: usize,
    parsing_state: &mut Option<ParsingState>,
    parse: &mut ParseFn<'_, O>,
) -> LoopAction<O> {
    match parse(buffer, offset, parsing_state.take()) {
        Ok(o) => LoopAction::Done(o),
        Err(es) => {
            *parsing_state = es.state;
            match es.err {
                ParsingError::Need(n) => LoopAction::NeedFill(n),
                ParsingError::ClearAndSkip(n) => LoopAction::Skip(n),
                ParsingError::Failed(s) => LoopAction::Failed(s),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ParsingState {
    TiffHeader(TiffHeader),
    HeifExifSize(usize),
    Cr3ExifSize(usize),
}

impl Display for ParsingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParsingState::TiffHeader(h) => Display::fmt(&format!("ParsingState: {h:?})"), f),
            ParsingState::HeifExifSize(n) => Display::fmt(&format!("ParsingState: {n}"), f),
            ParsingState::Cr3ExifSize(n) => Display::fmt(&format!("ParsingState: {n}"), f),
        }
    }
}

// Modern replacement for the `Load` trait in loader.rs. Adds offset-aware
// parsing and `ParsingState` threading for format-specific state machines.
pub(crate) trait BufParser: Buf + Debug {
    fn fill_buf<R: Read>(&mut self, reader: &mut R, size: usize) -> io::Result<usize>;

    fn load_and_parse<R: Read, P, O>(
        &mut self,
        reader: &mut R,
        skip_by_seek: SkipBySeekFn<R>,
        mut parse: P,
    ) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8], Option<ParsingState>) -> Result<O, ParsingErrorState>,
    {
        self.load_and_parse_with_offset(reader, skip_by_seek, |data, _, state| parse(data, state), 0)
    }

    #[tracing::instrument(skip_all)]
    fn load_and_parse_with_offset<R: Read, P, O>(
        &mut self,
        reader: &mut R,
        skip_by_seek: SkipBySeekFn<R>,
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
            match parse_loop_step(self.buffer(), offset, &mut parsing_state, &mut parse) {
                LoopAction::Done(o) => return Ok(o),
                LoopAction::NeedFill(needed) => {
                    let to_read = max(needed, MIN_GROW_SIZE);
                    let n = self.fill_buf(reader, to_read)?;
                    if n == 0 {
                        return Err(ParsedError::NoEnoughBytes);
                    }
                }
                LoopAction::Skip(n) => {
                    self.clear_and_skip(reader, skip_by_seek, n)?;
                }
                LoopAction::Failed(s) => return Err(ParsedError::Failed(s)),
            }
        }
    }

    #[tracing::instrument(skip(reader, skip_by_seek))]
    fn clear_and_skip<R: Read>(
        &mut self,
        reader: &mut R,
        skip_by_seek: SkipBySeekFn<R>,
        n: usize,
    ) -> Result<(), ParsedError> {
        match clear_and_skip_decide(self.buffer().len(), n) {
            SkipPlan::AdvanceOnly => {
                self.set_position(self.position() + n);
                Ok(())
            }
            SkipPlan::ClearAndSkip { extra: skip_n } => {
                self.clear();
                let done = (skip_by_seek)(
                    reader,
                    skip_n
                        .try_into()
                        .map_err(|_| ParsedError::Failed("skip too many bytes".into()))?,
                )?;
                if !done {
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
                }

                if self.buffer().is_empty() {
                    self.fill_buf(reader, MIN_GROW_SIZE)?;
                }
                Ok(())
            }
        }
    }
}

impl BufParser for MediaParser {
    #[tracing::instrument(skip(self, reader), fields(buf_len=self.state.buf().len()))]
    fn fill_buf<R: Read>(&mut self, reader: &mut R, size: usize) -> io::Result<usize> {
        check_fill_size(self.state.buf().len(), size)?;

        // Do not pre-allocate `size` bytes: a crafted box header can declare a
        // huge extended size (up to MAX_ALLOC_SIZE) that far exceeds the actual
        // stream length. reserve_exact would allocate that memory immediately
        // even when the reader has only a few bytes left. read_to_end grows the
        // buffer from the reader's actual size hint instead.
        let n = reader.take(size as u64).read_to_end(self.state.buf_mut())?;
        if n == 0 {
            tracing::error!(buf_len = self.state.buf().len(), "fill_buf: EOF");
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }

        tracing::debug!(
            ?size,
            ?n,
            buf_len = self.state.buf().len(),
            "fill_buf: read bytes"
        );

        Ok(n)
    }
}

impl Buf for MediaParser {
    fn buffer(&self) -> &[u8] {
        self.state.buffer()
    }

    fn clear(&mut self) {
        self.state.clear();
    }

    fn set_position(&mut self, pos: usize) {
        self.state.set_position(pos);
    }

    fn position(&self) -> usize {
        self.state.position()
    }
}

pub trait ParseOutput<R>: Sized {
    fn parse(parser: &mut MediaParser, ms: MediaSource<R>) -> crate::Result<Self>;
}

impl<R: Read> ParseOutput<R> for ExifIter {
    fn parse(parser: &mut MediaParser, mut ms: MediaSource<R>) -> crate::Result<Self> {
        if !ms.has_exif() {
            return Err(crate::Error::ExifNotFound);
        }
        parse_exif_iter(parser, ms.mime.unwrap_image(), &mut ms.reader, ms.skip_by_seek)
    }
}

impl<R: Read> ParseOutput<R> for TrackInfo {
    fn parse(parser: &mut MediaParser, mut ms: MediaSource<R>) -> crate::Result<Self> {
        if !ms.has_track() {
            return Err(crate::Error::TrackNotFound);
        }
        let mime_track = ms.mime.unwrap_track();
        let skip_by_seek = ms.skip_by_seek;
        let out = parser.load_and_parse(ms.reader.by_ref(), skip_by_seek, |data, _| {
            parse_track_info(data, mime_track).map_err(|e| ParsingErrorState::new(e, None))
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
    state: BufferedParserState,
}

impl Debug for MediaParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaParser")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Default for MediaParser {
    fn default() -> Self {
        Self {
            state: BufferedParserState::new(),
        }
    }
}

pub(crate) trait ShareBuf {
    fn share_buf(&mut self, range: Range<usize>) -> PartialVec;
}

impl ShareBuf for MediaParser {
    fn share_buf(&mut self, range: Range<usize>) -> PartialVec {
        self.state.share_buf(range)
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
    pub fn parse<R: Read, O: ParseOutput<R>>(
        &mut self,
        mut ms: MediaSource<R>,
    ) -> crate::Result<O> {
        self.reset();
        self.acquire_buf();

        self.buf_mut().append(&mut ms.buf);
        let res = self.do_parse(ms);

        self.reset();
        res
    }

    fn do_parse<R: Read, O: ParseOutput<R>>(
        &mut self,
        mut ms: MediaSource<R>,
    ) -> Result<O, crate::Error> {
        self.fill_buf(&mut ms.reader, INIT_BUF_SIZE)?;
        let res = ParseOutput::parse(self, ms)?;
        Ok(res)
    }

    fn reset(&mut self) {
        self.state.reset();
    }

    fn buf_mut(&mut self) -> &mut Vec<u8> {
        self.state.buf_mut()
    }

    fn acquire_buf(&mut self) {
        self.state.acquire_buf();
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
    #[case("exif-no-tz.jpg", Exif)]
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
    use crate::{EntryValue, Exif, ExifTag, TrackInfoTag};
    use chrono::{DateTime, FixedOffset, NaiveDateTime};
    use test_case::test_case;

    #[test_case("exif.jpg", ExifTag::DateTimeOriginal, DateTime::parse_from_str("2023-07-09T20:36:33+08:00", "%+").unwrap().into())]
    #[test_case("exif.heic", ExifTag::DateTimeOriginal, DateTime::parse_from_str("2022-07-22T21:26:32+08:00", "%+").unwrap().into())]
    #[test_case("exif.jpg", ExifTag::DateTimeOriginal, 
        (NaiveDateTime::parse_from_str("2023-07-09T20:36:33", "%Y-%m-%dT%H:%M:%S").unwrap(), 
            Some(FixedOffset::east_opt(8*3600).unwrap())).into())]
    #[test_case("exif-no-tz.jpg", ExifTag::DateTimeOriginal, 
        (NaiveDateTime::parse_from_str("2023-07-09T20:36:33", "%Y-%m-%dT%H:%M:%S").unwrap(), None).into())]
    fn parse_exif(path: &str, tag: ExifTag, v: EntryValue) {
        let mut parser = parser();

        let mf = MediaSource::seekable(open_sample(path).unwrap()).unwrap();
        assert!(mf.has_exif());
        let iter: ExifIter = parser.parse(mf).unwrap();
        let exif: Exif = iter.into();
        assert_eq!(exif.get(tag).unwrap(), &v);

        let mf = MediaSource::unseekable(open_sample(path).unwrap()).unwrap();
        assert!(mf.has_exif());
        let iter: ExifIter = parser.parse(mf).unwrap();
        let exif: Exif = iter.into();
        assert_eq!(exif.get(tag).unwrap(), &v);
    }

    use crate::video::TrackInfoTag::*;

    #[test_case("mkv_640x360.mkv", ImageWidth, 640_u32.into())]
    #[test_case("mkv_640x360.mkv", ImageHeight, 360_u32.into())]
    #[test_case("mkv_640x360.mkv", DurationMs, 13346_u64.into())]
    #[test_case("mkv_640x360.mkv", CreateDate, DateTime::parse_from_str("2008-08-08T08:08:08Z", "%+").unwrap().into())]
    #[test_case("meta.mov", Make, "Apple".into())]
    #[test_case("meta.mov", Model, "iPhone X".into())]
    #[test_case("meta.mov", GpsIso6709, "+27.1281+100.2508+000.000/".into())]
    #[test_case("meta.mov", CreateDate, DateTime::parse_from_str("2019-02-12T15:27:12+08:00", "%+").unwrap().into())]
    #[test_case("meta.mp4", ImageWidth, 1920_u32.into())]
    #[test_case("meta.mp4", ImageHeight, 1080_u32.into())]
    #[test_case("meta.mp4", DurationMs, 1063_u64.into())]
    #[test_case("meta.mp4", GpsIso6709, "+27.2939+112.6932/".into())]
    #[test_case("meta.mp4", CreateDate, DateTime::parse_from_str("2024-02-03T07:05:38Z", "%+").unwrap().into())]
    #[test_case("udta.auth.mp4", Author, "ReplayKitRecording".into(); "udta author")]
    #[test_case("auth.mov", Author, "ReplayKitRecording".into(); "mov author")]
    #[test_case("sony-a7-xavc.MP4", ImageWidth, 1920_u32.into())]
    #[test_case("sony-a7-xavc.MP4", ImageHeight, 1080_u32.into())]
    #[test_case("sony-a7-xavc.MP4", DurationMs, 1440_u64.into())]
    #[test_case("sony-a7-xavc.MP4", CreateDate, DateTime::parse_from_str("2026-04-26T09:25:15+00:00", "%+").unwrap().into())]
    fn parse_track_info(path: &str, tag: TrackInfoTag, v: EntryValue) {
        let mut parser = parser();

        let mf = MediaSource::seekable(open_sample(path).unwrap()).unwrap();
        let info: TrackInfo = parser.parse(mf).unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);

        let mf = MediaSource::unseekable(open_sample(path).unwrap()).unwrap();
        let info: TrackInfo = parser.parse(mf).unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);
    }

    #[test_case("crash_moov-trak")]
    #[test_case("crash_skip_large")]
    #[test_case("crash_add_large")]
    fn parse_track_crash(path: &str) {
        let mut parser = parser();

        let mf = MediaSource::file(open_sample(path).unwrap()).unwrap();
        let _: TrackInfo = parser.parse(mf).unwrap_or_default();

        let mf = MediaSource::unseekable(open_sample(path).unwrap()).unwrap();
        let _: TrackInfo = parser.parse(mf).unwrap_or_default();
    }

    #[test]
    fn parse_exif_on_track_returns_exif_not_found() {
        let mut parser = parser();
        let ms = MediaSource::file_path("testdata/meta.mov").unwrap();
        assert!(ms.has_track());
        let res: Result<ExifIter, _> = parser.parse(ms);
        assert!(matches!(res, Err(crate::Error::ExifNotFound)));
    }

    #[test]
    fn parse_track_on_image_returns_track_not_found() {
        let mut parser = parser();
        let ms = MediaSource::file_path("testdata/exif.jpg").unwrap();
        assert!(ms.has_exif());
        let res: Result<TrackInfo, _> = parser.parse(ms);
        assert!(matches!(res, Err(crate::Error::TrackNotFound)));
    }

    #[test]
    fn media_kind_classifies_image_and_track() {
        let img = MediaSource::file_path("testdata/exif.jpg").unwrap();
        assert_eq!(img.kind(), MediaKind::Image);

        let trk = MediaSource::file_path("testdata/meta.mov").unwrap();
        assert_eq!(trk.kind(), MediaKind::Track);
    }

    #[test]
    fn media_source_open_and_from_file() {
        use std::fs::File;

        // open(path) opens the file internally
        let ms = MediaSource::open("testdata/exif.jpg").unwrap();
        assert_eq!(ms.kind(), MediaKind::Image);

        // from_file(file) takes an already-open File
        let f = File::open("testdata/exif.jpg").unwrap();
        let ms = MediaSource::from_file(f).unwrap();
        assert_eq!(ms.kind(), MediaKind::Image);
    }
}

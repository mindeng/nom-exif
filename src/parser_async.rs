use std::{
    cmp::{max, min},
    fmt::Debug,
    io::{self},
    ops::Range,
    path::Path,
};

use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncSeek},
};

use crate::{
    error::{ParsedError, ParsingErrorState},
    exif::parse_exif_iter_async,
    parser::{
        check_fill_size, clear_and_skip_decide, parse_loop_step, Buf, BufferedParserState,
        LoopAction, ParsingState, ShareBuf, SkipPlan, INIT_BUF_SIZE, MAX_ALLOC_SIZE, MIN_GROW_SIZE,
    },
    partial_vec::PartialVec,
    video::parse_track_info,
    ExifIter, TrackInfo,
};

// Should be enough for parsing header
const HEADER_PARSE_BUF_SIZE: usize = 128;

/// Async counterpart to `crate::parser::SkipBySeekFn<R>`. Closures that
/// return a future cannot coerce to a plain `fn` type, so we use a fn pointer
/// to a `Pin<Box<dyn Future>>`-returning closure. The Box-per-skip overhead
/// is trivial against actual async I/O.
pub(crate) type AsyncSkipBySeekFn<R> = for<'a> fn(
    &'a mut R,
    u64,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<bool>> + Send + 'a>>;

pub struct AsyncMediaSource<R> {
    pub(crate) reader: R,
    pub(crate) buf: Vec<u8>,
    pub(crate) mime: crate::file::MediaMime,
    pub(crate) skip_by_seek: AsyncSkipBySeekFn<R>,
}

impl<R> Debug for AsyncMediaSource<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncMediaSource")
            .field("mime", &self.mime)
            .finish_non_exhaustive()
    }
}

impl<R: AsyncRead + Unpin> AsyncMediaSource<R> {
    async fn build(mut reader: R, skip_by_seek: AsyncSkipBySeekFn<R>) -> crate::Result<Self> {
        let mut buf = Vec::with_capacity(HEADER_PARSE_BUF_SIZE);
        (&mut reader)
            .take(HEADER_PARSE_BUF_SIZE as u64)
            .read_to_end(&mut buf)
            .await?;
        let mime: crate::file::MediaMime = buf.as_slice().try_into()?;
        Ok(Self {
            reader,
            buf,
            mime,
            skip_by_seek,
        })
    }

    pub fn kind(&self) -> crate::MediaKind {
        match self.mime {
            crate::file::MediaMime::Image(_) => crate::MediaKind::Image,
            crate::file::MediaMime::Track(_) => crate::MediaKind::Track,
        }
    }

    // Legacy alongside; deleted in Task 13.
    pub fn has_track(&self) -> bool {
        matches!(self.mime, crate::file::MediaMime::Track(_))
    }

    pub fn has_exif(&self) -> bool {
        matches!(self.mime, crate::file::MediaMime::Image(_))
    }
}

fn make_seekable_skip<R: AsyncRead + AsyncSeek + Unpin + Send>() -> AsyncSkipBySeekFn<R> {
    |r, n| {
        Box::pin(async move {
            use std::io::SeekFrom;
            use tokio::io::AsyncSeekExt;
            let signed: i64 = n
                .try_into()
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
            r.seek(SeekFrom::Current(signed)).await?;
            Ok(true)
        })
    }
}

fn make_unseekable_skip<R: AsyncRead + Unpin + Send>() -> AsyncSkipBySeekFn<R> {
    |_, _| Box::pin(async move { Ok(false) })
}

impl<R: AsyncRead + AsyncSeek + Unpin + Send> AsyncMediaSource<R> {
    pub async fn seekable(reader: R) -> crate::Result<Self> {
        Self::build(reader, make_seekable_skip::<R>()).await
    }
}

impl<R: AsyncRead + Unpin + Send> AsyncMediaSource<R> {
    pub async fn unseekable(reader: R) -> crate::Result<Self> {
        Self::build(reader, make_unseekable_skip::<R>()).await
    }
}

impl AsyncMediaSource<File> {
    /// Open a file at `path` (via `tokio::fs::File`) and parse its header.
    pub async fn open<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::seekable(File::open(path).await?).await
    }

    /// Wrap an already-open async `File` and parse its header.
    pub async fn from_file(file: File) -> crate::Result<Self> {
        Self::seekable(file).await
    }

    // Legacy aliases; deleted in Task 13.
    pub async fn file_path<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::open(path).await
    }

    pub async fn file(file: File) -> crate::Result<Self> {
        Self::from_file(file).await
    }
}

pub(crate) trait AsyncBufParser: Buf + Debug {
    async fn fill_buf<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        size: usize,
    ) -> io::Result<usize>;

    async fn load_and_parse<R: AsyncRead + Unpin, P, O>(
        &mut self,
        reader: &mut R,
        skip_by_seek: AsyncSkipBySeekFn<R>,
        parse: P,
    ) -> Result<O, ParsedError>
    where
        P: Fn(&[u8], Option<ParsingState>) -> Result<O, ParsingErrorState>,
    {
        self.load_and_parse_with_offset(
            reader,
            skip_by_seek,
            |data, _, state| parse(data, state),
            0,
        )
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn load_and_parse_with_offset<R: AsyncRead + Unpin, P, O>(
        &mut self,
        reader: &mut R,
        skip_by_seek: AsyncSkipBySeekFn<R>,
        parse: P,
        offset: usize,
    ) -> Result<O, ParsedError>
    where
        P: Fn(&[u8], usize, Option<ParsingState>) -> Result<O, ParsingErrorState>,
    {
        if offset >= self.buffer().len() {
            self.fill_buf(reader, MIN_GROW_SIZE).await?;
        }

        let mut parsing_state: Option<ParsingState> = None;
        let mut parse = parse; // coerce Fn → FnMut
        loop {
            match parse_loop_step(self.buffer(), offset, &mut parsing_state, &mut parse) {
                LoopAction::Done(o) => return Ok(o),
                LoopAction::NeedFill(needed) => {
                    let to_read = max(needed, MIN_GROW_SIZE);
                    let n = self.fill_buf(reader, to_read).await?;
                    if n == 0 {
                        return Err(ParsedError::NoEnoughBytes);
                    }
                }
                LoopAction::Skip(n) => {
                    self.clear_and_skip(reader, skip_by_seek, n).await?;
                }
                LoopAction::Failed(s) => return Err(ParsedError::Failed(s)),
            }
        }
    }

    #[tracing::instrument(skip(reader, skip_by_seek))]
    async fn clear_and_skip<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        skip_by_seek: AsyncSkipBySeekFn<R>,
        n: usize,
    ) -> Result<(), ParsedError> {
        match clear_and_skip_decide(self.buffer().len(), n) {
            SkipPlan::AdvanceOnly => {
                self.set_position(self.position() + n);
                return Ok(());
            }
            SkipPlan::ClearAndSkip { extra: skip_n } => {
                self.clear();
                let done = (skip_by_seek)(
                    reader,
                    skip_n
                        .try_into()
                        .map_err(|_| ParsedError::Failed("skip too many bytes".into()))?,
                )
                .await?;
                if !done {
                    let mut skipped = 0;
                    while skipped < skip_n {
                        let mut to_skip = skip_n - skipped;
                        to_skip = min(to_skip, MAX_ALLOC_SIZE);
                        let n = self.fill_buf(reader, to_skip).await?;
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
                    self.fill_buf(reader, MIN_GROW_SIZE).await?;
                }
                Ok(())
            }
        }
    }
}

pub trait AsyncParseOutput<R>: Sized {
    fn parse(
        parser: &mut AsyncMediaParser,
        ms: AsyncMediaSource<R>,
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send;
}

impl<R: AsyncRead + Unpin + Send> AsyncParseOutput<R> for ExifIter {
    async fn parse(
        parser: &mut AsyncMediaParser,
        mut ms: AsyncMediaSource<R>,
    ) -> crate::Result<Self> {
        if !ms.has_exif() {
            return Err(crate::Error::ExifNotFound);
        }
        parse_exif_iter_async(parser, ms.mime.unwrap_image(), &mut ms.reader, ms.skip_by_seek).await
    }
}

impl<R: AsyncRead + Unpin + Send> AsyncParseOutput<R> for TrackInfo {
    async fn parse(
        parser: &mut AsyncMediaParser,
        ms: AsyncMediaSource<R>,
    ) -> crate::Result<Self> {
        let mut ms = ms;
        let mime_track = match ms.mime {
            crate::file::MediaMime::Image(_) => return Err(crate::Error::TrackNotFound),
            crate::file::MediaMime::Track(t) => t,
        };
        let skip = ms.skip_by_seek;
        let out = parser
            .load_and_parse(&mut ms.reader, skip, |data, _| {
                parse_track_info(data, mime_track).map_err(|e| ParsingErrorState::new(e, None))
            })
            .await?;
        Ok(out)
    }
}

/// An async version of `MediaParser`. See [`crate::MediaParser`] for more
/// information.
///
/// ## Example
///
/// ```rust
/// use nom_exif::*;
/// use tokio::task::spawn_blocking;
/// use tokio::fs::File;
/// use chrono::DateTime;
///
/// #[cfg(feature = "tokio")]
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     let mut parser = AsyncMediaParser::new();
///
///     // ------------------- Parse Exif Info
///     let ms = AsyncMediaSource::file_path("./testdata/exif.heic").await.unwrap();
///     assert!(ms.has_exif());
///     let mut iter: ExifIter = parser.parse(ms).await.unwrap();
///
///     let entry = iter.next().unwrap();
///     assert_eq!(entry.tag().unwrap(), ExifTag::Make);
///     assert_eq!(entry.get_value().unwrap().as_str().unwrap(), "Apple");
///
///     // Convert `ExifIter` into an `Exif`. Clone it before converting, so that
///     // we can sure the iterator state has been reset.
///     let exif: Exif = iter.clone().into();
///     assert_eq!(exif.get(ExifTag::Make).unwrap().as_str().unwrap(), "Apple");
///
///     // ------------------- Parse Track Info
///     let ms = AsyncMediaSource::file_path("./testdata/meta.mov").await.unwrap();
///     assert!(ms.has_track());
///     let info: TrackInfo = parser.parse(ms).await.unwrap();
///
///     assert_eq!(info.get(TrackInfoTag::Make), Some(&"Apple".into()));
///     assert_eq!(info.get(TrackInfoTag::Model), Some(&"iPhone X".into()));
///     assert_eq!(info.get(TrackInfoTag::GpsIso6709), Some(&"+27.1281+100.2508+000.000/".into()));
///     assert_eq!(info.get_gps_info().unwrap().latitude_ref, 'N');
///     assert_eq!(
///         info.get_gps_info().unwrap().latitude,
///         [(27, 1), (7, 1), (68, 100)].into(),
///     );
///
///     Ok(())
/// }
/// ```
pub struct AsyncMediaParser {
    state: BufferedParserState,
}

impl Debug for AsyncMediaParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncMediaParser")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Default for AsyncMediaParser {
    fn default() -> Self {
        Self {
            state: BufferedParserState::new(),
        }
    }
}

impl ShareBuf for AsyncMediaParser {
    fn share_buf(&mut self, range: Range<usize>) -> PartialVec {
        self.state.share_buf(range)
    }
}

impl AsyncMediaParser {
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
    ///   by [`crate::MediaParser`], resulting in repeated memory allocation in the
    ///   subsequent parsing process.
    ///
    ///   If you really need to retain some data, please take out the required
    ///   Entry values ​​and save them, or convert the `ExifIter` into an
    ///   [`crate::Exif`] object to retain all Entry values.
    ///
    /// - For [`TrackInfo`] as parse output, you don't need to worry about
    ///   this, because `TrackInfo` dosn't reference the parsing buffer.
    pub async fn parse<R: AsyncRead + Unpin, O: AsyncParseOutput<R>>(
        &mut self,
        mut ms: AsyncMediaSource<R>,
    ) -> crate::Result<O> {
        self.reset();
        self.acquire_buf();

        self.buf_mut().append(&mut ms.buf);
        let res = self.do_parse(ms).await;

        self.reset();
        res
    }

    async fn do_parse<R: AsyncRead + Unpin, O: AsyncParseOutput<R>>(
        &mut self,
        mut ms: AsyncMediaSource<R>,
    ) -> Result<O, crate::Error> {
        self.fill_buf(&mut ms.reader, INIT_BUF_SIZE).await?;
        let res = O::parse(self, ms).await?;
        Ok(res)
    }

    fn reset(&mut self) {
        self.state.reset();
    }


    fn acquire_buf(&mut self) {
        self.state.acquire_buf();
    }

    fn buf_mut(&mut self) -> &mut Vec<u8> {
        self.state.buf_mut()
    }
}

impl AsyncBufParser for AsyncMediaParser {
    #[tracing::instrument(skip(self, reader))]
    async fn fill_buf<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        size: usize,
    ) -> io::Result<usize> {
        check_fill_size(self.state.buf().len(), size)?;

        // Same rationale as the sync version: do not pre-allocate `size` bytes.
        let n = reader.take(size as u64).read_to_end(self.state.buf_mut()).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }

        Ok(n)
    }
}

impl Buf for AsyncMediaParser {
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use test_case::case;

    enum TrackExif {
        Track,
        Exif,
        NoData,
        Invalid,
    }
    use tokio::fs::File;
    use TrackExif::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    #[case("3gp_640x360.3gp", Track)]
    #[case("broken.jpg", Exif)]
    #[case("compatible-brands-fail.heic", Invalid)]
    #[case("compatible-brands-fail.mov", Invalid)]
    #[case("compatible-brands.heic", NoData)]
    #[case("compatible-brands.mov", NoData)]
    #[case("embedded-in-heic.mov", Track)]
    #[case("exif.heic", Exif)]
    #[case("exif.jpg", Exif)]
    #[case("meta.mov", Track)]
    #[case("meta.mp4", Track)]
    #[case("mka.mka", Track)]
    #[case("mkv_640x360.mkv", Track)]
    #[case("exif-one-entry.heic", Exif)]
    #[case("no-exif.jpg", NoData)]
    #[case("tif.tif", Exif)]
    #[case("ramdisk.img", Invalid)]
    #[case("webm_480.webm", Track)]
    async fn parse_media(path: &str, te: TrackExif) {
        let mut parser = AsyncMediaParser::new();
        let ms = AsyncMediaSource::file_path(Path::new("testdata").join(path)).await;
        match te {
            Track => {
                let ms = ms.unwrap();
                // println!("path: {path} mime: {:?}", ms.mime);
                assert!(ms.has_track());
                let _: TrackInfo = parser.parse(ms).await.unwrap();
            }
            Exif => {
                let ms = ms.unwrap();
                // println!("path: {path} mime: {:?}", ms.mime);
                assert!(ms.has_exif());
                let mut it: ExifIter = parser.parse(ms).await.unwrap();
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
                    let res: Result<ExifIter, _> = parser.parse(ms).await;
                    res.unwrap_err();
                } else if ms.has_track() {
                    let res: Result<TrackInfo, _> = parser.parse(ms).await;
                    res.unwrap_err();
                }
            }
            Invalid => {
                ms.unwrap_err();
            }
        }
    }

    use crate::{EntryValue, ExifTag, TrackInfoTag};
    use chrono::DateTime;
    use test_case::test_case;

    use crate::video::TrackInfoTag::*;

    #[tokio::test(flavor = "current_thread")]
    #[ignore] // allocates ~1GiB, run manually with: cargo test fill_buf_rejects_oversize -- --ignored
    async fn fill_buf_rejects_oversize_when_combined_with_existing() {
        use tokio::io::repeat;
        let mut parser = AsyncMediaParser::new();
        parser.state.acquire_buf();
        parser.state.buf_mut().resize(MAX_ALLOC_SIZE - 1024, 0);
        let mut r = repeat(0);
        let res = parser.fill_buf(&mut r, 2 * 1024).await;
        assert!(
            res.is_err(),
            "expected Err, got Ok"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
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
    async fn parse_track_info(path: &str, tag: TrackInfoTag, v: EntryValue) {
        let mut parser = AsyncMediaParser::new();

        let f = File::open(Path::new("testdata").join(path)).await.unwrap();
        let ms = AsyncMediaSource::file(f).await.unwrap();
        let info: TrackInfo = parser.parse(ms).await.unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);

        let f = File::open(Path::new("testdata").join(path)).await.unwrap();
        let ms = AsyncMediaSource::unseekable(f).await.unwrap();
        let info: TrackInfo = parser.parse(ms).await.unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn async_media_kind_classifies_image_and_track() {
        let img = AsyncMediaSource::file_path("testdata/exif.jpg").await.unwrap();
        assert_eq!(img.kind(), crate::MediaKind::Image);

        let trk = AsyncMediaSource::file_path("testdata/meta.mov").await.unwrap();
        assert_eq!(trk.kind(), crate::MediaKind::Track);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn async_media_source_open_and_from_file() {
        let ms = AsyncMediaSource::open("testdata/exif.jpg").await.unwrap();
        assert_eq!(ms.kind(), crate::MediaKind::Image);

        let f = tokio::fs::File::open("testdata/exif.jpg").await.unwrap();
        let ms = AsyncMediaSource::from_file(f).await.unwrap();
        assert_eq!(ms.kind(), crate::MediaKind::Image);
    }
}

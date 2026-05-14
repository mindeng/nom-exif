use std::{
    cmp::{max, min},
    fmt::Debug,
    io::{self},
};

#[cfg(feature = "tokio-fs")]
use std::path::Path;
#[cfg(feature = "tokio-fs")]
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek};

use crate::{
    error::{ParsedError, ParsingErrorState},
    parser::{
        clear_and_skip_decide, parse_loop_step, Buf, LoopAction, ParsingState, SkipPlan,
        MAX_PARSE_BUF_SIZE, MIN_GROW_SIZE,
    },
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
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = io::Result<bool>> + Send + 'a>,
>;

pub struct AsyncMediaSource<R> {
    pub(crate) reader: R,
    pub(crate) buf: Vec<u8>,
    pub(crate) mime: crate::file::MediaMime,
    pub(crate) skip_by_seek: AsyncSkipBySeekFn<R>,
    /// Set when this source was constructed via [`Self::from_memory`].
    /// The full payload lives here as a zero-copy [`bytes::Bytes`]; the
    /// async parse methods branch on this field to take the memory path
    /// instead of `fill_buf`-ing from `reader`.
    pub(crate) memory: Option<bytes::Bytes>,
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
            memory: None,
        })
    }

    pub fn kind(&self) -> crate::MediaKind {
        match self.mime {
            crate::file::MediaMime::Image(_) => crate::MediaKind::Image,
            crate::file::MediaMime::Track(_) => crate::MediaKind::Track,
        }
    }
}

impl AsyncMediaSource<tokio::io::Empty> {
    /// Build an [`AsyncMediaSource`] from an in-memory byte payload.
    ///
    /// Async counterpart of [`crate::MediaSource::from_memory`]. Returns
    /// `AsyncMediaSource<tokio::io::Empty>`, which satisfies the
    /// `<R: AsyncRead + Unpin + Send>` bound on
    /// [`MediaParser::parse_exif_async`](crate::MediaParser::parse_exif_async),
    /// [`parse_track_async`](crate::MediaParser::parse_track_async), and
    /// [`parse_image_metadata_async`](crate::MediaParser::parse_image_metadata_async)
    /// so a single async entry point per "what to parse" handles both
    /// streaming and in-memory inputs.
    ///
    /// Accepts any type convertible into [`bytes::Bytes`] — `Bytes`,
    /// `Vec<u8>`, `&'static [u8]`, `String`, `Box<[u8]>`, plus HTTP-stack
    /// body types implementing `Into<Bytes>`. Zero-copy: parsed
    /// `ExifIter` / sub-IFDs share the original `Bytes` via reference
    /// counting, no copy.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn run() -> Result<(), nom_exif::Error> {
    /// use nom_exif::{AsyncMediaSource, MediaKind, MediaParser};
    ///
    /// let bytes = tokio::fs::read("./testdata/exif.jpg").await?;
    /// let ms = AsyncMediaSource::from_memory(bytes)?;
    /// assert_eq!(ms.kind(), MediaKind::Image);
    ///
    /// let mut parser = MediaParser::new();
    /// let _iter = parser.parse_exif_async(ms).await?;
    /// # Ok(()) }
    /// ```
    pub fn from_memory(bytes: impl Into<bytes::Bytes>) -> crate::Result<Self> {
        let bytes = bytes.into();
        let head_end = bytes.len().min(HEADER_PARSE_BUF_SIZE);
        let mime: crate::file::MediaMime = bytes[..head_end].try_into()?;
        Ok(Self {
            reader: tokio::io::empty(),
            buf: Vec::new(),
            mime,
            // Placeholder: never invoked in memory mode (AdvanceOnly path).
            skip_by_seek: |_, _| Box::pin(async move { Ok(false) }),
            memory: Some(bytes),
        })
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

#[cfg(feature = "tokio-fs")]
impl AsyncMediaSource<File> {
    /// Open a file at `path` (via `tokio::fs::File`) and parse its header.
    /// For an already-open async `File` use [`Self::seekable`].
    pub async fn open<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::seekable(File::open(path).await?).await
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
                        to_skip = min(to_skip, MAX_PARSE_BUF_SIZE);
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{ExifIter, TrackInfo};
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
        use crate::MediaParser;
        let mut parser = MediaParser::new();
        let ms = AsyncMediaSource::open(Path::new("testdata").join(path)).await;
        match te {
            Track => {
                let ms = ms.unwrap();
                assert_eq!(ms.kind(), crate::MediaKind::Track);
                let _: TrackInfo = parser.parse_track_async(ms).await.unwrap();
            }
            Exif => {
                let ms = ms.unwrap();
                assert_eq!(ms.kind(), crate::MediaKind::Image);
                let mut it: ExifIter = parser.parse_exif_async(ms).await.unwrap();
                let _ = it.parse_gps();

                if path.contains("one-entry") {
                    assert!(it.next().is_some());
                    assert!(it.next().is_none());

                    let exif: crate::Exif = it.clone_rewound().into();
                    assert!(exif.get(ExifTag::Orientation).is_some());
                } else {
                    let _: crate::Exif = it.clone_rewound().into();
                }
            }
            NoData => {
                let ms = ms.unwrap();
                match ms.kind() {
                    crate::MediaKind::Image => {
                        let res = parser.parse_exif_async(ms).await;
                        res.unwrap_err();
                    }
                    crate::MediaKind::Track => {
                        let res = parser.parse_track_async(ms).await;
                        res.unwrap_err();
                    }
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

    #[test]
    fn fill_buf_check_rejects_oversize_when_combined_with_existing() {
        use crate::parser::check_fill_size;
        // The combined size guard used by both sync and async fill_buf.
        // existing=MAX-1024, requested=2*1024 => existing+requested > MAX => Err.
        let res = check_fill_size(MAX_PARSE_BUF_SIZE - 1024, 2 * 1024);
        assert!(res.is_err(), "expected Err, got Ok");
        // Below the threshold passes.
        let res = check_fill_size(MAX_PARSE_BUF_SIZE - 4096, 1024);
        assert!(res.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    #[test_case("mkv_640x360.mkv", Width, 640_u32.into())]
    #[test_case("mkv_640x360.mkv", Height, 360_u32.into())]
    #[test_case("mkv_640x360.mkv", DurationMs, 13346_u64.into())]
    #[test_case("mkv_640x360.mkv", CreateDate, DateTime::parse_from_str("2008-08-08T08:08:08Z", "%+").unwrap().into())]
    #[test_case("meta.mov", Make, "Apple".into())]
    #[test_case("meta.mov", Model, "iPhone X".into())]
    #[test_case("meta.mov", GpsIso6709, "+27.1281+100.2508+000.000/".into())]
    #[test_case("meta.mp4", Width, 1920_u32.into())]
    #[test_case("meta.mp4", Height, 1080_u32.into())]
    #[test_case("meta.mp4", DurationMs, 1063_u64.into())]
    #[test_case("meta.mp4", GpsIso6709, "+27.2939+112.6932/".into())]
    #[test_case("meta.mp4", CreateDate, DateTime::parse_from_str("2024-02-03T07:05:38Z", "%+").unwrap().into())]
    async fn parse_track_info(path: &str, tag: TrackInfoTag, v: EntryValue) {
        use crate::MediaParser;
        let mut parser = MediaParser::new();

        let f = File::open(Path::new("testdata").join(path)).await.unwrap();
        let ms = AsyncMediaSource::seekable(f).await.unwrap();
        let info: TrackInfo = parser.parse_track_async(ms).await.unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);

        let f = File::open(Path::new("testdata").join(path)).await.unwrap();
        let ms = AsyncMediaSource::unseekable(f).await.unwrap();
        let info: TrackInfo = parser.parse_track_async(ms).await.unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn async_media_kind_classifies_image_and_track() {
        let img = AsyncMediaSource::open("testdata/exif.jpg").await.unwrap();
        assert_eq!(img.kind(), crate::MediaKind::Image);

        let trk = AsyncMediaSource::open("testdata/meta.mov").await.unwrap();
        assert_eq!(trk.kind(), crate::MediaKind::Track);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn async_media_source_open() {
        let ms = AsyncMediaSource::open("testdata/exif.jpg").await.unwrap();
        assert_eq!(ms.kind(), crate::MediaKind::Image);
    }
}

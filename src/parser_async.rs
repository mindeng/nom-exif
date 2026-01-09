use std::{
    cmp::{max, min},
    fmt::Debug,
    io::{self},
    marker::PhantomData,
    ops::Range,
    path::Path,
};

use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncSeek},
};

use crate::{
    buffer::Buffers,
    error::{ParsedError, ParsingError, ParsingErrorState},
    exif::parse_exif_iter_async,
    file::Mime,
    parser::{Buf, ParsingState, ShareBuf, INIT_BUF_SIZE, MAX_ALLOC_SIZE, MIN_GROW_SIZE},
    partial_vec::PartialVec,
    skip::AsyncSkip,
    video::parse_track_info,
    ExifIter, Seekable, TrackInfo, Unseekable,
};

// Should be enough for parsing header
const HEADER_PARSE_BUF_SIZE: usize = 128;

pub struct AsyncMediaSource<R, S = Seekable> {
    pub(crate) reader: R,
    pub(crate) buf: Vec<u8>,
    pub(crate) mime: Mime,
    phantom: PhantomData<S>,
}

impl<R: AsyncRead + Unpin, S: AsyncSkip<R>> AsyncMediaSource<R, S> {
    async fn build(mut reader: R) -> crate::Result<Self> {
        // TODO: reuse MediaParser to parse header
        let mut buf = Vec::with_capacity(HEADER_PARSE_BUF_SIZE);
        (&mut reader)
            .take(HEADER_PARSE_BUF_SIZE as u64)
            .read_to_end(&mut buf)
            .await?;
        let mime: Mime = buf.as_slice().try_into()?;
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

impl<R: AsyncRead + AsyncSeek + Unpin + Send> AsyncMediaSource<R, Seekable> {
    pub async fn seekable(reader: R) -> crate::Result<Self> {
        Self::build(reader).await
    }
}

impl<R: AsyncRead + Unpin + Send> AsyncMediaSource<R, Unseekable> {
    pub async fn unseekable(reader: R) -> crate::Result<Self> {
        Self::build(reader).await
    }
}

impl AsyncMediaSource<File, Seekable> {
    pub async fn file(reader: File) -> crate::Result<Self> {
        Self::build(reader).await
    }

    pub async fn file_path<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::build(File::open(path).await?).await
    }
}

pub(crate) trait AsyncBufParser: Buf + Debug {
    async fn fill_buf<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        size: usize,
    ) -> io::Result<usize>;

    async fn load_and_parse<R: AsyncRead + Unpin, S: AsyncSkip<R>, P, O>(
        &mut self,
        reader: &mut R,
        parse: P,
    ) -> Result<O, ParsedError>
    where
        P: Fn(&[u8], Option<ParsingState>) -> Result<O, ParsingErrorState>,
    {
        self.load_and_parse_with_offset::<R, S, _, _>(
            reader,
            |data, _, state| parse(data, state),
            0,
        )
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn load_and_parse_with_offset<R: AsyncRead + Unpin, S: AsyncSkip<R>, P, O>(
        &mut self,
        reader: &mut R,
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
        loop {
            let res = parse(self.buffer(), offset, parsing_state.take());
            match res {
                Ok(o) => return Ok(o),
                Err(es) => {
                    tracing::debug!(?es);
                    parsing_state = es.state;

                    match es.err {
                        ParsingError::ClearAndSkip(n) => {
                            self.clear_and_skip::<R, S>(reader, n).await?;
                        }
                        ParsingError::Need(i) => {
                            tracing::debug!(need = i, "need more bytes");
                            let to_read = max(i, MIN_GROW_SIZE);
                            // let to_read = min(to_read, MAX_GROW_SIZE);

                            let n = self.fill_buf(reader, to_read).await?;
                            if n == 0 {
                                return Err(ParsedError::NoEnoughBytes);
                            }
                            tracing::debug!(actual_read = n, "has been read");
                        }
                        ParsingError::Failed(s) => return Err(ParsedError::Failed(s)),
                    }
                }
            }
        }
    }

    #[tracing::instrument(skip(reader))]
    async fn clear_and_skip<R: AsyncRead + Unpin, S: AsyncSkip<R>>(
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

        let done = S::skip_by_seek(reader, skip_n.try_into().unwrap()).await?;
        if !done {
            tracing::debug!(skip_n, "skip by using our buffer");
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
        } else {
            tracing::debug!(skip_n, "skip with seek");
        }

        if self.buffer().is_empty() {
            self.fill_buf(reader, MIN_GROW_SIZE).await?;
        }
        Ok(())
    }
}

pub trait AsyncParseOutput<R, S>: Sized {
    fn parse(
        parser: &mut AsyncMediaParser,
        ms: AsyncMediaSource<R, S>,
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send;
}

impl<R: AsyncRead + Unpin + Send, S: AsyncSkip<R> + Send> AsyncParseOutput<R, S> for ExifIter {
    async fn parse(
        parser: &mut AsyncMediaParser,
        mut ms: AsyncMediaSource<R, S>,
    ) -> crate::Result<Self> {
        if !ms.has_exif() {
            return Err(crate::Error::ParseFailed("no Exif data here".into()));
        }
        parse_exif_iter_async::<R, S>(parser, ms.mime.unwrap_image(), &mut ms.reader).await
    }
}

impl<R: AsyncRead + Unpin + Send, S: AsyncSkip<R> + Send> AsyncParseOutput<R, S> for TrackInfo {
    async fn parse(
        parser: &mut AsyncMediaParser,
        ms: AsyncMediaSource<R, S>,
    ) -> crate::Result<Self> {
        let mut ms = ms;
        let out = match ms.mime {
            Mime::Image(_) => return Err("not a track".into()),
            Mime::Video(v) => {
                parser
                    .load_and_parse::<R, S, _, _>(&mut ms.reader, |data, _| {
                        parse_track_info(data, v).map_err(|e| ParsingErrorState::new(e, None))
                    })
                    .await?
            }
        };

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
/// #[cfg(feature = "async")]
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
    bb: Buffers,
    buf: Option<Vec<u8>>,
    position: usize,
}

impl Debug for AsyncMediaParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaParser")
            .field("buffers", &self.bb)
            .field("buf len", &self.buf.as_ref().map(|x| x.len()))
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl<R, S: AsyncSkip<R>> Debug for AsyncMediaSource<R, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaSource")
            // .field("reader", &self.reader)
            .field("mime", &self.mime)
            .field("seekable", &S::debug())
            .finish_non_exhaustive()
    }
}

impl Default for AsyncMediaParser {
    fn default() -> Self {
        Self {
            bb: Buffers::new(),
            buf: None,
            position: 0,
        }
    }
}

impl ShareBuf for AsyncMediaParser {
    fn share_buf(&mut self, mut range: Range<usize>) -> PartialVec {
        let buf = self.buf.take().unwrap();
        let vec = self.bb.release_to_share(buf);
        range.start += self.position;
        range.end += self.position;
        PartialVec::new(vec, range)
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
    pub async fn parse<R: AsyncRead + Unpin, S, O: AsyncParseOutput<R, S>>(
        &mut self,
        mut ms: AsyncMediaSource<R, S>,
    ) -> crate::Result<O> {
        self.reset();
        self.acquire_buf();

        self.buf_mut().append(&mut ms.buf);
        let res = self.do_parse(ms).await;

        self.reset();
        res
    }

    async fn do_parse<R: AsyncRead + Unpin, S, O: AsyncParseOutput<R, S>>(
        &mut self,
        mut ms: AsyncMediaSource<R, S>,
    ) -> Result<O, crate::Error> {
        self.fill_buf(&mut ms.reader, INIT_BUF_SIZE).await?;
        let res = O::parse(self, ms).await?;
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

    fn buf(&self) -> &Vec<u8> {
        self.buf.as_ref().unwrap()
    }

    fn acquire_buf(&mut self) {
        assert!(self.buf.is_none());
        self.buf = Some(self.bb.acquire());
    }

    fn buf_mut(&mut self) -> &mut Vec<u8> {
        self.buf.as_mut().unwrap()
    }
}

impl AsyncBufParser for AsyncMediaParser {
    #[tracing::instrument(skip(self, reader))]
    async fn fill_buf<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        size: usize,
    ) -> io::Result<usize> {
        if size > MAX_ALLOC_SIZE {
            tracing::error!(?size, "the requested buffer size is too big");
            return Err(io::ErrorKind::Unsupported.into());
        }
        self.buf_mut().reserve_exact(size);

        let n = reader.take(size as u64).read_to_end(self.buf_mut()).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }

        // let n = reader.read_buf(&mut self.buf).await?;
        // if n == 0 {
        //     return Err(std::io::ErrorKind::UnexpectedEof.into());
        // }

        Ok(n)
    }
}

impl Buf for AsyncMediaParser {
    fn buffer(&self) -> &[u8] {
        &self.buf()[self.position()..]
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
}

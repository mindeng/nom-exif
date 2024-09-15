use std::{
    cmp::{max, min},
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
    error::{ParsedError, ParsingError},
    exif::{extract_exif_data, ExifParser},
    file::Mime,
    input::Input,
    parser::{
        Buffer, ParsingState, INIT_BUF_SIZE, MAX_GROW_SIZE, MAX_REUSE_BUF_SIZE, MIN_GROW_SIZE,
    },
    skip::AsyncSkip,
    slice::SubsliceRange as _,
    video::parse_track_info,
    ExifIter, Seekable, TrackInfo, Unseekable,
};

// Should be enough for parsing header
const HEADER_PARSE_BUF_SIZE: usize = 128;

#[derive(Debug)]
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

pub(crate) trait AsyncBufParser: Buffer {
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
        P: Fn(&[u8], Option<ParsingState>) -> Result<O, ParsingError>,
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
        P: Fn(&[u8], usize, Option<ParsingState>) -> Result<O, ParsingError>,
    {
        if offset >= self.buffer().len() {
            self.fill_buf(reader, MIN_GROW_SIZE).await?;
        }

        let mut parsing_state: Option<ParsingState> = None;
        loop {
            match parse(self.buffer(), offset, parsing_state.take()) {
                Ok(o) => return Ok(o),
                Err(ParsingError::ClearAndSkip(n, skip_state)) => {
                    tracing::debug!(n, ?skip_state, "ClearAndSkip");
                    if n <= self.buffer().len() {
                        tracing::debug!(n, "set_position");
                        self.set_position(n);
                    } else {
                        let skip_n = n - self.buffer().len();
                        tracing::debug!(skip_n, "clear and skip bytes");
                        self.clear();

                        let done = S::skip_by_seek(reader, skip_n.try_into().unwrap()).await?;
                        if !done {
                            tracing::debug!(skip_n, "skip within our buffer");
                            let mut skipped = 0;
                            while skipped < skip_n {
                                let n = self.fill_buf(reader, skip_n - skipped).await?;
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
                    }
                    parsing_state = skip_state;
                }
                Err(ParsingError::Need(i)) => {
                    tracing::debug!(need = i, "need more bytes");
                    let to_read = max(i, MIN_GROW_SIZE);
                    let to_read = min(to_read, MAX_GROW_SIZE);

                    let n = self.fill_buf(reader, to_read).await?;
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

pub trait AsyncParseOutput<'a, R, S>: Sized + 'a {
    fn parse(
        parser: &'a mut AsyncMediaParser,
        ms: AsyncMediaSource<R, S>,
    ) -> impl std::future::Future<Output = crate::Result<Self>> + Send;
}

impl<'a, R: AsyncRead + Unpin + Send, S: AsyncSkip<R> + Send> AsyncParseOutput<'a, R, S>
    for ExifIter<'a>
{
    async fn parse(
        parser: &'a mut AsyncMediaParser,
        ms: AsyncMediaSource<R, S>,
    ) -> crate::Result<Self> {
        let mut reader = ms.reader;
        let mime = ms.mime;
        let out = parser
            .load_and_parse::<R, S, _, Option<(Range<_>, Option<ParsingState>)>>(
                &mut reader,
                |buf, state| match mime {
                    Mime::Image(img) => {
                        let exif_data = extract_exif_data(img, buf, state.as_ref())?;
                        Ok(exif_data
                            .and_then(|x| buf.subslice_range(x))
                            .map(|x| (x, state)))
                    }
                    Mime::Video(_) => Err("not an image".into()),
                },
            )
            .await?;

        if let Some((range, state)) = out {
            let input: Input<'a> = parser.buffer()[range].into();
            let parser = ExifParser::new(input);
            let iter = parser.parse_iter(state)?;
            Ok(iter)
        } else {
            Err("parse exif failed".into())
        }
    }
}

impl<'a, R: AsyncRead + Unpin + Send, S: AsyncSkip<R> + Send> AsyncParseOutput<'a, R, S>
    for TrackInfo
{
    async fn parse(
        parser: &'a mut AsyncMediaParser,
        ms: AsyncMediaSource<R, S>,
    ) -> crate::Result<Self> {
        let mut ms = ms;
        let out = match ms.mime {
            Mime::Image(_) => return Err("not a track".into()),
            Mime::Video(v) => {
                parser
                    .load_and_parse::<R, S, _, _>(&mut ms.reader, |data, _| {
                        parse_track_info(data, v)
                    })
                    .await?
            }
        };

        Ok(out)
    }
}

/// An async version of [`crate::MediaParser`]
#[derive(Debug)]
pub struct AsyncMediaParser {
    buf: Vec<u8>,
    position: usize,
}

impl Default for AsyncMediaParser {
    fn default() -> Self {
        Self::with_capacity(INIT_BUF_SIZE)
    }
}

impl AsyncMediaParser {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_capacity(size: usize) -> Self {
        Self {
            buf: Vec::with_capacity(size),
            position: 0,
        }
    }

    /// `AsyncMediaParser` comes with its own buffer management, so that
    /// buffers can be reused during multiple parsing processes to avoid
    /// frequent memory allocations. Therefore, try to reuse a
    /// `AsyncMediaParser` instead of creating a new one every time you need
    /// it.
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
    ///     
    /// **Note**:
    ///
    /// - For [`ExifIter`] as parse output, the result must be dropped before
    ///   the next call of `parse()`, or there will be compiling errors.
    ///   
    ///   Since the inner data of `ExifIter` is borrowed from `MediaParser`,
    ///   and the next call of `parse()` will clear the data previously parsed.
    ///
    ///   If you want to save the Exif info for later use, then you should
    ///   convert the `ExifIter` into an [`Exif`], e.g.: `let exif: Exif =
    ///   iter.into()`.
    ///
    /// - For [`TrackInfo`] as parse output, don't worry about this, because
    ///   `TrackInfo` is an owned value type.
    pub async fn parse<'a, R: AsyncRead + Unpin, S, O: AsyncParseOutput<'a, R, S>>(
        &'a mut self,
        mut ms: AsyncMediaSource<R, S>,
    ) -> crate::Result<O> {
        self.clear();
        if self.buf.capacity() > MAX_REUSE_BUF_SIZE {
            self.buf.shrink_to(MAX_REUSE_BUF_SIZE);
        }

        self.buf.append(&mut ms.buf);
        self.fill_buf(&mut ms.reader, INIT_BUF_SIZE).await?;

        O::parse(self, ms).await
    }
}

impl AsyncBufParser for AsyncMediaParser {
    async fn fill_buf<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        size: usize,
    ) -> io::Result<usize> {
        self.buf.reserve_exact(size);

        let n = reader
            .take(size as u64)
            .read_to_end(self.buf.as_mut())
            .await?;
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

impl Buffer for AsyncMediaParser {
    fn buffer(&self) -> &[u8] {
        &self.buf[self.position()..]
    }

    fn clear(&mut self) {
        self.buf.clear();
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

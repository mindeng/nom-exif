use std::{
    cmp::{max, min},
    fs::File,
    io::{self, Read, Seek},
    marker::PhantomData,
    net::TcpStream,
    ops::Range,
    path::Path,
};

use crate::{
    error::{ParsedError, ParsingError},
    exif::ExifParser,
    file::Mime,
    heif,
    input::Input,
    jpeg,
    skip::Skip,
    slice::SubsliceRange,
    video::parse_track_info,
    ExifIter, Seekable, TrackInfo, Unseekable,
};

/// `MediaSource` represents a media data source that can be parsed by
/// [`MediaParser`].
///
/// - Use `MediaSource::file(path)` to create a MediaSource from a file
/// - Use `MediaSource::tcp_stream(stream)` to create a MediaSource from a `TcpStream`
/// - In other cases:
///
///   - Use `MediaSource::seekable(reader)` to create a MediaSource from a `Read + Seek`
///   
///   - Use `MediaSource::unseekable(read)` to create a MediaSource from a
///     reader that only impl `Read`
///   
/// `seekable` is preferred to `unseekable`, since the former is more efficient
/// when the parser needs to skip a large number of bytes.
///
/// Passing in a `BufRead` should be avoided because [`MediaParser`] comes with
/// its own buffer management and the buffer can be shared between multiple
/// parsing tasks, thus avoiding frequent memory allocations.

#[derive(Debug)]
pub struct MediaSource<R, S = Seekable> {
    read: R,
    buf: Vec<u8>,
    mime: Mime,
    phantom: PhantomData<S>,
}

// Should be enough for parsing header
const HEADER_PARSE_BUF_SIZE: usize = 128;

impl<R: Read, S: Skip<R>> MediaSource<R, S> {
    fn build(mut read: R) -> crate::Result<Self> {
        // TODO: reuse MediaParser to parse header
        let mut buf = Vec::with_capacity(HEADER_PARSE_BUF_SIZE);
        read.by_ref()
            .take(HEADER_PARSE_BUF_SIZE as u64)
            .read_to_end(&mut buf)?;
        let mime: Mime = buf.as_slice().try_into()?;
        Ok(Self {
            read,
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
    pub fn seekable(read: R) -> crate::Result<Self> {
        Self::build(read)
    }
}

impl<R: Read> MediaSource<R, Unseekable> {
    pub fn unseekable(read: R) -> crate::Result<Self> {
        Self::build(read)
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
const INIT_BUF_SIZE: usize = 4096 - HEADER_PARSE_BUF_SIZE;
const MIN_GROW_SIZE: usize = 4096;
// Max size of APP1 is 0xFFFF
const MAX_GROW_SIZE: usize = 63 * 1024;
// Set a reasonable value to avoid causing frequent memory allocations
const MAX_REUSE_BUF_SIZE: usize = 16 * 1024 * 1024;

pub(crate) trait Buffer {
    fn buffer(&self) -> &[u8];
    fn fill_buf<R: Read>(&mut self, read: &mut R, size: usize) -> io::Result<usize>;
    fn clear(&mut self);
}

pub(crate) trait BufParse: Buffer {
    fn load_and_parse<R: Read, S: Skip<R>, P, O>(
        &mut self,
        reader: &mut R,
        mut parse: P,
    ) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8]) -> Result<O, ParsingError>,
    {
        self.load_and_parse_with_offset::<R, S, _, _>(reader, |data, _| parse(data), 0)
    }

    #[tracing::instrument(skip_all)]
    fn load_and_parse_with_offset<R: Read, S: Skip<R>, P, O>(
        &mut self,
        reader: &mut R,
        mut parse: P,
        offset: usize,
    ) -> Result<O, ParsedError>
    where
        P: FnMut(&[u8], usize) -> Result<O, ParsingError>,
    {
        if offset >= self.buffer().len() {
            self.fill_buf(reader, MIN_GROW_SIZE)?;
        }

        loop {
            match parse(self.buffer(), offset) {
                Ok(o) => return Ok(o),
                Err(ParsingError::ClearAndSkip(n)) => {
                    tracing::debug!(n, "clear and skip bytes");
                    self.clear();
                    // self.skip(read, n)?;
                    S::skip(reader, n.try_into().unwrap())?;
                    self.fill_buf(reader, MIN_GROW_SIZE)?;
                }
                Err(ParsingError::Need(i)) => {
                    tracing::debug!(need = i, "need more bytes");
                    let to_read = max(i, MIN_GROW_SIZE);
                    let to_read = min(to_read, MAX_GROW_SIZE);

                    let n = self.fill_buf(reader, to_read)?;
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

impl<T: Buffer> BufParse for T {}

impl Buffer for MediaParser {
    fn buffer(&self) -> &[u8] {
        &self.buf
    }

    fn fill_buf<R: Read>(&mut self, read: &mut R, size: usize) -> io::Result<usize> {
        self.buf.reserve_exact(size);

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
    /// `MediaParser` comes with its own buffer management, so that buffers can
    /// be reused during multiple parsing processes to avoid frequent memory
    /// allocations. Therefore, try to reuse a `MediaParser` instead of
    /// creating a new one every time you need it.
    ///
    /// ## Example
    ///
    /// ```rust
    /// use nom_exif::*;
    /// use std::fs::File;
    /// use chrono::DateTime;
    ///
    /// let mut parser = MediaParser::new();
    ///
    /// // ------------------- Parse Exif Info
    /// let ms = MediaSource::file_path("./testdata/exif.heic").unwrap();
    /// let mut iter: ExifIter = parser.parse(ms).unwrap();
    ///
    /// let entry = iter.next().unwrap();
    /// assert_eq!(entry.tag().unwrap(), ExifTag::Make);
    /// assert_eq!(entry.get_value().unwrap().as_str().unwrap(), "Apple");
    ///
    /// // Convert `ExifIter` into an `Exif`. Clone it before converting, so that
    /// // we can sure the iterator state has been reset.
    /// let exif: Exif = iter.clone().into();
    /// assert_eq!(exif.get(ExifTag::Make).unwrap().as_str().unwrap(), "Apple");
    ///
    /// // ------------------- Parse Track Info
    /// let ms = MediaSource::file_path("./testdata/meta.mov").unwrap();
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
    pub fn parse<'a, R: Read, S, O: ParseOutput<'a, R, S>>(
        &'a mut self,
        mut ms: MediaSource<R, S>,
    ) -> crate::Result<O> {
        self.clear();
        if self.buf.capacity() > MAX_REUSE_BUF_SIZE {
            self.buf.shrink_to(MAX_REUSE_BUF_SIZE);
        }

        self.buf.append(&mut ms.buf);
        self.fill_buf(ms.read.by_ref(), INIT_BUF_SIZE)?;

        ParseOutput::parse(self, &mut ms)
    }
}

pub trait ParseOutput<'a, R, S>: Sized + 'a {
    fn parse(parser: &'a mut MediaParser, mf: &mut MediaSource<R, S>) -> crate::Result<Self>;
}

impl<'a, R: Read, S: Skip<R>> ParseOutput<'a, R, S> for ExifIter<'a> {
    fn parse(parser: &'a mut MediaParser, mf: &mut MediaSource<R, S>) -> crate::Result<Self> {
        let range =
            parser.load_and_parse::<R, S, _, Option<Range<_>>>(mf.read.by_ref(), |buf| match mf
                .mime
            {
                Mime::Image(img) => {
                    let (_, exif_data) = match img {
                        crate::file::MimeImage::Jpeg => jpeg::extract_exif_data(buf)?,
                        crate::file::MimeImage::Heic | crate::file::MimeImage::Heif => {
                            heif::extract_exif_data(buf)?
                        }
                    };

                    Ok(exif_data.and_then(|x| buf.subslice_range(x)))
                }
                Mime::Video(_) => Err("not an image".into()),
            })?;

        if let Some(range) = range {
            let input: Input<'a> = parser.buffer()[range].into();
            let parser = ExifParser::new(input);
            let iter = parser.parse_iter()?;
            Ok(iter)
        } else {
            Err("parse exif failed".into())
        }
    }
}

impl<'a, R: Read, S: Skip<R>> ParseOutput<'a, R, S> for TrackInfo {
    fn parse(parser: &mut MediaParser, mf: &mut MediaSource<R, S>) -> crate::Result<Self> {
        let out = match mf.mime {
            Mime::Image(_) => return Err("not a track".into()),
            Mime::Video(v) => parser
                .load_and_parse::<R, S, _, _>(mf.read.by_ref(), |data| parse_track_info(data, v))?,
        };

        Ok(out)
    }
}

/// A `MediaParser` can parse video/audio info from a [`MediaSource`].
///
/// MediaParser manages an inner parse buffer that can be shared between
/// multiple parsing tasks, thus avoiding frequent memory allocations.
pub struct MediaParser {
    buf: Vec<u8>,
}

impl Default for MediaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaParser {
    pub fn new() -> Self {
        Self::with_capacity(INIT_BUF_SIZE)
    }

    fn with_capacity(size: usize) -> Self {
        Self {
            buf: Vec::with_capacity(size),
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
    use test_case::case;

    enum TrackExif {
        Track,
        Exif,
        NoData,
        Invalid,
    }
    use TrackExif::*;

    #[case("3gp_640x360.3gp", Track)]
    #[case("broken.jpg", Exif)]
    #[case("compatible-brands-fail.heic", Invalid)]
    #[case("compatible-brands-fail.mov", Invalid)]
    #[case("compatible-brands.heic", NoData)]
    #[case("embedded-in-heic.mov", Track)]
    #[case("exif.heic", Exif)]
    #[case("exif.jpg", Exif)]
    #[case("meta.mov", Track)]
    #[case("meta.mp4", Track)]
    #[case("mka.mka", Track)]
    #[case("mkv_640x360.mkv", Track)]
    #[case("no-exif.heic", Exif)]
    #[case("no-exif.jpg", NoData)]
    // TODO:
    // #[case("png.png", Exif)]
    // #[case("tif.png", Exif)]
    #[case("ramdisk.img", Invalid)]
    #[case("webm_480.webm", Track)]
    fn parse_media(path: &str, te: TrackExif) {
        let mut parser = MediaParser::new();
        let ms = MediaSource::file_path(Path::new("testdata").join(path));
        match te {
            Track => {
                let ms = ms.unwrap();
                println!("path: {path} mime: {:?}", ms.mime);
                assert!(ms.has_track());
                let _: TrackInfo = parser.parse(ms).unwrap();
            }
            Exif => {
                let ms = ms.unwrap();
                println!("path: {path} mime: {:?}", ms.mime);
                assert!(ms.has_exif());
                let _: ExifIter = parser.parse(ms).unwrap();
            }
            NoData => {
                let ms = ms.unwrap();
                println!("path: {path} mime: {:?}", ms.mime);
                assert!(ms.has_exif());
                let res: Result<ExifIter, _> = parser.parse(ms);
                res.unwrap_err();
            }
            Invalid => {
                ms.unwrap_err();
            }
        }
    }

    // #[case("compatible-brands.mov", Track)]

    use crate::testkit::open_sample;
    use crate::{EntryValue, TrackInfoTag};
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
    fn parse_track_info(path: &str, tag: TrackInfoTag, v: EntryValue) {
        let mf = MediaSource::file(open_sample(path).unwrap()).unwrap();
        let info: TrackInfo = MediaParser::new().parse(mf).unwrap();
        assert_eq!(info.get(tag).unwrap(), &v);
    }
}

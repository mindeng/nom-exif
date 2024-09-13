use nom::{bytes::complete, multi::many0, FindSubstring, IResult};
use std::{
    fmt::Display,
    io::{Cursor, Read},
};

use crate::{
    bbox::{travel_header, BoxHolder}, ebml::element::parse_ebml_doc_type, error::{ParsedError, ParsingError}, exif::TiffHeader, heif, jpeg::{self, check_jpeg, check_jpeg_exif}, loader::Load, slice::SubsliceRange
};

const HEIF_HEIC_BRAND_NAMES: &[&[u8]] = &[
    b"heic", // the usual HEIF images
    b"heix", // 10bit images, or anything that uses h265 with range extension
    b"hevc", // 'hevx': brands for image sequences
    b"heim", // multiview
    b"heis", // scalable
    b"hevm", // multiview sequence
    b"hevs", // scalable sequence
    b"mif1", b"MiHE", b"miaf", b"MiHB", // HEIC file's compatible brands
];

const HEIC_BRAND_NAMES: &[&[u8]] = &[b"heic", b"heix", b"heim", b"heis"];

// TODO: Refer to the information on the website https://www.ftyps.com to add
// other less common MP4 brands.
const MP4_BRAND_NAMES: &[&str] = &[
    "3g2a", "3g2b", "3g2c", "3ge6", "3ge7", "3gg6", "3gp4", "3gp5", "3gp6", "3gs7", "avc1", "mp41",
    "mp42", "iso2", "isom", "vfj1",
];

const QT_BRAND_NAMES: &[&str] = &["qt  ", "mqt "];

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum Mime {
    Image(MimeImage),
    Video(MimeVideo),
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum MimeImage {
    Jpeg,
    Heic,
    Heif,
    Tiff,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum MimeVideo {
    QuickTime,
    Mp4,
    Webm,
    Matroska,
    _3gpp,
}

impl TryFrom<&[u8]> for Mime {
    type Error = crate::Error;
    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        let mime = if let Ok(x) = parse_bmff_mime(input) {
            x
        } else if let Ok(x) = get_ebml_doc_type(input) {
            if x == "webm" {
                Mime::Video(MimeVideo::Webm)
            } else {
                Mime::Video(MimeVideo::Matroska)
            }
        } else if TiffHeader::parse(input).is_ok() {
            Mime::Image(MimeImage::Tiff)
        } else if check_jpeg(input).is_ok() {
            Mime::Image(MimeImage::Jpeg)
        } else {
            return Err(crate::Error::UnrecognizedFileFormat);
        };

        Ok(mime)
    }
}

#[derive(Debug, Clone)]
pub struct MediaType {
    media_type: MediaKind,
    mime: &'static str,
}

impl MediaType {
    pub fn try_from_reader<R: Read>(reader: R) -> crate::Result<MediaType> {
        const BUF_SIZE: usize = 4096;
        let mut buf = Vec::with_capacity(BUF_SIZE);
        let n = reader.take(BUF_SIZE as u64).read_to_end(buf.as_mut())?;
        if n == 0 {
            Err("file is empty")?;
        }

        Self::try_from_bytes(&buf)
    }

    pub fn try_from_bytes(input: &[u8]) -> crate::Result<MediaType> {
        let (media_type, mime) = if check_jpeg_exif(input).is_ok_and(|x| x.1) {
            (MediaKind::Image, "image/jpeg")
        } else if let Ok(x) = get_bmff_media_type(input) {
            x
        } else if let Ok(x) = get_ebml_doc_type(input) {
            if x == "webm" {
                (MediaKind::Video, "video/webm")
            } else {
                (MediaKind::Video, "video/matroska")
            }
        } else {
            return Err(crate::Error::UnrecognizedFileFormat);
        };

        Ok(MediaType { media_type, mime })
    }

    pub fn is_image(&self) -> bool {
        self.media_type == MediaKind::Image
    }

    pub fn is_track(&self) -> bool {
        !self.is_image()
    }

    pub fn mime(&self) -> &str {
        self.mime
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaKind {
    Image,
    Video,
    #[allow(unused)]
    Audio,
}

/// *Deprecated*: Please use [`MediaType`] instead.
#[deprecated(since = "2.0.0")]
#[allow(unused)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Jpeg,
    /// heic, heif
    Heif,

    // Currently, there is not much difference between QuickTime and MP4 when
    // parsing metadata, and they share the same parsing mechanism.
    //
    // The only difference is that if detected as an MP4 file, the
    // `moov/udta/©xyz` atom is additionally checked and an attempt is made to
    // read GPS information from it, since Android phones store GPS information
    // in that atom.
    /// mov
    QuickTime,
    MP4,

    /// webm, mkv, mka, mk3d
    Ebml,
}

// Parse the input buffer and detect its file type
impl TryFrom<&[u8]> for FileFormat {
    type Error = crate::Error;

    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        if let Ok(ff) = check_bmff(input) {
            Ok(ff)
        } else if get_ebml_doc_type(input).is_ok() {
            Ok(Self::Ebml)
        } else if check_jpeg(input).is_ok() {
            Ok(Self::Jpeg)
        } else {
            Err(crate::Error::UnrecognizedFileFormat)
        }
    }
}

impl FileFormat {
    pub fn try_from_read<T: Read>(reader: T) -> crate::Result<Self> {
        const BUF_SIZE: usize = 4096;
        let mut buf = Vec::with_capacity(BUF_SIZE);
        let n = reader.take(BUF_SIZE as u64).read_to_end(buf.as_mut())?;
        if n == 0 {
            Err("file is empty")?;
        }

        buf.as_slice().try_into()
    }

    pub(crate) fn try_from_load<T: Load>(loader: &mut T) -> Result<Self, ParsedError> {
        loader.load_and_parse(|x| {
            x.try_into()
                .map_err(|_| ParsingError::Failed("unrecognized file format".to_string()))
        })
    }

    pub(crate) fn extract_exif_data<'a>(
        &self,
        input: &'a [u8],
    ) -> IResult<&'a [u8], Option<&'a [u8]>> {
        match self {
            Jpeg => jpeg::extract_exif_data(input),
            Heif => heif::extract_exif_data(input),
            QuickTime => {
                nom::error::context("no exif data in QuickTime file", nom::combinator::fail)(input)
            }
            MP4 => nom::error::context("no exif data in MP4 file", nom::combinator::fail)(input),
            Ebml => nom::error::context("no exif data in EBML file", nom::combinator::fail)(input),
        }
    }
}

use FileFormat::*;
impl Display for FileFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Jpeg => "JPEG".fmt(f),
            Heif => "HEIF/HEIC".fmt(f),
            QuickTime => "QuickTime".fmt(f),
            MP4 => "MP4".fmt(f),
            Ebml => "EBML".fmt(f),
        }
    }
}

fn get_ebml_doc_type(input: &[u8]) -> crate::Result<String> {
    let mut cursor = Cursor::new(input);
    let doc = parse_ebml_doc_type(&mut cursor)?;
    Ok(doc)
}

fn parse_bmff_mime(input: &[u8]) -> crate::Result<Mime> {
    let (ftyp, Some(major_brand)) =
        get_ftyp_and_major_brand(input).map_err(|_| crate::Error::UnrecognizedFileFormat)?
    else {
        if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
            // ftyp is None, mdat box is found, assume it's a MOV file extracted from HEIC
            return Ok(Mime::Video(MimeVideo::QuickTime));
        }

        return Err(crate::Error::UnrecognizedFileFormat);
    };

    // Check if it is a QuickTime file
    if QT_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok(Mime::Video(MimeVideo::QuickTime));
    }

    // Check if it is a HEIF file
    if HEIF_HEIC_BRAND_NAMES.contains(&major_brand) {
        if HEIC_BRAND_NAMES.contains(&major_brand) {
            return Ok(Mime::Image(MimeImage::Heic));
        }
        return Ok(Mime::Image(MimeImage::Heif));
    }

    // Check if it is a MP4 file
    if MP4_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        if major_brand.starts_with(b"3gp") {
            return Ok(Mime::Video(MimeVideo::_3gpp));
        }
        return Ok(Mime::Video(MimeVideo::Mp4));
    }

    // Check compatible brands
    let compatible_brands = ftyp.body_data();

    if QT_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.find_substring(v.as_bytes()).is_some())
    {
        return Ok(Mime::Video(MimeVideo::QuickTime));
    }

    if HEIF_HEIC_BRAND_NAMES
        .iter()
        .any(|x| compatible_brands.find_substring(*x).is_some())
    {
        if HEIC_BRAND_NAMES.contains(&major_brand) {
            return Ok(Mime::Image(MimeImage::Heic));
        }
        return Ok(Mime::Image(MimeImage::Heif));
    }

    if MP4_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.subslice_range(v.as_bytes()).is_some())
    {
        if major_brand.starts_with(b"3gp") {
            return Ok(Mime::Video(MimeVideo::_3gpp));
        }
        return Ok(Mime::Video(MimeVideo::Mp4));
    }

    tracing::error!(
        marjor_brand = major_brand.iter().map(|b| *b as char).collect::<String>(),
        "unknown major brand",
    );

    if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
        // mdat box found, assume it's a mp4 file
        return Ok(Mime::Video(MimeVideo::Mp4));
    }

    Err(crate::Error::UnrecognizedFileFormat)
}

fn get_bmff_media_type(input: &[u8]) -> crate::Result<(MediaKind, &'static str)> {
    let (ftyp, Some(major_brand)) = get_ftyp_and_major_brand(input)? else {
        if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
            // ftyp is None, mdat box is found, assume it's a MOV file extracted from HEIC
            return Ok((MediaKind::Video, "video/quicktime"));
        }

        return Err(crate::Error::UnrecognizedFileFormat);
    };

    // Check if it is a QuickTime file
    if QT_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok((MediaKind::Video, "video/quicktime"));
    }

    // Check if it is a HEIF file
    if HEIF_HEIC_BRAND_NAMES.contains(&major_brand) {
        if HEIC_BRAND_NAMES.contains(&major_brand) {
            return Ok((MediaKind::Image, "image/heic"));
        }
        return Ok((MediaKind::Image, "image/heif"));
    }

    // Check if it is a MP4 file
    if MP4_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        if major_brand.starts_with(b"3gp") {
            return Ok((MediaKind::Video, "video/3gpp"));
        }
        return Ok((MediaKind::Video, "video/mp4"));
    }

    // Check compatible brands
    let compatible_brands = get_compatible_brands(ftyp.body_data())?;

    if QT_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.iter().any(|x| v.as_bytes() == *x))
    {
        return Ok((MediaKind::Video, "video/quicktime"));
    }

    if HEIF_HEIC_BRAND_NAMES
        .iter()
        .any(|x| compatible_brands.contains(x))
    {
        if HEIC_BRAND_NAMES.contains(&major_brand) {
            return Ok((MediaKind::Image, "image/heic"));
        }
        return Ok((MediaKind::Image, "image/heif"));
    }

    if MP4_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.iter().any(|x| v.as_bytes() == *x))
    {
        if major_brand.starts_with(b"3gp") {
            return Ok((MediaKind::Video, "video/3gpp"));
        }
        return Ok((MediaKind::Video, "video/mp4"));
    }

    tracing::error!(
        marjor_brand = major_brand.iter().map(|b| *b as char).collect::<String>(),
        "unknown major brand",
    );

    if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
        // mdat box found, assume it's a mp4 file
        return Ok((MediaKind::Video, "video/mp4"));
    }

    Err(crate::Error::UnrecognizedFileFormat)
}

fn check_bmff(input: &[u8]) -> crate::Result<FileFormat> {
    let (ftyp, Some(major_brand)) = get_ftyp_and_major_brand(input)? else {
        if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
            // ftyp is None, mdat box is found, assume it's a MOV file extracted from HEIC
            return Ok(FileFormat::QuickTime);
        }

        return Err(crate::Error::UnrecognizedFileFormat);
    };

    // Check if it is a QuickTime file
    if QT_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok(FileFormat::QuickTime);
    }

    // Check if it is a HEIF file
    if HEIF_HEIC_BRAND_NAMES.contains(&major_brand) {
        return Ok(FileFormat::Heif);
    }

    // Check if it is a MP4 file
    if MP4_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok(FileFormat::MP4);
    }

    // Check compatible brands
    let compatible_brands = get_compatible_brands(ftyp.body_data())?;

    if QT_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.iter().any(|x| v.as_bytes() == *x))
    {
        return Ok(FileFormat::QuickTime);
    }

    if HEIF_HEIC_BRAND_NAMES
        .iter()
        .any(|x| compatible_brands.contains(x))
    {
        return Ok(FileFormat::Heif);
    }

    if MP4_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.iter().any(|x| v.as_bytes() == *x))
    {
        return Ok(FileFormat::MP4);
    }

    tracing::error!(
        marjor_brand = major_brand.iter().map(|b| *b as char).collect::<String>(),
        "unknown major brand",
    );

    if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
        // find mdat box, assume it's a mp4 file
        return Ok(FileFormat::MP4);
    }

    Err(crate::Error::UnrecognizedFileFormat)
}

pub(crate) fn check_heif(input: &[u8]) -> crate::Result<()> {
    let (ftyp, Some(major_brand)) = get_ftyp_and_major_brand(input)? else {
        return Err("invalid ISOBMFF file; ftyp not found".into());
    };

    if HEIF_HEIC_BRAND_NAMES.contains(&major_brand) {
        Ok(())
    } else {
        // Check compatible brands
        let compatible_brands = get_compatible_brands(ftyp.body_data())?;
        if HEIF_HEIC_BRAND_NAMES
            .iter()
            .any(|x| compatible_brands.contains(x))
        {
            Ok(())
        } else {
            Err(format!("unsupported HEIF/HEIC file; major brand: {major_brand:?}").into())
        }
    }
}

#[tracing::instrument(skip_all)]
fn check_qt_mp4(input: &[u8]) -> crate::Result<FileFormat> {
    let (ftyp, Some(major_brand)) = get_ftyp_and_major_brand(input)? else {
        if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
            // ftyp is None, mdat box is found, assume it's a MOV file extracted from HEIC
            return Ok(FileFormat::QuickTime);
        }

        return Err(crate::Error::UnrecognizedFileFormat);
    };

    // Check if it is a QuickTime file
    if QT_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok(FileFormat::QuickTime);
    }

    // Check if it is a MP4 file
    if MP4_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok(FileFormat::MP4);
    }

    // Check compatible brands
    let compatible_brands = get_compatible_brands(ftyp.body_data())?;

    if QT_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.iter().any(|x| v.as_bytes() == *x))
    {
        return Ok(FileFormat::QuickTime);
    }

    if MP4_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.iter().any(|x| v.as_bytes() == *x))
    {
        return Ok(FileFormat::MP4);
    }

    tracing::error!(
        marjor_brand = major_brand.iter().map(|b| *b as char).collect::<String>(),
        "unknown major brand",
    );

    if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
        // find mdat box, assume it's a mp4 file
        return Ok(FileFormat::MP4);
    }

    Err(crate::Error::UnrecognizedFileFormat)
}

fn get_ftyp_and_major_brand(input: &[u8]) -> crate::Result<(BoxHolder, Option<&[u8]>)> {
    let (_, bbox) = BoxHolder::parse(input).map_err(|e| format!("parse ftyp failed: {e}"))?;

    if bbox.box_type() == "ftyp" {
        if bbox.body_data().len() < 4 {
            return Err(format!(
                "parse ftyp failed; body size should greater than 4, got {}",
                bbox.body_data().len()
            )
            .into());
        }
        let (_, ftyp) = complete::take(4_usize)(bbox.body_data())?;
        Ok((bbox, Some(ftyp)))
    } else if bbox.box_type() == "wide" {
        // MOV files that extracted from HEIC starts with `wide` & `mdat` atoms
        Ok((bbox, None))
    } else {
        Err(format!("parse ftyp failed; first box type is: {}", bbox.box_type()).into())
    }
}

fn get_compatible_brands(body: &[u8]) -> crate::Result<Vec<&[u8]>> {
    let Ok((_, brands)) = many0(complete::take::<usize, &[u8], nom::error::Error<&[u8]>>(
        4_usize,
    ))(body) else {
        return Err("get compatible brands failed".into());
    };
    Ok(brands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;
    use MediaKind::*;

    use crate::testkit::open_sample;

    #[test_case("exif.heic", Image, "image/heic")]
    #[test_case("exif.jpg", Image, "image/jpeg")]
    #[test_case("meta.mp4", Video, "video/mp4")]
    #[test_case("meta.mov", Video, "video/quicktime")]
    #[test_case("embedded-in-heic.mov", Video, "video/quicktime")]
    #[test_case("compatible-brands.mov", Video, "video/quicktime")]
    #[test_case("webm_480.webm", Video, "video/webm")]
    #[test_case("mkv_640x360.mkv", Video, "video/matroska")]
    #[test_case("mka.mka", Video, "video/matroska")]
    #[test_case("3gp_640x360.3gp", Video, "video/3gpp")]
    fn media_type(path: &str, mt: MediaKind, mime: &str) {
        let f = open_sample(path).unwrap();
        let mi = MediaType::try_from_reader(f).unwrap();
        assert_eq!(mi.media_type, mt);
        assert_eq!(mi.mime(), mime);
    }

    #[test_case("exif.heic", Heif)]
    #[test_case("exif.jpg", Jpeg)]
    #[test_case("meta.mov", QuickTime)]
    #[test_case("meta.mp4", MP4)]
    #[test_case("embedded-in-heic.mov", QuickTime)]
    #[test_case("compatible-brands.mov", QuickTime)]
    fn file_format(path: &str, expect: FileFormat) {
        let f = open_sample(path).unwrap();
        let ff = FileFormat::try_from_read(f).unwrap();
        assert_eq!(ff, expect);
    }

    #[test_case("compatible-brands-fail.mov")]
    fn file_format_error(path: &str) {
        let f = open_sample(path).unwrap();
        FileFormat::try_from_read(f).unwrap_err();
    }
}

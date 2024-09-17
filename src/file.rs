use nom::{bytes::complete, multi::many0, FindSubstring};
use std::{
    fmt::Display,
    io::{Cursor, Read},
};

use crate::{
    bbox::{travel_header, BoxHolder},
    ebml::element::parse_ebml_doc_type,
    error::{ParsedError, ParsingError},
    exif::TiffHeader,
    jpeg::check_jpeg,
    loader::Load,
    slice::SubsliceRange,
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

impl Mime {
    pub fn unwrap_image(self) -> MimeImage {
        match self {
            Mime::Image(val) => val,
            Mime::Video(_) => panic!("called `Mime::unwrap_image()` on an `Mime::Video`"),
        }
    }
    pub fn unwrap_video(self) -> MimeVideo {
        match self {
            Mime::Image(_) => panic!("called `Mime::unwrap_video()` on an `Mime::Image`"),
            Mime::Video(val) => val,
        }
    }
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
#[allow(deprecated)]
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

#[allow(deprecated)]
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
}

#[allow(deprecated)]
impl Display for FileFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Jpeg => "JPEG".fmt(f),
            Self::Heif => "HEIF/HEIC".fmt(f),
            Self::QuickTime => "QuickTime".fmt(f),
            Self::MP4 => "MP4".fmt(f),
            Self::Ebml => "EBML".fmt(f),
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

#[allow(deprecated)]
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

#[allow(deprecated)]
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

#[allow(deprecated)]
#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use super::*;
    use test_case::test_case;
    use Mime::*;
    use MimeImage::*;
    use MimeVideo::*;

    use crate::testkit::{open_sample, read_sample};

    #[test_case("exif.heic", Image(Heic))]
    #[test_case("exif.jpg", Image(Jpeg))]
    #[test_case("meta.mp4", Video(Mp4))]
    #[test_case("meta.mov", Video(QuickTime))]
    #[test_case("embedded-in-heic.mov", Video(QuickTime))]
    #[test_case("compatible-brands.mov", Video(QuickTime))]
    #[test_case("webm_480.webm", Video(Webm))]
    #[test_case("mkv_640x360.mkv", Video(Matroska))]
    #[test_case("mka.mka", Video(Matroska))]
    #[test_case("3gp_640x360.3gp", Video(_3gpp))]
    fn mime(path: &str, mime: Mime) {
        let data = read_sample(path).unwrap();
        let m: Mime = data.deref().try_into().unwrap();
        assert_eq!(m, mime);
    }

    #[test_case("exif.heic", FileFormat::Heif)]
    #[test_case("exif.jpg", FileFormat::Jpeg)]
    #[test_case("meta.mov", FileFormat::QuickTime)]
    #[test_case("meta.mp4", FileFormat::MP4)]
    #[test_case("embedded-in-heic.mov", FileFormat::QuickTime)]
    #[test_case("compatible-brands.mov", FileFormat::QuickTime)]
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

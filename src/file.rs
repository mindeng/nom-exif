use nom::{bytes::complete, multi::many0, IResult};
use std::{
    fmt::Display,
    io::{Cursor, Read},
};
use FileFormat::*;

use crate::{
    bbox::{travel_header, BoxHolder},
    ebml::element::parse_ebml_doc_type,
    heif,
    jpeg::{self, check_jpeg},
};

const HEIF_BRAND_NAMES: &[&[u8]] = &[
    b"heic", // the usual HEIF images
    b"heix", // 10bit images, or anything that uses h265 with range extension
    b"hevc", // 'hevx': brands for image sequences
    b"heim", // multiview
    b"heis", // scalable
    b"hevm", // multiview sequence
    b"hevs", // scalable sequence
    b"mif1", b"MiHE", b"miaf", b"MiHB", // HEIC file's compatible brands
];

// TODO: Refer to the information on the website https://www.ftyps.com to add
// other less common MP4 brands.
const MP4_BRAND_NAMES: &[&str] = &[
    "3g2a", "3g2b", "3g2c", "3ge6", "3ge7", "3gg6", "3gp4", "3gp5", "3gp6", "3gs7", "avc1", "mp41",
    "mp42", "iso2", "isom", "vfj1",
];

const QT_BRAND_NAMES: &[&str] = &["qt  ", "mqt "];

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
        if check_jpeg(input).is_ok() {
            Ok(Self::Jpeg)
        } else if let Ok(ff) = check_bmff(input) {
            Ok(ff)
        } else if check_ebml(input).is_ok() {
            Ok(Self::Ebml)
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

    pub(crate) fn check(&self, input: &[u8]) -> crate::Result<()> {
        match self {
            Jpeg => check_jpeg(input),
            Heif => check_heif(input),
            QuickTime => {
                let ff = check_qt_mp4(input)?;
                if ff == *self {
                    Ok(())
                } else {
                    Err("not a QuickTime file".into())
                }
            }
            MP4 => {
                let ff = check_qt_mp4(input)?;
                if ff == *self {
                    Ok(())
                } else {
                    Err("not a MP4 file".into())
                }
            }
            Ebml => check_ebml(input),
        }
    }
}

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

pub(crate) fn check_ebml(input: &[u8]) -> crate::Result<()> {
    let mut cursor = Cursor::new(input);
    parse_ebml_doc_type(&mut cursor)?;
    Ok(())
}

pub(crate) fn check_bmff(input: &[u8]) -> crate::Result<FileFormat> {
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
    if HEIF_BRAND_NAMES.contains(&major_brand) {
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

    if HEIF_BRAND_NAMES
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

    if HEIF_BRAND_NAMES.contains(&major_brand) {
        Ok(())
    } else {
        // Check compatible brands
        let compatible_brands = get_compatible_brands(ftyp.body_data())?;
        if HEIF_BRAND_NAMES
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
pub(crate) fn check_qt_mp4(input: &[u8]) -> crate::Result<FileFormat> {
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

    use crate::testkit::open_sample;

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

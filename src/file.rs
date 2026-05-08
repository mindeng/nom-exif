use nom::{bytes::complete, FindSubstring};
use std::io::Cursor;

use crate::{
    bbox::{travel_header, BoxHolder},
    ebml::element::parse_ebml_doc_type,
    exif::TiffHeader,
    jpeg::check_jpeg,
    raf::RafInfo,
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
    "mp42", "iso2", "isom", "vfj1", "XAVC",
];

const QT_BRAND_NAMES: &[&str] = &["qt  ", "mqt "];

const CR3_BRAND_NAMES: &[&str] = &["crx "];

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
    Raf, // Fujifilm RAW, image/x-fuji-raf
    Cr3, // Canon RAW, image/x-canon-cr3
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
        } else if RafInfo::check(input).is_ok() {
            Mime::Image(MimeImage::Raf)
        } else {
            return Err(crate::Error::UnrecognizedFileFormat);
        };

        Ok(mime)
    }
}

fn get_ebml_doc_type(input: &[u8]) -> crate::Result<String> {
    let mut cursor = Cursor::new(input);
    let doc = parse_ebml_doc_type(&mut cursor)?;
    Ok(doc)
}

#[tracing::instrument(skip_all)]
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

    tracing::debug!(?ftyp);

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

    // Check if it is a CR3 file
    if CR3_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok(Mime::Image(MimeImage::Cr3));
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
        .any(|v| compatible_brands.subslice_in_range(v.as_bytes()).is_some())
    {
        if major_brand.starts_with(b"3gp") {
            return Ok(Mime::Video(MimeVideo::_3gpp));
        }
        return Ok(Mime::Video(MimeVideo::Mp4));
    }

    tracing::warn!(
        marjor_brand = major_brand.iter().map(|b| *b as char).collect::<String>(),
        "unknown major brand",
    );

    if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
        // mdat box found, assume it's a mp4 file
        return Ok(Mime::Video(MimeVideo::Mp4));
    }

    Err(crate::Error::UnrecognizedFileFormat)
}

fn get_ftyp_and_major_brand(input: &[u8]) -> crate::Result<(BoxHolder<'_>, Option<&[u8]>)> {
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

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use super::*;
    use test_case::test_case;
    use Mime::*;
    use MimeImage::*;
    use MimeVideo::*;

    use crate::testkit::read_sample;

    #[test_case("exif.heic", Image(Heic))]
    #[test_case("exif.jpg", Image(Jpeg))]
    #[test_case("fujifilm_x_t1_01.raf.meta", Image(Raf))]
    #[test_case("meta.mp4", Video(Mp4))]
    #[test_case("meta.mov", Video(QuickTime))]
    #[test_case("embedded-in-heic.mov", Video(QuickTime))]
    #[test_case("compatible-brands.mov", Video(QuickTime))]
    #[test_case("webm_480.webm", Video(Webm))]
    #[test_case("mkv_640x360.mkv", Video(Matroska))]
    #[test_case("mka.mka", Video(Matroska))]
    #[test_case("3gp_640x360.3gp", Video(_3gpp))]
    #[test_case("sony-a7-xavc.MP4", Video(Mp4))]
    fn mime(path: &str, mime: Mime) {
        let data = read_sample(path).unwrap();
        let m: Mime = data.deref().try_into().unwrap();
        assert_eq!(m, mime);
    }
}

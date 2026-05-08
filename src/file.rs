use nom::{bytes::complete, FindSubstring};
use std::io::Cursor;

use crate::{
    bbox::{travel_header, BoxHolder},
    ebml::element::parse_ebml_doc_type,
    error::MalformedKind,
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
pub(crate) enum MediaMime {
    Image(MediaMimeImage),
    Track(MediaMimeTrack),
}

impl MediaMime {
    pub fn unwrap_image(self) -> MediaMimeImage {
        match self {
            MediaMime::Image(val) => val,
            MediaMime::Track(_) => panic!("called `MediaMime::unwrap_image()` on a `MediaMime::Track`"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum MediaMimeImage {
    Jpeg,
    Heic,
    Heif,
    Tiff,
    Raf,
    Cr3,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum MediaMimeTrack {
    QuickTime,
    Mp4,
    Webm,
    Matroska,
    _3gpp,
}

impl TryFrom<&[u8]> for MediaMime {
    type Error = crate::Error;
    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        let mime = if let Ok(x) = parse_bmff_mime(input) {
            x
        } else if let Ok(x) = get_ebml_doc_type(input) {
            if x == "webm" {
                MediaMime::Track(MediaMimeTrack::Webm)
            } else {
                MediaMime::Track(MediaMimeTrack::Matroska)
            }
        } else if TiffHeader::parse(input).is_ok() {
            MediaMime::Image(MediaMimeImage::Tiff)
        } else if check_jpeg(input).is_ok() {
            MediaMime::Image(MediaMimeImage::Jpeg)
        } else if RafInfo::check(input).is_ok() {
            MediaMime::Image(MediaMimeImage::Raf)
        } else {
            return Err(crate::Error::UnsupportedFormat);
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
fn parse_bmff_mime(input: &[u8]) -> crate::Result<MediaMime> {
    let (ftyp, Some(major_brand)) =
        get_ftyp_and_major_brand(input).map_err(|_| crate::Error::UnsupportedFormat)?
    else {
        if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
            // ftyp is None, mdat box is found, assume it's a MOV file extracted from HEIC
            return Ok(MediaMime::Track(MediaMimeTrack::QuickTime));
        }

        return Err(crate::Error::UnsupportedFormat);
    };

    tracing::debug!(?ftyp);

    // Check if it is a QuickTime file
    if QT_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok(MediaMime::Track(MediaMimeTrack::QuickTime));
    }

    // Check if it is a HEIF file
    if HEIF_HEIC_BRAND_NAMES.contains(&major_brand) {
        if HEIC_BRAND_NAMES.contains(&major_brand) {
            return Ok(MediaMime::Image(MediaMimeImage::Heic));
        }
        return Ok(MediaMime::Image(MediaMimeImage::Heif));
    }

    // Check if it is a MP4 file
    if MP4_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        if major_brand.starts_with(b"3gp") {
            return Ok(MediaMime::Track(MediaMimeTrack::_3gpp));
        }
        return Ok(MediaMime::Track(MediaMimeTrack::Mp4));
    }

    // Check if it is a CR3 file
    if CR3_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok(MediaMime::Image(MediaMimeImage::Cr3));
    }

    // Check compatible brands
    let compatible_brands = ftyp.body_data();

    if QT_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.find_substring(v.as_bytes()).is_some())
    {
        return Ok(MediaMime::Track(MediaMimeTrack::QuickTime));
    }

    if HEIF_HEIC_BRAND_NAMES
        .iter()
        .any(|x| compatible_brands.find_substring(*x).is_some())
    {
        if HEIC_BRAND_NAMES.contains(&major_brand) {
            return Ok(MediaMime::Image(MediaMimeImage::Heic));
        }
        return Ok(MediaMime::Image(MediaMimeImage::Heif));
    }

    if MP4_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.subslice_in_range(v.as_bytes()).is_some())
    {
        if major_brand.starts_with(b"3gp") {
            return Ok(MediaMime::Track(MediaMimeTrack::_3gpp));
        }
        return Ok(MediaMime::Track(MediaMimeTrack::Mp4));
    }

    tracing::warn!(
        marjor_brand = major_brand.iter().map(|b| *b as char).collect::<String>(),
        "unknown major brand",
    );

    if travel_header(input, |header, _| header.box_type != "mdat").is_ok() {
        // mdat box found, assume it's a mp4 file
        return Ok(MediaMime::Track(MediaMimeTrack::Mp4));
    }

    Err(crate::Error::UnsupportedFormat)
}

fn get_ftyp_and_major_brand(input: &[u8]) -> crate::Result<(BoxHolder<'_>, Option<&[u8]>)> {
    let (_, bbox) = BoxHolder::parse(input).map_err(|e| crate::Error::Malformed {
        kind: MalformedKind::IsoBmffBox,
        message: format!("parse ftyp failed: {e}"),
    })?;

    if bbox.box_type() == "ftyp" {
        if bbox.body_data().len() < 4 {
            return Err(crate::Error::Malformed {
                kind: MalformedKind::IsoBmffBox,
                message: format!(
                    "parse ftyp failed; body size should greater than 4, got {}",
                    bbox.body_data().len()
                ),
            });
        }
        let (_, ftyp) = complete::take(4_usize)(bbox.body_data())?;
        Ok((bbox, Some(ftyp)))
    } else if bbox.box_type() == "wide" {
        // MOV files that extracted from HEIC starts with `wide` & `mdat` atoms
        Ok((bbox, None))
    } else {
        Err(crate::Error::Malformed {
            kind: MalformedKind::IsoBmffBox,
            message: format!("parse ftyp failed; first box type is: {}", bbox.box_type()),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use super::*;
    use test_case::test_case;
    use MediaMime::*;
    use MediaMimeImage::*;
    use MediaMimeTrack::*;

    use crate::testkit::read_sample;

    #[test_case("exif.heic", Image(Heic))]
    #[test_case("exif.jpg", Image(Jpeg))]
    #[test_case("fujifilm_x_t1_01.raf.meta", Image(Raf))]
    #[test_case("meta.mp4", Track(Mp4))]
    #[test_case("meta.mov", Track(QuickTime))]
    #[test_case("embedded-in-heic.mov", Track(QuickTime))]
    #[test_case("compatible-brands.mov", Track(QuickTime))]
    #[test_case("webm_480.webm", Track(Webm))]
    #[test_case("mkv_640x360.mkv", Track(Matroska))]
    #[test_case("mka.mka", Track(Matroska))]
    #[test_case("3gp_640x360.3gp", Track(_3gpp))]
    #[test_case("sony-a7-xavc.MP4", Track(Mp4))]
    fn mime(path: &str, mime: MediaMime) {
        let data = read_sample(path).unwrap();
        let m: MediaMime = data.deref().try_into().unwrap();
        assert_eq!(m, mime);
    }
}

#[cfg(test)]
mod v3_tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn unrecognized_returns_unsupported_format() {
        let bogus = b"\x00\x00\x00\x00not a real file";
        let res: Result<MediaMime, Error> = bogus.as_slice().try_into();
        assert!(matches!(res, Err(Error::UnsupportedFormat)));
    }
}

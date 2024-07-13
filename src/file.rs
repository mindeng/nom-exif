use std::fmt::Display;

const HEIF_FTYPS: &[&[u8]] = &[
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
#[derive(Debug, PartialEq, Eq)]
pub enum FileType {
    Jpeg,
    Heif,

    // Currently, there is not much difference between QuickTime and MP4 when
    // parsing metadata, and they share the same parsing mechanism.
    //
    // The only difference is that if detected as an MP4 file, the
    // `moov/udta/©xyz` atom is additionally checked and an attempt is made to
    // read GPS information from it, since Android phones store GPS information
    // in that atom.
    QuickTime,
    MP4,
}

use nom::{bytes::complete, multi::many0};
use FileType::*;

use crate::bbox::BoxHolder;

// Parse the input buffer and detect its file type
impl TryFrom<&[u8]> for FileType {
    type Error = crate::Error;

    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        // check qt & mp4 first, because a embedded QT file may not have a ftyp
        // box
        if let Ok(ft) = check_qt_mp4(input) {
            Ok(ft)
        } else {
            check_heif(input)
        }
    }
}

impl Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Jpeg => "JPEG".fmt(f),
            Heif => "HEIF/HEIC".fmt(f),
            QuickTime => "QuickTime".fmt(f),
            MP4 => "MP4".fmt(f),
        }
    }
}

pub fn check_heif(input: &[u8]) -> crate::Result<FileType> {
    let (ftyp, Some(major_brand)) = get_ftyp_and_major_brand(input)? else {
        return Err("invalid ISOBMFF file; ftyp not found".into());
    };

    if HEIF_FTYPS.contains(&major_brand) {
        Ok(FileType::Heif)
    } else {
        // Check compatible brands
        let compatible_brands = get_compatible_brands(ftyp.body_data())?;
        if HEIF_FTYPS.iter().any(|x| compatible_brands.contains(x)) {
            Ok(FileType::Heif)
        } else {
            Err(format!("unsupported HEIF/HEIC file; major brand: {major_brand:?}").into())
        }
    }
}

pub fn check_qt_mp4(input: &[u8]) -> crate::Result<FileType> {
    let (ftyp, Some(major_brand)) = get_ftyp_and_major_brand(input)? else {
        // ftyp is None, assume it's a MOV file extracted from HEIC
        return Ok(FileType::QuickTime);
    };

    // Check if it is a QuickTime file
    if QT_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok(FileType::QuickTime);
    }

    // Check if it is a MP4 file
    if MP4_BRAND_NAMES.iter().any(|v| v.as_bytes() == major_brand) {
        return Ok(FileType::MP4);
    }

    // Check compatible brands
    let compatible_brands = get_compatible_brands(ftyp.body_data())?;

    if QT_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.iter().any(|x| v.as_bytes() == *x))
    {
        return Ok(FileType::QuickTime);
    }

    if MP4_BRAND_NAMES
        .iter()
        .any(|v| compatible_brands.iter().any(|x| v.as_bytes() == *x))
    {
        return Ok(FileType::MP4);
    }

    Err(format!(
        "unsupported video file; major brand: '{}'",
        major_brand.iter().map(|b| *b as char).collect::<String>()
    )
    .into())
}

pub fn get_ftyp_and_major_brand(input: &[u8]) -> crate::Result<(BoxHolder, Option<&[u8]>)> {
    let (_, bbox) = BoxHolder::parse(input).map_err(|_| "parse ftyp failed")?;

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

pub fn get_compatible_brands(body: &[u8]) -> crate::Result<Vec<&[u8]>> {
    let Ok((_, brands)) = many0(complete::take::<usize, &[u8], nom::error::Error<&[u8]>>(
        4_usize,
    ))(body) else {
        return Err("get compatible brands failed".into());
    };
    Ok(brands)
}

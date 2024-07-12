use std::cmp;
use std::io::{Read, Seek};

use nom::combinator::fail;
use nom::Needed;
use nom::{number::complete::be_u32, IResult};

use crate::bbox::get_ftyp;
use crate::exif::{parse_exif, Exif};
use crate::{
    bbox::{travel_while, BoxHolder, MetaBox, ParseBox},
    exif::check_exif_header,
};

/// Analyze the byte stream in the `reader` as a HEIF/HEIC file, attempting to
/// extract Exif data it may contain.
///
/// Please note that the parsing routine itself provides a buffer, so the
/// `reader` may not need to be wrapped with `BufRead`.
///
/// # Usage
///
/// ```rust
/// use nom_exif::*;
/// use nom_exif::ExifTag::*;
///
/// use std::fs::File;
/// use std::path::Path;
///
/// let f = File::open(Path::new("./testdata/exif.heic")).unwrap();
/// let exif = parse_heif_exif(f).unwrap().unwrap();
///
/// assert_eq!(exif.get_value(&Make).unwrap().unwrap().to_string(), "Apple");
///
/// assert_eq!(
///     exif.get_values(&[DateTimeOriginal, CreateDate, ModifyDate])
///         .into_iter()
///         .map(|x| (x.0.to_string(), x.1.to_string()))
///         .collect::<Vec<_>>(),
///     [
///         ("DateTimeOriginal(0x9003)", "2022-07-22T21:26:32+08:00"),
///         ("CreateDate(0x9004)", "2022-07-22T21:26:32+08:00"),
///         ("ModifyDate(0x0132)", "2022-07-22T21:26:32+08:00")
///     ]
///     .into_iter()
///     .map(|x| (x.0.to_string(), x.1.to_string()))
///     .collect::<Vec<_>>()
/// );
/// ```
pub fn parse_heif_exif<R: Read + Seek>(mut reader: R) -> crate::Result<Option<Exif>> {
    const INIT_BUF_SIZE: usize = 4096;
    const GROW_BUF_SIZE: usize = 1024;

    let mut buf = Vec::with_capacity(INIT_BUF_SIZE);
    let mut to_read = INIT_BUF_SIZE;

    let n = reader
        .by_ref()
        .take(to_read as u64)
        .read_to_end(buf.as_mut())?;
    if n == 0 {
        Err("file is empty")?;
    }

    let Some(ftyp) = get_ftyp(&buf).map_err(|e| format!("unsupported HEIF/HEIC file; {}", e))?
    else {
        return Err("unsupported HEIF/HEIC file; ftyp not found".into());
    };
    if !HEIF_FTYPS.contains(&ftyp) {
        Err(format!("unsupported HEIF/HEIC file; ftyp: {ftyp:?}"))?;
    }

    let (_, exif_data) = loop {
        to_read = match extract_exif_data(&buf) {
            Ok((remain, bbox)) => break (remain, bbox),
            Err(nom::Err::Incomplete(needed)) => match needed {
                Needed::Unknown => GROW_BUF_SIZE,
                Needed::Size(need) => need.get(),
            },
            Err(e) => Err(e)?,
        };

        // println!("to_read: {to_read}");
        assert!(to_read > 0);

        let to_read = cmp::max(GROW_BUF_SIZE, to_read);
        buf.reserve(to_read);

        let n = reader
            .by_ref()
            .take(to_read as u64)
            .read_to_end(buf.as_mut())?;
        if n == 0 {
            Err("meta box not found")?;
        }
    };

    exif_data.map(parse_exif).transpose()
}

const HEIF_FTYPS: [&[u8]; 7] = [
    b"heic", // the usual HEIF images
    b"heix", // 10bit images, or anything that uses h265 with range extension
    b"hevc", // 'hevx': brands for image sequences
    b"heim", // multiview
    b"heis", // scalable
    b"hevm", // multiview sequence
    b"hevs", // scalable sequence
];

/// Extract Exif TIFF data from the bytes of a HEIF/HEIC file.
fn extract_exif_data(input: &[u8]) -> IResult<&[u8], Option<&[u8]>> {
    let remain = input;
    let (remain, bbox) = BoxHolder::parse(remain)?;
    if bbox.box_type() != "ftyp" {
        return fail(input);
    }

    let (_, bbox) = travel_while(remain, |b| b.header.box_type != "meta")?;
    let (_, bbox) = MetaBox::parse_box(bbox.data)?;
    let (out_remain, data) = bbox.exif_data(input)?;

    if let Some(data) = data {
        let (remain, _) = be_u32(data)?;
        if check_exif_header(remain) {
            Ok((out_remain, Some(&remain[6..]))) // Safe-slice
        } else {
            Ok((out_remain, None))
        }
    } else {
        Ok((out_remain, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use test_case::test_case;

    #[test_case("exif.heic")]
    fn heif(path: &str) {
        let reader = open_sample(path).unwrap();
        let exif = parse_heif_exif(reader).unwrap().unwrap();

        assert_eq!(
            sorted_exif_entries(&exif),
            [
                "ApertureValue(0x9202) » 14447/10653 (1.3561)",
                "BrightnessValue(0x9203) » 97777/16376 (5.9707)",
                "ColorSpace(0xa001) » 65535",
                "CreateDate(0x9004) » 2022-07-22T21:26:32+08:00",
                "DateTimeOriginal(0x9003) » 2022-07-22T21:26:32+08:00",
                "ExifImageHeight(0xa003) » 3024",
                "ExifImageWidth(0xa002) » 4032",
                "ExposureBiasValue(0x9204) » 0/1 (0.0000)",
                "ExposureMode(0xa402) » 0",
                "ExposureProgram(0x8822) » 2",
                "ExposureTime(0x829a) » 1/171 (0.0058)",
                "FNumber(0x829d) » 8/5 (1.6000)",
                "Flash(0x9209) » 16",
                "FocalLength(0x920a) » 21/5 (4.2000)",
                "FocalLengthIn35mmFilm(0xa405) » 26",
                "GPSAltitude(0x0006) » 572946/359 (1595.9499)",
                "GPSAltitudeRef(0x0005) » 0",
                "GPSDestBearing(0x0018) » 443187/1672 (265.0640)",
                "GPSDestBearingRef(0x0017) » T",
                "GPSImgDirection(0x0011) » 443187/1672 (265.0640)",
                "GPSImgDirectionRef(0x0010) » T",
                "GPSLatitude(0x0002) » 43/1 (43.0000)",
                "GPSLatitudeRef(0x0001) » N",
                "GPSLongitude(0x0004) » 84/1 (84.0000)",
                "GPSLongitudeRef(0x0003) » E",
                "GPSSpeed(0x000d) » 0/1 (0.0000)",
                "GPSSpeedRef(0x000c) » K",
                "HostComputer(0x013c) » iPhone 12 Pro",
                "ISOSpeedRatings(0x8827) » 32",
                "LensMake(0xa433) » Apple",
                "LensModel(0xa434) » iPhone 12 Pro back triple camera 4.2mm f/1.6",
                "LensSpecification(0xa432) » 807365/524263 (1.5400)",
                "Make(0x010f) » Apple",
                "MeteringMode(0x9207) » 5",
                "Model(0x0110) » iPhone 12 Pro",
                "ModifyDate(0x0132) » 2022-07-22T21:26:32+08:00",
                "OffsetTime(0x9010) » +08:00",
                "OffsetTimeOriginal(0x9011) » +08:00",
                "Orientation(0x0112) » 6",
                "ResolutionUnit(0x0128) » 2",
                "SensingMethod(0xa217) » 2",
                "ShutterSpeedValue(0x9201) » 139397/18789 (7.4191)",
                "Software(0x0131) » 15.5",
                "SubjectArea(0x9214) » 2009",
                "WhiteBalanceMode(0xa403) » 0",
                "XResolution(0x011a) » 72/1 (72.0000)",
                "YResolution(0x011b) » 72/1 (72.0000)"
            ]
        );
    }

    #[test_case("ramdisk.img")]
    fn invalid_heic(path: &str) {
        let reader = open_sample(path).unwrap();
        parse_heif_exif(reader).expect_err("should be ParseFailed error");
    }

    #[test_case("no-exif.heic", 0x24-10)]
    #[test_case("exif.heic", 0xa3a-10)]
    fn heic_exif_data(path: &str, exif_size: usize) {
        let buf = read_sample(path).unwrap();
        let (_, exif) = extract_exif_data(&buf[..]).unwrap();

        if exif_size == 0 {
            assert!(exif.is_none());
        } else {
            assert_eq!(exif.unwrap().len(), exif_size);
        }
    }
}

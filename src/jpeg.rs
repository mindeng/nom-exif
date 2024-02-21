use std::{
    cmp,
    io::{Read, Seek},
};

use nom::{bytes::streaming, number, sequence::tuple, IResult, Needed};

use crate::{
    error::convert_parse_error,
    exif::{check_exif_header, parse_exif, Exif},
};

/// Analyze the byte stream in the `reader` as a JPEG file, attempting to
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
/// let f = File::open(Path::new("./testdata/exif.jpg")).unwrap();
/// let exif = parse_jpeg_exif(f).unwrap().unwrap();
///
/// assert_eq!(exif.get_value(&Make).unwrap().unwrap().to_string(), "vivo");
///
/// assert_eq!(
///     exif.get_values(&[DateTimeOriginal, CreateDate, ModifyDate])
///         .into_iter()
///         .map(|x| (x.0.to_string(), x.1.to_string()))
///         .collect::<Vec<_>>(),
///     [
///         ("DateTimeOriginal(0x9003)", "2023-07-09T20:36:33+08:00"),
///         ("CreateDate(0x9004)", "2023-07-09T20:36:33+08:00"),
///         ("ModifyDate(0x0132)", "2023-07-09T20:36:33+08:00")
///     ]
///     .into_iter()
///     .map(|x| (x.0.to_string(), x.1.to_string()))
///     .collect::<Vec<_>>()
/// );
/// ```
pub fn parse_jpeg_exif<R: Read>(mut reader: R) -> crate::Result<Option<Exif>> {
    const INIT_BUF_SIZE: usize = 4096;
    const GROW_BUF_SIZE: usize = 4096;

    let mut buf = Vec::with_capacity(INIT_BUF_SIZE);

    let n = reader
        .by_ref()
        .take(INIT_BUF_SIZE as u64)
        .read_to_end(buf.as_mut())?;
    if n == 0 {
        Err("invalid JPEG file; file is empty")?;
    }

    check_jpeg(&buf)?;

    let exif_data = loop {
        let to_read = match extract_exif_data(&buf[..]) {
            Ok((_, res)) => break res,
            Err(nom::Err::Incomplete(needed)) => match needed {
                Needed::Unknown => GROW_BUF_SIZE,
                Needed::Size(n) => cmp::max(n.get(), GROW_BUF_SIZE),
            },
            Err(err) => return Err(convert_parse_error(err, "parse JPEG exif failed")),
        };
        buf.reserve(to_read);
        let n = reader
            .by_ref()
            .take(to_read as u64)
            .read_to_end(buf.as_mut())?;
        if n == 0 {
            Err("parse JPEG exif failed; not enough bytes")?;
        }
    };

    exif_data
        .and_then(|exif_data| Some(parse_exif(exif_data)))
        .transpose()
}

/// Extract Exif TIFF data from the bytes of a JPEG file.
fn extract_exif_data<'a>(input: &'a [u8]) -> IResult<&'a [u8], Option<&'a [u8]>> {
    let (remain, segment) = find_exif_segment(input)?;
    let data = segment.and_then(|segment| {
        if segment.payload_len() <= 6 {
            None
        } else {
            Some(&segment.payload[6..])
        }
    });

    Ok((remain, data))
}

struct Segment<'a> {
    marker_code: u8,
    payload: &'a [u8],
}

impl<'a> Segment<'a> {
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }
}

fn find_exif_segment<'a>(input: &'a [u8]) -> IResult<&'a [u8], Option<Segment<'a>>> {
    let (remain, segment) = travel_until(input, |s| {
        (s.marker_code == MarkerCode::APP1.code() && check_exif_header(s.payload))
            || s.marker_code == MarkerCode::SOS.code() // searching stop at SOS
    })?;

    if segment.marker_code != MarkerCode::SOS.code() {
        Ok((remain, Some(segment)))
    } else {
        Ok((remain, None))
    }
}

fn travel_until<'a, F>(input: &'a [u8], mut predicate: F) -> IResult<&'a [u8], Segment<'a>>
where
    F: FnMut(&Segment<'a>) -> bool,
{
    let mut remain = input;

    loop {
        let (rem, (_, code)) = tuple((streaming::tag([0xFF]), number::streaming::u8))(remain)?;
        let (rem, segment) = parse_segment(code, rem)?;
        // Sanity check
        assert!(rem.len() < remain.len());
        remain = rem;
        // println!("got segment {:x}", segment.marker_code);

        if predicate(&segment) {
            break Ok((remain, segment));
        }
    }
}

/// len of input should be >= 2
pub fn check_jpeg(input: &[u8]) -> crate::Result<()> {
    // check SOI marker [0XFF, 0XD8]
    let (_, (_, code)) = tuple((streaming::tag([0xFF]), number::complete::u8))(input)?;

    // SOI has no payload
    if code != MarkerCode::SOI.code() {
        Err("invalid JPEG file; SOI marker not found".into())
    } else {
        Ok(())
    }
}

fn parse_segment<'a>(marker_code: u8, input: &'a [u8]) -> IResult<&'a [u8], Segment<'a>> {
    let remain = input;

    // SOI has no payload
    if marker_code == MarkerCode::SOI.code() {
        Ok((
            remain,
            Segment {
                marker_code,
                payload: b"",
            },
        ))
    } else {
        let (remain, size) = number::streaming::be_u16(remain)?;
        // size contains the two bytes of `size` itself
        let (remain, data) = streaming::take(size - 2)(remain)?;
        Ok((
            remain,
            Segment {
                marker_code,
                payload: data,
            },
        ))
    }
}

/// Read all image data after the first SOS marker & before EOI marker.
///
/// The returned data might include several other SOS markers if the image is a
/// progressive JPEG.
fn read_image_data<T: Read + Seek>(mut reader: T) -> crate::Result<Vec<u8>> {
    let mut header = [0u8; 2];
    loop {
        reader.read_exact(&mut header)?;
        let (tag, marker) = (header[0], header[1]);
        if tag != 0xFF {
            return Err("".into());
        }

        if marker == MarkerCode::SOI.code() {
            // SOI has no body
            continue;
        }
        if marker == MarkerCode::EOI.code() {
            return Err(crate::Error::NotFound);
        }

        if marker == MarkerCode::SOS.code() {
            // found it
            let mut data = Vec::new();
            reader.read_to_end(&mut data)?;

            // remove tail data
            loop {
                let Some(tail) = data.pop() else {
                    // empty
                    break;
                };
                if tail == MarkerCode::EOI.code() {
                    if let Some(tail) = data.pop() {
                        if tail == 0xFF {
                            // EOI marker has been popped
                            break;
                        }
                    }
                }
            }
            return Ok(data);
        } else {
            // skip other markers
            reader.read_exact(&mut header)?;
            let len = u16::from_be_bytes([header[0], header[1]]);
            reader.seek(std::io::SeekFrom::Current(len as i64 - 2))?;
        }
    }
}

/// A marker code is a byte following 0xFF that indicates the kind of marker.
enum MarkerCode {
    // Start of Image
    SOI = 0xD8,

    // APP1 marker
    APP1 = 0xE1,

    // Start of Scan
    SOS = 0xDA,

    // End of Image
    EOI = 0xD9,
}

impl MarkerCode {
    fn code(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exif::ExifTag::*;
    use crate::testkit::*;
    use test_case::test_case;

    #[test_case("exif.jpg")]
    fn jpeg(path: &str) {
        let f = open_sample(path).unwrap();
        let exif = parse_jpeg_exif(f).unwrap().unwrap();

        assert_eq!(
            sorted_exif_entries(&exif),
            [
                "ApertureValue(0x9202) » 161/100 (1.6100)",
                "BrightnessValue(0x9203) » 70/100 (0.7000)",
                "ColorSpace(0xa001) » 1",
                "CreateDate(0x9004) » 2023-07-09T20:36:33+08:00",
                "DateTimeOriginal(0x9003) » 2023-07-09T20:36:33+08:00",
                "DigitalZoomRatio(0xa404) » 1/1 (1.0000)",
                "ExifImageHeight(0xa003) » 4096",
                "ExifImageWidth(0xa002) » 3072",
                "ExposureBiasValue(0x9204) » 0/1 (0.0000)",
                "ExposureMode(0xa402) » 0",
                "ExposureProgram(0x8822) » 2",
                "ExposureTime(0x829a) » 9997/1000000 (0.0100)",
                "FNumber(0x829d) » 175/100 (1.7500)",
                "Flash(0x9209) » 16",
                "FocalLength(0x920a) » 8670/1000 (8.6700)",
                "FocalLengthIn35mmFilm(0xa405) » 23",
                "GPSAltitude(0x0006) » 0/1 (0.0000)",
                "GPSAltitudeRef(0x0005) » 0",
                "GPSDateStamp(0x001d) » 2023:07:09",
                "GPSLatitude(0x0002) » 22/1 (22.0000)",
                "GPSLatitudeRef(0x0001) » N",
                "GPSLongitude(0x0004) » 114/1 (114.0000)",
                "GPSLongitudeRef(0x0003) » E",
                "GPSTimeStamp(0x0007) » 12/1 (12.0000)",
                "ISOSpeedRatings(0x8827) » 454",
                "ImageHeight(0x0101) » 4096",
                "ImageWidth(0x0100) » 3072",
                "LightSource(0x9208) » 21",
                "Make(0x010f) » vivo",
                "MaxApertureValue(0x9205) » 161/100 (1.6100)",
                "MeteringMode(0x9207) » 1",
                "Model(0x0110) » vivo X90 Pro+",
                "ModifyDate(0x0132) » 2023-07-09T20:36:33+08:00",
                "OffsetTime(0x9010) » +08:00",
                "OffsetTimeOriginal(0x9011) » +08:00",
                "ResolutionUnit(0x0128) » 2",
                "SceneCaptureType(0xa406) » 0",
                "SensingMethod(0xa217) » 2",
                "SensitivityType(0x8830) » 2",
                "ShutterSpeedValue(0x9201) » 6644/1000 (6.6440)",
                "WhiteBalanceMode(0xa403) » 0",
                "XResolution(0x011a) » 72/1 (72.0000)",
                "YResolution(0x011b) » 72/1 (72.0000)"
            ]
        );

        assert_eq!(exif.get_value(&Make).unwrap().unwrap().to_string(), "vivo");

        assert_eq!(
            exif.get_values(&[DateTimeOriginal, CreateDate, ModifyDate])
                .into_iter()
                .map(|x| (x.0.to_string(), x.1.to_string()))
                .collect::<Vec<_>>(),
            [
                ("DateTimeOriginal(0x9003)", "2023-07-09T20:36:33+08:00"),
                ("CreateDate(0x9004)", "2023-07-09T20:36:33+08:00"),
                ("ModifyDate(0x0132)", "2023-07-09T20:36:33+08:00")
            ]
            .into_iter()
            .map(|x| (x.0.to_string(), x.1.to_string()))
            .collect::<Vec<_>>()
        );

        let mut entries = exif
            .get_values(&[ImageWidth, ImageHeight])
            .into_iter()
            .map(|x| (x.0.to_string(), x.1.to_string()))
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(
            entries,
            [
                ("ImageHeight(0x0101)", "4096"),
                ("ImageWidth(0x0100)", "3072")
            ]
            .into_iter()
            .map(|x| (x.0.to_string(), x.1.to_string()))
            .collect::<Vec<_>>()
        );
    }

    #[test_case("no-exif.jpg", 0)]
    #[test_case("exif.jpg", 0x4569-2)]
    fn jpeg_find_exif(path: &str, exif_size: usize) {
        let buf = read_sample(path).unwrap();
        let (_, segment) = find_exif_segment(&buf[..]).unwrap();

        if exif_size == 0 {
            assert!(segment.is_none());
        } else {
            assert_eq!(segment.unwrap().payload_len(), exif_size);
        }
    }

    #[test_case("no-exif.jpg", 0)]
    #[test_case("exif.jpg", 0x4569-8)]
    fn jpeg_exif_data(path: &str, exif_size: usize) {
        let buf = read_sample(path).unwrap();
        let (_, exif) = extract_exif_data(&buf[..]).unwrap();

        if exif_size == 0 {
            assert!(exif.is_none());
        } else {
            assert_eq!(exif.unwrap().len(), exif_size);
        }
    }

    #[test_case("no-exif.jpg", 4089704, 0x000c0301, 0xb3b3e43f)]
    #[test_case("exif.jpg", 3564768, 0x000c0301, 0x84a297a9)]
    fn jpeg_image_data(path: &str, len: usize, start: u32, end: u32) {
        let f = open_sample(path).unwrap();
        let data = read_image_data(f).unwrap();
        assert_eq!(data.len(), len);
        assert_eq!(u32::from_be_bytes(data[..4].try_into().unwrap()), start);
        assert_eq!(
            u32::from_be_bytes(data[data.len() - 4..].try_into().unwrap()),
            end
        );
    }
}

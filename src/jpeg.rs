use std::io::{Read, Seek};

use nom::{bytes::streaming, combinator::fail, number, IResult, Parser};

use crate::error::MalformedKind;
use crate::exif::check_exif_header;

/// XMP APP1 segment payload prefix (29 bytes including the trailing NUL).
const XMP_NS_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\x00";

/// Find the byte length of a Pixel/Google Motion Photo trailer in a JPEG
/// buffer.
///
/// Walks JPEG markers up to SOS, looking for an APP1 XMP segment that
/// contains `GCamera:MotionPhoto="1"` together with a
/// `GCamera:MotionPhotoOffset="N"` attribute. Returns `Some(N)` when both
/// signals are present — `N` is the trailer length in bytes (i.e. the MP4
/// starts at `file_size - N`). Returns `None` for plain JPEGs.
///
/// Conservative on error: any malformed-marker or short-buffer condition
/// yields `None` so callers can treat absence of a Motion Photo as the
/// default. This is content detection, not validation.
pub(crate) fn find_motion_photo_offset(input: &[u8]) -> Option<u64> {
    let mut remain = input;
    loop {
        let parsed: IResult<&[u8], (&[u8], u8)> =
            (streaming::tag(&[0xFF_u8][..]), number::streaming::u8).parse(remain);
        let (rem, (_, code)) = parsed.ok()?;
        let (rem, segment) = parse_segment(code, rem).ok()?;
        remain = rem;

        if segment.marker_code == MarkerCode::Sos.code() {
            return None;
        }
        if segment.marker_code == MarkerCode::APP1.code()
            && segment.payload.starts_with(XMP_NS_HEADER)
        {
            let xmp = &segment.payload[XMP_NS_HEADER.len()..];
            if let Some(offset) = parse_motion_photo_offset(xmp) {
                return Some(offset);
            }
        }
    }
}

/// Parse a Motion Photo offset value from an XMP packet body.
///
/// Looks for `GCamera:MotionPhoto="1"` and `GCamera:MotionPhotoOffset="N"`
/// (or the older `GCamera:MicroVideo="1"` / `GCamera:MicroVideoOffset="N"`
/// pair used on pre-2018 Pixels). Returns `None` if either signal is
/// missing or the offset value is unparseable.
fn parse_motion_photo_offset(xmp: &[u8]) -> Option<u64> {
    let has_motion_photo = contains_attr_eq(xmp, b"GCamera:MotionPhoto", b"1")
        || contains_attr_eq(xmp, b"GCamera:MicroVideo", b"1");
    if !has_motion_photo {
        return None;
    }
    extract_attr_value(xmp, b"GCamera:MotionPhotoOffset")
        .or_else(|| extract_attr_value(xmp, b"GCamera:MicroVideoOffset"))
        .and_then(|s| std::str::from_utf8(s).ok()?.parse::<u64>().ok())
}

/// True if `xmp` contains an attribute `name="value"`.
fn contains_attr_eq(xmp: &[u8], name: &[u8], value: &[u8]) -> bool {
    let needle = [name, b"=\"", value, b"\""].concat();
    memchr_subslice(xmp, &needle).is_some()
}

/// Extract the quoted value of an attribute named `name`, if present.
fn extract_attr_value<'a>(xmp: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let prefix = [name, b"=\""].concat();
    let start = memchr_subslice(xmp, &prefix)? + prefix.len();
    let end = xmp[start..].iter().position(|&b| b == b'"')?;
    Some(&xmp[start..start + end])
}

fn memchr_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// Extract Exif TIFF data from the bytes of a JPEG file.
pub(crate) fn extract_exif_data(input: &[u8]) -> IResult<&[u8], Option<&[u8]>> {
    let (remain, segment) = find_exif_segment(input)?;
    let data = segment.and_then(|segment| {
        if segment.payload_len() <= 6 {
            None
        } else {
            Some(&segment.payload[6..]) // Safe-slice
        }
    });
    Ok((remain, data))
}

struct Segment<'a> {
    marker_code: u8,
    payload: &'a [u8],
}

impl Segment<'_> {
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }
}

fn find_exif_segment(input: &[u8]) -> IResult<&[u8], Option<Segment<'_>>> {
    let mut remain = input;

    let (remain, segment) = loop {
        let (rem, (_, code)) =
            (streaming::tag(&[0xFF_u8][..]), number::streaming::u8).parse(remain)?;
        let (rem, segment) = parse_segment(code, rem)?;
        // Sanity check
        assert!(rem.len() < remain.len());
        remain = rem;
        tracing::debug!(
            marker = format!("0x{:04x}", segment.marker_code),
            size = format!("0x{:04x}", segment.payload.len()),
            "got segment"
        );

        let s = &segment;
        if (s.marker_code == MarkerCode::APP1.code() && check_exif_header(s.payload)?)
            || s.marker_code == MarkerCode::Sos.code()
        // searching stop at SOS
        {
            break (remain, segment);
        }
    };

    if segment.marker_code != MarkerCode::Sos.code() {
        Ok((remain, Some(segment)))
    } else {
        Ok((remain, None))
    }
}

pub fn check_jpeg(input: &[u8]) -> crate::Result<()> {
    // check soi marker [0xff, 0xd8]
    let (_, (_, code)) = (
        nom::bytes::complete::tag(&[0xFF_u8][..]),
        number::complete::u8,
    )
        .parse(input)?;

    // SOI has no payload
    if code != MarkerCode::Soi.code() {
        return Err(crate::Error::Malformed {
            kind: crate::error::MalformedKind::JpegSegment,
            message: "SOI marker not found".into(),
        });
    }

    // check next marker [0xff, *]
    let (_, (_, _)) = (
        nom::bytes::complete::tag(&[0xFF_u8][..]),
        number::complete::u8,
    )
        .parse(input)?;
    Ok(())
}

fn parse_segment(marker_code: u8, input: &[u8]) -> IResult<&[u8], Segment<'_>> {
    let remain = input;

    // SOI has no payload
    if marker_code == MarkerCode::Soi.code() {
        Ok((
            remain,
            Segment {
                marker_code,
                payload: b"",
            },
        ))
    } else {
        let (remain, size) = number::streaming::be_u16(remain)?;
        if size < 2 {
            return fail().parse(remain);
        }
        // size contains the two bytes of `size` itself
        let (remain, data) = streaming::take(size - 2).parse(remain)?;
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
#[allow(dead_code)]
fn read_image_data<T: Read + Seek>(mut reader: T) -> crate::Result<Vec<u8>> {
    let mut header = [0u8; 2];
    loop {
        reader.read_exact(&mut header)?;
        let (tag, marker) = (header[0], header[1]);
        if tag != 0xFF {
            return Err(crate::Error::Malformed {
                kind: MalformedKind::JpegSegment,
                message: "expected 0xFF marker prefix".to_string(),
            });
        }

        if marker == MarkerCode::Soi.code() {
            // SOI has no body
            continue;
        }
        if marker == MarkerCode::Eoi.code() {
            return Err(crate::Error::ExifNotFound);
        }

        if marker == MarkerCode::Sos.code() {
            // found it
            let mut data = Vec::new();
            reader.read_to_end(&mut data)?;

            // remove tail data
            loop {
                let Some(tail) = data.pop() else {
                    // empty
                    break;
                };
                if tail == MarkerCode::Eoi.code() {
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
    Soi = 0xD8,

    // APP1 marker
    APP1 = 0xE1,

    // Start of Scan
    Sos = 0xDA,

    // End of Image
    Eoi = 0xD9,
}

impl MarkerCode {
    fn code(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use test_case::test_case;

    #[test_case("exif.jpg", true)]
    #[test_case("broken.jpg", true)]
    #[test_case("no-exif.jpg", false)]
    fn test_check_jpeg(path: &str, has_exif: bool) {
        let data = read_sample(path).unwrap();
        check_jpeg(&data).unwrap();
        let (_, data) = extract_exif_data(&data).unwrap();
        if has_exif {
            data.unwrap();
        }
    }

    #[test_case("no-exif.jpg", 0)]
    #[test_case("exif.jpg", 0x4569-2)]
    fn jpeg_find_exif(path: &str, exif_size: usize) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

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
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

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
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let f = open_sample(path).unwrap();
        let data = read_image_data(f).unwrap();
        assert_eq!(data.len(), len);
        assert_eq!(u32::from_be_bytes(data[..4].try_into().unwrap()), start); // Safe-slice in test_case
        assert_eq!(
            u32::from_be_bytes(data[data.len() - 4..].try_into().unwrap()), // Safe-slice in test_case
            end
        );
    }
}

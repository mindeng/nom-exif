use std::io::{Read, Seek};

use nom::{bytes::streaming, combinator::fail, number, IResult, Parser};

use crate::error::MalformedKind;
use crate::exif::check_exif_header;

/// XMP APP1 segment payload prefix (29 bytes including the trailing NUL).
const XMP_NS_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\x00";

/// Outcome of scanning a JPEG buffer for a Pixel/Google Motion Photo
/// signal.
pub(crate) enum MotionPhotoScan {
    /// Saw `GCamera:MotionPhoto="1"` (or `GCamera:MicroVideo="1"`) with a
    /// trailer-length attribute. The MP4 trailer starts at
    /// `file_size - N`.
    Found(u64),
    /// Walked far enough to be sure no Motion Photo signal is present
    /// (e.g. reached the SOS marker, or hit a malformed segment).
    NotPresent,
    /// The buffer ended mid-walk before reaching SOS — caller should
    /// load more bytes and retry from the start.
    NeedMoreBytes,
}

/// Scan a JPEG buffer for a Pixel/Google Motion Photo signal.
///
/// Walks JPEG markers up to SOS, looking for an APP1 XMP segment that
/// contains `GCamera:MotionPhoto="1"` together with a
/// `GCamera:MotionPhotoOffset="N"` attribute (or the older
/// `MicroVideo` / `MicroVideoOffset` pair). Returns
/// [`MotionPhotoScan::Found(N)`] when both are present, where `N` is the
/// trailer length in bytes.
///
/// The 3-state result lets callers distinguish "definitively no
/// trailer" (NotPresent — the scanner reached SOS or a malformed marker)
/// from "ran out of buffer" (NeedMoreBytes — the answer is unknown until
/// more bytes are loaded).
pub(crate) fn scan_motion_photo(input: &[u8]) -> MotionPhotoScan {
    let mut remain = input;
    loop {
        let parsed: IResult<&[u8], (&[u8], u8)> =
            (streaming::tag(&[0xFF_u8][..]), number::streaming::u8).parse(remain);
        let (rem, (_, code)) = match parsed {
            Ok(t) => t,
            Err(nom::Err::Incomplete(_)) => return MotionPhotoScan::NeedMoreBytes,
            Err(_) => return MotionPhotoScan::NotPresent,
        };
        let (rem, segment) = match parse_segment(code, rem) {
            Ok(t) => t,
            Err(nom::Err::Incomplete(_)) => return MotionPhotoScan::NeedMoreBytes,
            Err(_) => return MotionPhotoScan::NotPresent,
        };
        remain = rem;

        if segment.marker_code == MarkerCode::Sos.code() {
            return MotionPhotoScan::NotPresent;
        }
        if segment.marker_code == MarkerCode::APP1.code()
            && segment.payload.starts_with(XMP_NS_HEADER)
        {
            let xmp = &segment.payload[XMP_NS_HEADER.len()..];
            if let Some(offset) = parse_motion_photo_offset(xmp) {
                return MotionPhotoScan::Found(offset);
            }
            // Some files may carry XMP without a Motion Photo signal, or
            // split it across multiple APP1 segments — keep walking.
        }
    }
}

/// Convenience wrapper: returns the trailer offset if (and only if) the
/// scan finishes with a definite answer of "found". Used by
/// `parse_track`'s polymorphic JPEG path which always sees the full
/// file in memory and therefore can't get `NeedMoreBytes`.
pub(crate) fn find_motion_photo_offset(input: &[u8]) -> Option<u64> {
    match scan_motion_photo(input) {
        MotionPhotoScan::Found(n) => Some(n),
        MotionPhotoScan::NotPresent | MotionPhotoScan::NeedMoreBytes => None,
    }
}

/// Parse a Motion Photo trailer length from an XMP packet body.
///
/// Pixel cameras have used three layouts over time; this function tries
/// them in order:
///
/// 1. **Adobe XMP Container directory** (modern Pixel, including Ultra
///    HDR Motion Photos). The XMP carries a `<Container:Directory>`
///    with an item whose `Item:Mime="video/mp4"` and
///    `Item:Semantic="MotionPhoto"`; trailer length is the sum of
///    `Item:Length` (+ optional `Item:Padding`) for that item plus all
///    items after it in directory order.
/// 2. **`GCamera:MotionPhotoOffset`** attribute (older Pixel
///    `PXL_*.MP.jpg`).
/// 3. **`GCamera:MicroVideoOffset`** attribute (pre-2018 Pixel
///    `MVIMG_*.jpg`).
///
/// Requires `GCamera:MotionPhoto="1"` or `GCamera:MicroVideo="1"` as a
/// gate so plain Ultra HDR JPEGs (Container directory present, no
/// motion photo) don't false-positive.
fn parse_motion_photo_offset(xmp: &[u8]) -> Option<u64> {
    let has_motion_photo = contains_attr_eq(xmp, b"GCamera:MotionPhoto", b"1")
        || contains_attr_eq(xmp, b"GCamera:MicroVideo", b"1");
    if !has_motion_photo {
        return None;
    }
    container_motion_photo_offset(xmp)
        .or_else(|| {
            extract_attr_value(xmp, b"GCamera:MotionPhotoOffset")
                .or_else(|| extract_attr_value(xmp, b"GCamera:MicroVideoOffset"))
                .and_then(|s| std::str::from_utf8(s).ok()?.parse::<u64>().ok())
        })
}

/// Walk `<Container:Directory>` and return the trailer length of the
/// `MotionPhoto` item: its `Item:Length` plus optional `Item:Padding`,
/// plus the same for every item that follows it in directory order.
///
/// Returns `None` if no Container directory is present or if no item
/// matches the MotionPhoto signature.
fn container_motion_photo_offset(xmp: &[u8]) -> Option<u64> {
    let dir_start = memchr_subslice(xmp, b"<Container:Directory")?;
    let dir_end_rel = memchr_subslice(&xmp[dir_start..], b"</Container:Directory>")?;
    let dir = &xmp[dir_start..dir_start + dir_end_rel];

    // Collect every <Container:Item ...> tag in directory order.
    let mut items: Vec<&[u8]> = Vec::new();
    let mut pos = 0;
    while let Some(idx) = memchr_subslice(&dir[pos..], b"<Container:Item") {
        let abs = pos + idx;
        let tag_end_rel = dir[abs..].iter().position(|&b| b == b'>')?;
        items.push(&dir[abs..abs + tag_end_rel]);
        pos = abs + tag_end_rel;
    }

    let mp_idx = items.iter().position(|tag| {
        extract_attr_value(tag, b"Item:Semantic") == Some(b"MotionPhoto")
            || extract_attr_value(tag, b"Item:Mime") == Some(b"video/mp4")
    })?;

    // Each item's `Item:Padding` is the gap between this item and the
    // next one in the container; the last item's padding is therefore
    // not part of the file (the Galaxy-1 sample has Length=4299299
    // Padding=80 as the last item, and 80 zero-bytes after the MP4
    // would push the offset past EOF). Sum all Lengths in [mp_idx..],
    // plus Padding only for the non-final entries.
    let mut total: u64 = 0;
    let last = items.len() - 1;
    for (i, tag) in items.iter().enumerate().skip(mp_idx) {
        let length = extract_attr_value(tag, b"Item:Length")
            .and_then(|s| std::str::from_utf8(s).ok()?.parse::<u64>().ok())?;
        total = total.checked_add(length)?;
        if i != last {
            let padding = extract_attr_value(tag, b"Item:Padding")
                .and_then(|s| std::str::from_utf8(s).ok()?.parse::<u64>().ok())
                .unwrap_or(0);
            total = total.checked_add(padding)?;
        }
    }
    Some(total)
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

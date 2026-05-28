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
    container_motion_photo_offset(xmp).or_else(|| {
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
        extract_attr_value(tag, b"Item:Semantic") == Some(&b"MotionPhoto"[..])
            || extract_attr_value(tag, b"Item:Mime") == Some(&b"video/mp4"[..])
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
    haystack.windows(needle.len()).position(|w| w == needle)
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
    let (remain, (_, code)) = (
        nom::bytes::complete::tag(&[0xFF_u8][..]),
        number::complete::u8,
    )
        .parse(input)
        .map_err(|e| {
            crate::error::nom_err_to_malformed(e, crate::error::MalformedKind::JpegSegment)
        })?;

    // SOI has no payload
    if code != MarkerCode::Soi.code() {
        return Err(crate::Error::Malformed {
            kind: crate::error::MalformedKind::JpegSegment,
            message: "SOI marker not found".into(),
        });
    }

    // check next marker [0xff, *]
    (
        nom::bytes::complete::tag(&[0xFF_u8][..]),
        number::complete::u8,
    )
        .parse(remain)
        .map_err(|e| {
            crate::error::nom_err_to_malformed(e, crate::error::MalformedKind::JpegSegment)
        })?;
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
            while let Some(tail) = data.pop() {
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

    #[test]
    fn memchr_subslice_empty_needle_returns_none() {
        assert_eq!(memchr_subslice(b"hello", b""), None);
    }

    #[test]
    fn memchr_subslice_needle_longer_than_haystack() {
        assert_eq!(memchr_subslice(b"ab", b"abcdef"), None);
    }

    #[test]
    fn memchr_subslice_no_match() {
        assert_eq!(memchr_subslice(b"hello", b"xyz"), None);
    }

    #[test]
    fn memchr_subslice_finds_first_match() {
        assert_eq!(memchr_subslice(b"hello world hello", b"hello"), Some(0));
        assert_eq!(memchr_subslice(b"xx hello world", b"hello"), Some(3));
    }

    #[test]
    fn extract_attr_value_not_found() {
        assert_eq!(extract_attr_value(b"key=\"val\"", b"Missing"), None);
    }

    #[test]
    fn extract_attr_value_unclosed_quote() {
        assert_eq!(extract_attr_value(b"key=\"val", b"key"), None);
    }

    #[test]
    fn extract_attr_value_found() {
        assert_eq!(
            extract_attr_value(b"tag key=\"hello\" rest", b"key"),
            Some(&b"hello"[..])
        );
    }

    #[test]
    fn contains_attr_eq_true() {
        assert!(contains_attr_eq(
            b"GCamera:MotionPhoto=\"1\"",
            b"GCamera:MotionPhoto",
            b"1"
        ));
    }

    #[test]
    fn contains_attr_eq_false() {
        assert!(!contains_attr_eq(
            b"GCamera:MotionPhoto=\"0\"",
            b"GCamera:MotionPhoto",
            b"1"
        ));
    }

    #[test]
    fn contains_attr_eq_missing() {
        assert!(!contains_attr_eq(b"", b"GCamera:MotionPhoto", b"1"));
    }

    #[test]
    fn parse_motion_photo_offset_no_gate_returns_none() {
        let xmp = b"SomeOther=\"1\" GCamera:MotionPhotoOffset=\"1234\"";
        assert_eq!(parse_motion_photo_offset(xmp), None);
    }

    #[test]
    fn parse_motion_photo_offset_micro_video_gate_and_offset() {
        let xmp = b"GCamera:MicroVideo=\"1\" GCamera:MicroVideoOffset=\"5678\"";
        assert_eq!(parse_motion_photo_offset(xmp), Some(5678));
    }

    #[test]
    fn parse_motion_photo_offset_fallback_motion_photo_offset() {
        let xmp = b"GCamera:MotionPhoto=\"1\" GCamera:MotionPhotoOffset=\"9999\"";
        assert_eq!(parse_motion_photo_offset(xmp), Some(9999));
    }

    #[test]
    fn parse_motion_photo_offset_offset_not_a_number() {
        let xmp = b"GCamera:MotionPhoto=\"1\" GCamera:MotionPhotoOffset=\"not-a-number\"";
        assert_eq!(parse_motion_photo_offset(xmp), None);
    }

    #[test]
    fn container_motion_photo_offset_single_item_semantic() {
        let xmp = concat!(
            "GCamera:MotionPhoto=\"1\"\n",
            "<Container:Directory>\n",
            "  <Container:Item Item:Semantic=\"MotionPhoto\" Item:Length=\"100\"/>\n",
            "</Container:Directory>"
        );
        assert_eq!(parse_motion_photo_offset(xmp.as_bytes()), Some(100));
    }

    #[test]
    fn container_motion_photo_offset_single_item_mime() {
        let xmp = concat!(
            "GCamera:MotionPhoto=\"1\"\n",
            "<Container:Directory>\n",
            "  <Container:Item Item:Mime=\"video/mp4\" Item:Length=\"500\"/>\n",
            "</Container:Directory>"
        );
        assert_eq!(parse_motion_photo_offset(xmp.as_bytes()), Some(500));
    }

    #[test]
    fn container_motion_photo_offset_multiple_items_with_padding() {
        let xmp = concat!(
            "GCamera:MotionPhoto=\"1\"\n",
            "<Container:Directory>\n",
            "  <Container:Item Item:Semantic=\"MotionPhoto\" Item:Length=\"100\" Item:Padding=\"50\"/>\n",
            "  <Container:Item Item:Length=\"200\" Item:Padding=\"75\"/>\n",
            "  <Container:Item Item:Length=\"300\" Item:Padding=\"999\"/>\n",
            "</Container:Directory>"
        );
        assert_eq!(parse_motion_photo_offset(xmp.as_bytes()), Some(725));
    }

    #[test]
    fn container_motion_photo_offset_micro_video_gate() {
        let xmp = concat!(
            "GCamera:MicroVideo=\"1\"\n",
            "<Container:Directory>\n",
            "  <Container:Item Item:Semantic=\"MotionPhoto\" Item:Length=\"42\"/>\n",
            "</Container:Directory>"
        );
        assert_eq!(parse_motion_photo_offset(xmp.as_bytes()), Some(42));
    }

    #[test]
    fn container_motion_photo_offset_no_container_returns_none() {
        let xmp = b"GCamera:MotionPhoto=\"1\" no container directory";
        assert_eq!(parse_motion_photo_offset(xmp), None);
    }

    #[test]
    fn container_motion_photo_offset_no_motion_photo_item_returns_none() {
        let xmp = concat!(
            "GCamera:MotionPhoto=\"1\"\n",
            "<Container:Directory>\n",
            "  <Container:Item Item:Semantic=\"StillImage\" Item:Length=\"100\"/>\n",
            "</Container:Directory>"
        );
        assert_eq!(parse_motion_photo_offset(xmp.as_bytes()), None);
    }

    #[test]
    fn scan_motion_photo_truncated_returns_need_more() {
        assert!(matches!(
            scan_motion_photo(&[0xFF]),
            MotionPhotoScan::NeedMoreBytes
        ));
    }

    #[test]
    fn scan_motion_photo_empty_buffer_returns_need_more() {
        assert!(matches!(
            scan_motion_photo(b""),
            MotionPhotoScan::NeedMoreBytes
        ));
    }

    #[test]
    fn scan_motion_photo_truncated_segment_returns_need_more() {
        let buf = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x01, 0x00];
        assert!(matches!(
            scan_motion_photo(&buf),
            MotionPhotoScan::NeedMoreBytes
        ));
    }

    #[test]
    fn scan_motion_photo_malformed_returns_not_present() {
        assert!(matches!(
            scan_motion_photo(&[0x00, 0x00, 0x00, 0x00]),
            MotionPhotoScan::NotPresent
        ));
    }

    #[test]
    fn scan_motion_photo_sos_returns_not_present() {
        let buf = vec![0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02];
        assert!(matches!(
            scan_motion_photo(&buf),
            MotionPhotoScan::NotPresent
        ));
    }

    #[test]
    fn scan_motion_photo_app1_without_xmp_header_not_found() {
        let exif_header = b"Exif\x00\x00";
        let mut buf = vec![0xFF, 0xD8];
        let payload_len = exif_header.len() + 4;
        buf.extend_from_slice(&[0xFF, 0xE1]);
        buf.extend_from_slice(&(payload_len as u16 + 2).to_be_bytes());
        buf.extend_from_slice(exif_header);
        buf.extend_from_slice(b"dummy");
        buf.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
        assert!(matches!(
            scan_motion_photo(&buf),
            MotionPhotoScan::NotPresent
        ));
    }

    #[test]
    fn scan_motion_photo_finds_offset() {
        let xmp_payload = b"http://ns.adobe.com/xap/1.0/\x00GCamera:MotionPhoto=\"1\" GCamera:MotionPhotoOffset=\"777\"";
        let mut buf = vec![0xFF, 0xD8];
        buf.extend_from_slice(&[0xFF, 0xE1]);
        buf.extend_from_slice(&(xmp_payload.len() as u16 + 2).to_be_bytes());
        buf.extend_from_slice(xmp_payload);
        buf.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
        assert!(matches!(
            scan_motion_photo(&buf),
            MotionPhotoScan::Found(777)
        ));
    }

    #[test]
    fn find_motion_photo_offset_found() {
        let xmp = b"http://ns.adobe.com/xap/1.0/\x00GCamera:MotionPhoto=\"1\" GCamera:MotionPhotoOffset=\"888\"";
        let mut buf = vec![0xFF, 0xD8];
        buf.extend_from_slice(&[0xFF, 0xE1]);
        buf.extend_from_slice(&(xmp.len() as u16 + 2).to_be_bytes());
        buf.extend_from_slice(xmp);
        buf.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
        assert_eq!(find_motion_photo_offset(&buf), Some(888));
    }

    #[test]
    fn find_motion_photo_offset_not_present() {
        assert_eq!(find_motion_photo_offset(b""), None);
    }

    #[test]
    fn parse_segment_size_too_small() {
        let result = parse_segment(0xE1, &[0x00, 0x01]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_segment_soi_returns_empty_payload() {
        let (_, segment) = parse_segment(0xD8, b"dummy").unwrap();
        assert_eq!(segment.payload_len(), 0);
    }

    #[test]
    fn extract_exif_data_payload_len_exactly_six_returns_none() {
        let exif_header = b"Exif\x00\x00";
        let mut buf = vec![0xFF, 0xD8];
        buf.extend_from_slice(&[0xFF, 0xE1]);
        buf.extend_from_slice(&(exif_header.len() as u16 + 2).to_be_bytes());
        buf.extend_from_slice(exif_header);
        buf.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
        let (_, data) = extract_exif_data(&buf).unwrap();
        assert!(data.is_none());
    }

    #[test]
    fn check_jpeg_empty_input() {
        assert!(check_jpeg(b"").is_err());
    }

    #[test]
    fn check_jpeg_not_ff_first_byte() {
        assert!(check_jpeg(b"\x00\x00").is_err());
    }

    #[test]
    fn check_jpeg_not_soi() {
        assert!(check_jpeg(&[0xFF, 0xD9]).is_err());
    }

    #[test]
    fn check_jpeg_soi_followed_by_non_ff() {
        assert!(check_jpeg(&[0xFF, 0xD8, 0x00]).is_err());
    }

    #[test]
    fn scan_motion_photo_parsed_segment_error_returns_not_present() {
        let buf = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x00];
        assert!(matches!(
            scan_motion_photo(&buf),
            MotionPhotoScan::NotPresent
        ));
    }
}

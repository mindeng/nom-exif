use crate::{exif::read_exif, file::FileFormat};
use std::io::{Read, Seek};

use nom::{bytes::streaming, combinator::fail, number, sequence::tuple, IResult};

use crate::exif::{check_exif_header, input_to_exif, Exif};

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
#[inline]
pub fn parse_jpeg_exif<R: Read>(reader: R) -> crate::Result<Option<Exif>> {
    read_exif(reader, Some(FileFormat::Jpeg))?
        .map(input_to_exif)
        .transpose()
}

/// Extract Exif TIFF data from the bytes of a JPEG file.
pub fn extract_exif_data(input: &[u8]) -> IResult<&[u8], Option<&[u8]>> {
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
    pub const fn payload_len(&self) -> usize {
        self.payload.len()
    }
}

fn find_exif_segment(input: &[u8]) -> IResult<&[u8], Option<Segment<'_>>> {
    let (remain, segment) = travel_until(input, |s| {
        (s.marker_code == MarkerCode::APP1.code() && check_exif_header(s.payload))
            || s.marker_code == MarkerCode::Sos.code() // searching stop at SOS
    })?;

    if segment.marker_code != MarkerCode::Sos.code() {
        Ok((remain, Some(segment)))
    } else {
        Ok((remain, None))
    }
}

#[tracing::instrument(skip_all)]
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
        tracing::debug!(?segment.marker_code, "Got segment.");

        if predicate(&segment) {
            break Ok((remain, segment));
        }
    }
}

/// len of input should be >= 2
pub fn check_jpeg(input: &[u8]) -> crate::Result<()> {
    assert!(input.len() >= 2);

    // check SOI marker [0XFF, 0XD8]
    let (_, (_, code)) = tuple((nom::bytes::complete::tag([0xFF]), number::complete::u8))(input)?;

    // SOI has no payload
    if code != MarkerCode::Soi.code() {
        Err("invalid JPEG file; SOI marker not found".into())
    } else {
        Ok(())
    }
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
            return fail(remain);
        }
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
#[allow(dead_code)]
fn read_image_data<T: Read + Seek>(mut reader: T) -> crate::Result<Vec<u8>> {
    let mut header = [0u8; 2];
    loop {
        reader.read_exact(&mut header)?;
        let (tag, marker) = (header[0], header[1]);
        if tag != 0xFF {
            return Err("".into());
        }

        if marker == MarkerCode::Soi.code() {
            // SOI has no body
            continue;
        }
        if marker == MarkerCode::Eoi.code() {
            return Err("exif not found".into());
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
    const fn code(self) -> u8 {
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
    #[allow(deprecated)]
    fn jpeg(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let f = open_sample(path).unwrap();
        let exif = parse_jpeg_exif(f).unwrap().unwrap();

        // TODO
        // assert_eq!(
        //     sorted_exif_entries(&exif).join("\n"),

        // );

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
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let (_, segment) = find_exif_segment(&buf).unwrap();

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
        let (_, exif) = extract_exif_data(&buf).unwrap();

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
    fn broken_jpg() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let f = open_sample("broken.jpg").unwrap();
        parse_jpeg_exif(f).unwrap();
    }
}

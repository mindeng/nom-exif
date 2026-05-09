use std::fmt::Debug;

use nom::{
    branch::alt, bytes::streaming::tag, combinator, number::Endianness, IResult, Needed, Parser,
};

use crate::{EntryValue, ExifIter, ExifTag, GPSInfo, ParsedExifEntry};

use super::ifd::ParsedImageFileDirectory;

/// Represents parsed Exif information, can be converted from an [`ExifIter`]
/// like this: `let exif: Exif = iter.into()`.
#[derive(Clone, Debug, PartialEq)]
pub struct Exif {
    ifds: Vec<ParsedImageFileDirectory>,
    gps_info: Option<GPSInfo>,
}

impl Exif {
    fn new(gps_info: Option<GPSInfo>) -> Exif {
        Exif {
            ifds: Vec::new(),
            gps_info,
        }
    }

    /// Get entry value for the specified `tag` in ifd0 (the main image).
    ///
    /// *Note*:
    ///
    /// - The parsing error related to this tag won't be reported by this
    ///   method. Either this entry is not parsed successfully, or the tag does
    ///   not exist in the input data, this method will return None.
    ///
    /// - If you want to handle parsing error, please consider to use
    ///   [`ExifIter`].
    ///
    /// - If you have any custom defined tag which does not exist in
    ///   [`ExifTag`], you can always get the entry value by a raw tag code,
    ///   see [`Self::get_by_ifd_tag_code`].
    ///
    ///   ## Example
    ///
    ///   ```rust
    ///   use nom_exif::*;
    ///
    ///   fn main() -> Result<()> {
    ///       let mut parser = MediaParser::new();
    ///       
    ///       let ms = MediaSource::open("./testdata/exif.jpg")?;
    ///       assert_eq!(ms.kind(), MediaKind::Image);
    ///       let iter = parser.parse_exif(ms)?;
    ///       let exif: Exif = iter.into();
    ///
    ///       assert_eq!(exif.get(ExifTag::Model).unwrap(), &"vivo X90 Pro+".into());
    ///       Ok(())
    ///   }
    pub fn get(&self, tag: ExifTag) -> Option<&EntryValue> {
        self.get_by_ifd_tag_code(0, tag.code())
    }

    /// Get entry value for the specified `tag` in the specified `ifd`.
    ///
    /// `ifd` value range:
    /// - 0: ifd0 (the main image)
    /// - 1: ifd1 (thumbnail image)
    ///
    /// *Note*:
    ///
    /// - The parsing error related to this tag won't be reported by this
    ///   method. Either this entry is not parsed successfully, or the tag does
    ///   not exist in the input data, this method will return None.
    ///
    /// - If you want to handle parsing error, please consider to use
    ///   [`ExifIter`].
    ///
    ///   ## Example
    ///
    ///   ```rust
    ///   use nom_exif::*;
    ///
    ///   fn main() -> Result<()> {
    ///       let mut parser = MediaParser::new();
    ///       
    ///       let ms = MediaSource::open("./testdata/exif.jpg")?;
    ///       assert_eq!(ms.kind(), MediaKind::Image);
    ///       let iter = parser.parse_exif(ms)?;
    ///       let exif: Exif = iter.into();
    ///
    ///       assert_eq!(exif.get_by_ifd_tag_code(0, 0x0110).unwrap(), &"vivo X90 Pro+".into());
    ///       assert_eq!(exif.get_by_ifd_tag_code(1, 0xa002).unwrap(), &240_u32.into());
    ///       Ok(())
    ///   }
    ///   ```
    pub fn get_by_ifd_tag_code(&self, ifd: usize, tag: u16) -> Option<&EntryValue> {
        self.ifds.get(ifd).and_then(|ifd| ifd.get(tag))
    }

    /// Get parsed GPS information.
    pub fn get_gps_info(&self) -> crate::Result<Option<GPSInfo>> {
        Ok(self.gps_info.clone())
    }

    fn put(&mut self, res: &mut ParsedExifEntry) {
        while self.ifds.len() < res.ifd_index() + 1 {
            self.ifds.push(ParsedImageFileDirectory::new());
        }
        if let Some(v) = res.take_value() {
            self.ifds[res.ifd_index()].put(res.tag_code(), v);
        }
    }
}

impl From<ExifIter> for Exif {
    fn from(iter: ExifIter) -> Self {
        let gps_info = iter.parse_gps_info().ok().flatten();
        let mut exif = Exif::new(gps_info);

        for mut it in iter {
            exif.put(&mut it);
        }

        exif
    }
}

pub(crate) const TIFF_HEADER_LEN: usize = 8;

/// TIFF Header
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TiffHeader {
    pub endian: Endianness,
    pub ifd0_offset: u32,
}

impl Default for TiffHeader {
    fn default() -> Self {
        Self {
            endian: Endianness::Big,
            ifd0_offset: 0,
        }
    }
}

impl Debug for TiffHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let endian_str = match self.endian {
            Endianness::Big => "Big",
            Endianness::Little => "Little",
            Endianness::Native => "Native",
        };
        f.debug_struct("TiffHeader")
            .field("endian", &endian_str)
            .field("ifd0_offset", &format!("{:#x}", self.ifd0_offset))
            .finish()
    }
}

pub(crate) const IFD_ENTRY_SIZE: usize = 12;

impl TiffHeader {
    pub fn parse(input: &[u8]) -> IResult<&[u8], TiffHeader> {
        use nom::number::streaming::{u16, u32};
        let (remain, endian) = TiffHeader::parse_endian(input)?;
        let (_, (_, offset)) = (
            combinator::verify(u16(endian), |magic| *magic == 0x2a),
            u32(endian),
        ).parse(remain)?;

        let header = Self {
            endian,
            ifd0_offset: offset,
        };

        Ok((remain, header))
    }

    pub fn parse_ifd_entry_num(input: &[u8], endian: Endianness) -> IResult<&[u8], u16> {
        let (remain, num) = nom::number::streaming::u16(endian)(input)?; // Safe-slice
        if num == 0 {
            return Ok((remain, 0));
        }

        // 12 bytes per entry
        let size = (num as usize)
            .checked_mul(IFD_ENTRY_SIZE)
            .expect("should fit");

        if size > remain.len() {
            return Err(nom::Err::Incomplete(Needed::new(size - remain.len())));
        }

        Ok((remain, num))
    }

    // pub fn first_ifd<'a>(&self, input: &'a [u8], tag_ids: HashSet<u16>) -> IResult<&'a [u8], IFD> {
    //     // ifd0_offset starts from the beginning of Header, so we should
    //     // subtract the header size, which is 8
    //     let offset = self.ifd0_offset - 8;

    //     // skip to offset
    //     let (_, remain) = take(offset)(input)?;

    //     IFD::parse(remain, self.endian, tag_ids)
    // }

    fn parse_endian(input: &[u8]) -> IResult<&[u8], Endianness> {
        combinator::map(alt((tag("MM"), tag("II"))), |endian_marker| {
            if endian_marker == b"MM" {
                Endianness::Big
            } else {
                Endianness::Little
            }
        }).parse(input)
    }
}

pub(crate) fn check_exif_header(data: &[u8]) -> Result<bool, nom::Err<nom::error::Error<&[u8]>>> {
    tag::<_, _, nom::error::Error<_>>(EXIF_IDENT)(data).map(|_| true)
}

pub(crate) fn check_exif_header2(i: &[u8]) -> IResult<&[u8], ()> {
    let (remain, _) = (
        nom::number::complete::be_u32,
        nom::bytes::complete::tag(EXIF_IDENT),
    ).parse(i)?;
    Ok((remain, ()))
}

pub(crate) const EXIF_IDENT: &str = "Exif\0\0";

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::thread;

    use test_case::test_case;

    use crate::exif::input_into_iter;
    use crate::jpeg::extract_exif_data;
    use crate::slice::SubsliceRange;
    use crate::testkit::{open_sample, read_sample};
    use crate::ParsedExifEntry;

    use super::*;

    #[test]
    fn header() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = [0x4d, 0x4d, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x08, 0x00];

        let (_, header) = TiffHeader::parse(&buf).unwrap();
        assert_eq!(
            header,
            TiffHeader {
                endian: Endianness::Big,
                ifd0_offset: 8,
            }
        );
    }

    #[test_case("exif.jpg")]
    fn exif_iter_gps(path: &str) {
        let buf = read_sample(path).unwrap();
        let (_, data) = extract_exif_data(&buf).unwrap();
        let range = data.and_then(|x| buf.subslice_in_range(x)).unwrap();
        let data = bytes::Bytes::from(buf).slice(range);
        let iter = input_into_iter(data, None).unwrap();
        let gps = iter.parse_gps_info().unwrap().unwrap();
        assert_eq!(gps.to_iso6709(), "+22.53113+114.02148/");
    }

    #[test_case("exif.jpg")]
    fn clone_exif_iter_to_thread(path: &str) {
        let buf = read_sample(path).unwrap();
        let (_, data) = extract_exif_data(&buf).unwrap();
        let range = data.and_then(|x| buf.subslice_in_range(x)).unwrap();
        let data = bytes::Bytes::from(buf).slice(range);
        let iter = input_into_iter(data, None).unwrap();
        let iter2 = iter.clone();

        let mut expect = String::new();
        open_sample(&format!("{path}.txt"))
            .unwrap()
            .read_to_string(&mut expect)
            .unwrap();

        let jh = thread::spawn(move || iter_to_str(iter2));

        let result = iter_to_str(iter);

        // open_sample_w(&format!("{path}.txt"))
        //     .unwrap()
        //     .write_all(result.as_bytes())
        //     .unwrap();

        assert_eq!(result.trim(), expect.trim());
        assert_eq!(jh.join().unwrap().trim(), expect.trim());
    }

    fn iter_to_str(it: impl Iterator<Item = ParsedExifEntry>) -> String {
        let ss = it
            .map(|x| {
                format!(
                    "ifd{}.{:<32} » {}",
                    x.ifd_index(),
                    x.tag()
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| format!("Unknown(0x{:04x})", x.tag_code())),
                    x.get_result()
                        .map(|v| v.to_string())
                        .map_err(|e| e.to_string())
                        .unwrap_or_else(|s| s)
                )
            })
            .collect::<Vec<String>>();
        ss.join("\n")
    }

    #[test]
    fn p5_baseline_exif_jpg_dump_snapshot() {
        // Lock down the post-refactor invariant: parsing testdata/exif.jpg
        // through the public API yields the same set of (ifd, tag, value)
        // triples before and after every P5 task. Captured as a sorted
        // formatted string so the assertion is a single Vec compare.
        use crate::{MediaParser, MediaSource};
        let mut parser = MediaParser::new();
        let ms = MediaSource::open("testdata/exif.jpg").unwrap();
        let iter = parser.parse_exif(ms).unwrap();

        let mut entries: Vec<String> = iter
            .map(|e| {
                let val = match e.get_result() {
                    Ok(v) => format!("{v}"),
                    Err(err) => format!("<err:{err}>"),
                };
                format!("ifd{}.0x{:04x}={val}", e.ifd_index(), e.tag_code())
            })
            .collect();
        entries.sort();
        assert!(entries.len() > 5, "expected >5 entries, got {}", entries.len());
        assert!(
            entries.iter().any(|s| s.contains("0x010f")),
            "expected Make tag (0x010f) in snapshot, got {entries:?}"
        );
    }
}

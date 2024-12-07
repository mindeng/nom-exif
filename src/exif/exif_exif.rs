use nom::{branch::alt, bytes::streaming::tag, combinator, number::Endianness, IResult, Needed};

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
    ///   see [`Self::get_by_tag_code`].
    ///
    ///   ## Example
    ///
    ///   ```rust
    ///   use nom_exif::*;
    ///
    ///   fn main() -> Result<()> {
    ///       let mut parser = MediaParser::new();
    ///       
    ///       let ms = MediaSource::file_path("./testdata/exif.jpg")?;
    ///       let iter: ExifIter = parser.parse(ms)?;
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
    ///       let ms = MediaSource::file_path("./testdata/exif.jpg")?;
    ///       let iter: ExifIter = parser.parse(ms)?;
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

    /// Get entry values for the specified `tags` in ifd0 (the main image).
    ///
    /// Please note that this method will ignore errors encountered during the
    /// search and parsing process, such as missing tags or errors in parsing
    /// values, and handle them silently.
    #[deprecated(
        since = "1.5.0",
        note = "please use [`Self::get`] or [`ExifIter`] instead"
    )]
    pub fn get_values<'b>(&self, tags: &'b [ExifTag]) -> Vec<(&'b ExifTag, EntryValue)> {
        tags.iter()
            .zip(tags.iter())
            .filter_map(|x| {
                #[allow(deprecated)]
                self.get_value(x.0)
                    .map(|v| v.map(|v| (x.0, v)))
                    .unwrap_or(None)
            })
            .collect::<Vec<_>>()
    }

    /// Get entry value for the specified `tag` in ifd0 (the main image).
    #[deprecated(since = "1.5.0", note = "please use [`Self::get`] instead")]
    pub fn get_value(&self, tag: &ExifTag) -> crate::Result<Option<EntryValue>> {
        #[allow(deprecated)]
        self.get_value_by_tag_code(tag.code())
    }

    /// Get entry value for the specified `tag` in ifd0 (the main image).
    #[deprecated(since = "1.5.0", note = "please use [`Self::get_by_tag_code`] instead")]
    pub fn get_value_by_tag_code(&self, tag: u16) -> crate::Result<Option<EntryValue>> {
        Ok(self.get_by_ifd_tag_code(0, tag).map(|x| x.to_owned()))
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

/// TIFF Header
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TiffHeader {
    pub endian: Endianness,
    pub bigtiff: bool,
    pub ifd0_offset: u64,
}

impl Default for TiffHeader {
    fn default() -> Self {
        Self {
            endian: Endianness::Big,
            bigtiff: false,
            ifd0_offset: 0,
        }
    }
}

pub(crate) const IFD_REGULAR_TIFF_ENTRY_SIZE: usize = 12;
pub(crate) const IFD_BIG_TIFF_ENTRY_SIZE: usize = 20;

impl TiffHeader {
    pub fn parse(input: &[u8]) -> IResult<&[u8], TiffHeader> {
        let (remain, endian) = TiffHeader::parse_endian(input)?;
        let (remain, bigtiff) = TiffHeader::parse_bigtiff(remain, endian)?;
        let (remain, offset) = if bigtiff {
            // http://bigtiff.org/ describes the BigTIFF header additions as constants 0x8 and 0x0.
            let (remain, first_word) = nom::number::streaming::u16(endian)(remain)?;
            if first_word != 0x8 {
                return Err(nom::Err::Failure(nom::error::make_error(
                    input,
                    nom::error::ErrorKind::Fail,
                )));
            }
            let (remain, second_word) = nom::number::streaming::u16(endian)(remain)?;
            if second_word != 0x0 {
                return Err(nom::Err::Failure(nom::error::make_error(
                    input,
                    nom::error::ErrorKind::Fail,
                )));
            }
            // offset
            nom::number::streaming::u64(endian)(remain)?
        } else {
            let (remain, offset) = nom::number::streaming::u32(endian)(remain)?;
            (remain, offset as u64)
        };

        let header = Self {
            endian,
            bigtiff,
            ifd0_offset: offset,
        };

        Ok((remain, header))
    }

    pub fn parse_ifd_entry_num(
        input: &[u8],
        endian: Endianness,
        bigtiff: bool,
    ) -> IResult<&[u8], u64> {
        let (remain, num) = if bigtiff {
            nom::number::streaming::u64(endian)(input)?
        } else {
            let (remain, num) = nom::number::streaming::u16(endian)(input)?;
            (remain, num as u64)
        };
        if num == 0 {
            return Ok((remain, 0));
        }

        /* TODO
        // 12 bytes per entry
        let size = (num as usize)
            .checked_mul(IFD_ENTRY_SIZE)
            .expect("should fit");

        if size > remain.len() {
            return Err(nom::Err::Incomplete(Needed::new(size - remain.len())));
        }
        */

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
        })(input)
    }

    fn parse_bigtiff(input: &[u8], endian: Endianness) -> IResult<&[u8], bool> {
        let (remain, magic_marker) = nom::number::streaming::u16(endian)(input)?;
        if magic_marker == 43 {
            Ok((remain, true))
        } else if magic_marker == 42 {
            Ok((remain, false))
        } else {
            Err(nom::Err::Failure(nom::error::make_error(
                input,
                nom::error::ErrorKind::Fail,
            )))
        }
    }
}

pub(crate) fn check_exif_header(data: &[u8]) -> Result<bool, nom::Err<nom::error::Error<&[u8]>>> {
    tag::<_, _, nom::error::Error<_>>(EXIF_IDENT)(data).map(|_| true)
}

pub(crate) fn check_exif_header2(i: &[u8]) -> IResult<&[u8], ()> {
    let (remain, _) = nom::sequence::tuple((
        nom::number::complete::be_u32,
        nom::bytes::complete::tag(EXIF_IDENT),
    ))(i)?;
    Ok((remain, ()))
}

pub(crate) const EXIF_IDENT: &str = "Exif\0\0";

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::thread;

    use crate::partial_vec::PartialVec;
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
                bigtiff: false,
                ifd0_offset: 8,
            }
        );
    }

    #[test_case("exif.jpg")]
    fn exif_iter_gps(path: &str) {
        let buf = read_sample(path).unwrap();
        let (_, data) = extract_exif_data(&buf).unwrap();
        let data = data
            .and_then(|x| buf.subslice_in_range(x))
            .map(|x| PartialVec::from_vec_range(buf, x))
            .unwrap();
        let iter = input_into_iter(data, None).unwrap();
        let gps = iter.parse_gps_info().unwrap().unwrap();
        assert_eq!(gps.format_iso6709(), "+22.53113+114.02148/");
    }

    #[test_case("exif.jpg")]
    fn clone_exif_iter_to_thread(path: &str) {
        let buf = read_sample(path).unwrap();
        let (_, data) = extract_exif_data(&buf).unwrap();
        let data = data
            .and_then(|x| buf.subslice_in_range(x))
            .map(|x| PartialVec::from_vec_range(buf, x))
            .unwrap();
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
}

use nom::{
    branch::alt,
    bytes::streaming::tag,
    combinator::{map, verify},
    number::{
        streaming::{u16, u32},
        Endianness,
    },
    sequence::tuple,
    IResult, Needed,
};

use crate::{
    error::{ParsedError, ParsingError},
    exif::{ExifTag, GPSInfo},
    input::{self, Input},
    parser::ParsingState,
    EntryValue,
};

use super::{
    exif_iter::{ExifIter, ImageFileDirectoryIter, ParsedExifEntry},
    ifd::ParsedImageFileDirectory,
};

/// Parses Exif information from the `input` TIFF data.
pub(crate) fn input_to_exif_iter<'a>(
    input: impl Into<input::Input<'a>>,
) -> crate::Result<ExifIter<'a>> {
    let input = input.into();
    let parser = ExifParser::new(input);
    let iter: ExifIter<'a> = parser.parse_iter(None)?;
    Ok(iter)
}

pub(crate) struct ExifParser<'a> {
    inner: Inner<'a>,
}

impl<'a> ExifParser<'a> {
    /// Create a new ExifParser. `input` can be:
    ///
    /// - A `Vec<u8>`, which will be auto converted to an `Input<'static>`,
    ///   therefore an `ExifParser<'static>` will be returned.
    ///
    /// - A `&'a [u8]`, which will be auto converted to an `Input<'a>`,
    ///   therefore an `ExifParser<'a>` will be returned.
    ///
    pub fn new(input: impl Into<input::Input<'a>>) -> ExifParser<'a> {
        Self {
            inner: Inner::new(input),
        }
    }

    /// Parses header from input data, and returns an [`ExifIter`].
    ///
    /// All entries are lazy-parsed. That is, only when you iterate over
    /// [`ExifIter`] will the IFD entries be parsed one by one.
    ///
    /// The one exception is the time zone entries. The method will try to find
    /// and parse the time zone data first, so we can correctly parse all time
    /// information in subsequent iterates.
    pub fn parse_iter(self, state: Option<ParsingState>) -> Result<ExifIter<'a>, ParsedError> {
        let iter = self.inner.try_into_iter(state).map_err(|e| match e {
            ParsingError::Need(_) => unreachable!(),
            ParsingError::ClearAndSkip(_, _) => unreachable!(),
            ParsingError::Failed(v) => ParsedError::Failed(v),
        })?;
        Ok(iter)
    }
}

struct Inner<'a> {
    input: Input<'a>,
}

impl<'a> Inner<'a> {
    fn new(input: impl Into<input::Input<'a>>) -> Inner<'a> {
        Self {
            input: input.into(),
        }
    }

    fn try_into_iter(self, state: Option<ParsingState>) -> Result<ExifIter<'a>, ParsingError> {
        let (header, start) = match state {
            // header has been parsed, and header has been skipped, input data
            // is the IFD data
            Some(ParsingState::TiffHeader(header)) => (header, 0),
            None => {
                // header has not been parsed, input data includes IFD header
                let (_, header) = TiffHeader::parse(&self.input[..])?;
                let start = header.ifd0_offset as usize;
                if start > self.input.len() {
                    return Err(ParsingError::ClearAndSkip(
                        start,
                        Some(ParsingState::TiffHeader(header)),
                    ));
                    // return Err(ParsingError::Need(start - data.len()));
                }

                (header, start)
            }
        };

        let data = &self.input[..];

        let mut ifd0 = match ImageFileDirectoryIter::try_new(
            0,
            self.input.make_associated(&data[start..]),
            header.ifd0_offset,
            header.endian,
            None,
        ) {
            Ok(ifd0) => ifd0,
            Err(e) => return Err(ParsingError::Failed(e.to_string())),
        };

        let tz = ifd0.find_tz_offset();
        ifd0.tz = tz.clone();
        let iter: ExifIter<'a> = ExifIter::new(self.input, header, tz, Some(ifd0));

        Ok(iter)
    }
}

/// Represents a parsed Exif information, can be converted from an [`ExifIter`]
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

impl From<ExifIter<'_>> for Exif {
    fn from(iter: ExifIter<'_>) -> Self {
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

pub(crate) const IFD_ENTRY_SIZE: usize = 12;

impl TiffHeader {
    pub fn parse(input: &[u8]) -> IResult<&[u8], TiffHeader> {
        let (remain, endian) = TiffHeader::parse_endian(input)?;
        let (_, (_, offset)) =
            tuple((verify(u16(endian), |magic| *magic == 0x2a), u32(endian)))(remain)?;

        let header = Self {
            endian,
            ifd0_offset: offset,
        };

        Ok((remain, header))
    }

    #[tracing::instrument(skip_all)]
    pub fn parse_ifd_entry_num<'a>(input: &'a [u8], endian: Endianness) -> IResult<&'a [u8], u16> {
        let (remain, num) = nom::number::streaming::u16(endian)(input)?; // Safe-slice
        if num == 0 {
            return Ok((remain, 0));
        }

        // 12 bytes per entry
        let size = (num as usize)
            .checked_mul(IFD_ENTRY_SIZE)
            .expect("should be fit");

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
        map(alt((tag("MM"), tag("II"))), |endian_marker| {
            if endian_marker == b"MM" {
                Endianness::Big
            } else {
                Endianness::Little
            }
        })(input)
    }
}

/// data.len() MUST >= 6
pub(crate) fn check_exif_header(data: &[u8]) -> bool {
    use nom::bytes::complete;
    assert!(data.len() >= 6);

    const EXIF_IDENT: &str = "Exif\0\0";
    complete::tag::<_, _, nom::error::Error<_>>(EXIF_IDENT)(data).is_ok()
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::thread;

    use test_case::test_case;

    use crate::exif::{GPSInfo, LatLng};
    use crate::jpeg::extract_exif_data;
    use crate::slice::SubsliceRange;
    use crate::testkit::{open_sample, read_sample};
    use crate::values::URational;

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

    #[test_case(
        "exif.jpg",
        'N',
        [(22, 1), (31, 1), (5208, 100)].into(),
        'E',
        [(114, 1), (1, 1), (1733, 100)].into(),
        0u8,
        (0, 1).into(),
        None,
        None
    )]
    #[allow(clippy::too_many_arguments)]
    fn gps_info(
        path: &str,
        latitude_ref: char,
        latitude: LatLng,
        longitude_ref: char,
        longitude: LatLng,
        altitude_ref: u8,
        altitude: URational,
        speed_ref: Option<char>,
        speed: Option<URational>,
    ) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();

        // skip first 12 bytes
        let exif: Exif = input_to_exif_iter(&buf[12..]).unwrap().into(); // Safe-slice in test_case

        let gps = exif.get_gps_info().unwrap().unwrap();
        assert_eq!(
            gps,
            GPSInfo {
                latitude_ref,
                latitude,
                longitude_ref,
                longitude,
                altitude_ref,
                altitude,
                speed_ref,
                speed,
            }
        )
    }

    #[test_case("exif.jpg")]
    fn exif_iter_gps(path: &str) {
        let buf = read_sample(path).unwrap();
        let (_, data) = extract_exif_data(&buf).unwrap();
        let data = data
            .and_then(|x| buf.subslice_range(x))
            .map(|x| Input::from_vec_range(buf, x))
            .unwrap();
        let parser = ExifParser::new(data);
        let iter = parser.parse_iter(None).unwrap();
        let gps = iter.parse_gps_info().unwrap().unwrap();
        assert_eq!(gps.format_iso6709(), "+22.53113+114.02148/");
    }

    #[test_case("exif.jpg")]
    fn clone_exif_iter_to_thread(path: &str) {
        let buf = read_sample(path).unwrap();
        let (_, data) = extract_exif_data(&buf).unwrap();
        let data = data
            .and_then(|x| buf.subslice_range(x))
            .map(|x| Input::from_vec_range(buf, x))
            .unwrap();
        let parser = ExifParser::new(data);
        let iter = parser.parse_iter(None).unwrap();
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

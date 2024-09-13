use nom::{
    branch::alt,
    bytes::complete::{tag, take},
    combinator::{map, verify},
    number::{
        complete::{u16, u32},
        Endianness,
    },
    sequence::tuple,
    IResult,
};

use crate::{
    exif::{ExifTag, GPSInfo},
    input::{self, Input},
    EntryValue,
};

use super::{
    exif_iter::{ExifIter, ImageFileDirectoryIter, ParsedExifEntry},
    ifd::ParsedImageFileDirectory,
};

/// Parses Exif information from the `input` TIFF data.
pub(crate) fn input_to_iter<'a>(input: impl Into<input::Input<'a>>) -> crate::Result<ExifIter<'a>> {
    let input = input.into();
    let parser = ExifParser::new(input);
    let iter: ExifIter<'a> = parser.parse_iter()?;
    Ok(iter)
}

/// Parses Exif information from the `input` TIFF data.
pub(crate) fn input_to_exif<'a>(input: impl Into<input::Input<'a>>) -> crate::Result<Exif> {
    Ok(input_to_iter(input)?.into())
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
    pub fn parse_iter(self) -> crate::Result<ExifIter<'a>> {
        let iter = self.inner.try_into_iter()?;
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

    fn try_into_iter(self) -> crate::Result<ExifIter<'a>> {
        let data = &self.input[..];
        let (_, header) = Header::parse(data)?;

        // jump to ifd0
        let (remain, _) = take::<_, _, nom::error::Error<_>>((header.ifd0_offset) as usize)(data)
            .map_err(|_| "not enough bytes")?;
        if remain.is_empty() {
            return Ok(ExifIter::default());
        }

        let pos = data.len() - remain.len();
        let mut ifd0 = match ImageFileDirectoryIter::try_new(
            0,
            self.input.make_associated(data),
            pos,
            header.endian,
            None,
        ) {
            Ok(ifd0) => ifd0,
            Err(e) => return Err(e),
        };

        let tz = ifd0.find_tz_offset();
        ifd0.tz = tz.clone();
        let iter: ExifIter<'a> = ExifIter::new(self.input, header.endian, tz, Some(ifd0));

        Ok(iter)
    }
}

/// Represents a parsed Exif information.
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
    pub fn get(&self, tag: ExifTag) -> Option<&EntryValue> {
        self.get_by_tag_code(tag.code())
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
    pub fn get_by_tag_code(&self, tag: u16) -> Option<&EntryValue> {
        self.ifd0().and_then(|ifd0| ifd0.get(tag))
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
        Ok(self.get_by_tag_code(tag).map(|x| x.to_owned()))
    }

    /// Get parsed GPS information.
    pub fn get_gps_info(&self) -> crate::Result<Option<GPSInfo>> {
        Ok(self.gps_info.clone())
    }

    fn put(&mut self, res: ParsedExifEntry) {
        while self.ifds.len() < res.ifd_index() + 1 {
            self.ifds.push(ParsedImageFileDirectory::new());
        }
        if let Some(v) = res.take_value() {
            self.ifds[res.ifd_index()].put(res.tag_code(), v);
        }
    }

    fn ifd0(&self) -> Option<&ParsedImageFileDirectory> {
        self.ifds.first()
    }
}

impl From<ExifIter<'_>> for Exif {
    fn from(iter: ExifIter<'_>) -> Self {
        let gps_info = iter.parse_gps_info().ok().flatten();
        let mut exif = Exif::new(gps_info);

        for it in iter {
            exif.put(it);
        }

        exif
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Header {
    pub endian: Endianness,
    pub ifd0_offset: u32,
}

impl Header {
    pub fn parse(input: &[u8]) -> IResult<&[u8], Header> {
        let (remain, endian) = Header::parse_endian(input)?;
        map(
            tuple((verify(u16(endian), |magic| *magic == 0x2a), u32(endian))),
            move |(_, offset)| Header {
                endian,
                ifd0_offset: offset,
            },
        )(remain)
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

        let buf = [0x4d, 0x4d, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x08];

        let (_, header) = Header::parse(&buf).unwrap();
        assert_eq!(
            header,
            Header {
                endian: Endianness::Big,
                ifd0_offset: 8
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
        '\x00',
        URational::default()
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
        speed_ref: char,
        speed: URational,
    ) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();

        // skip first 12 bytes
        let exif = input_to_exif(&buf[12..]).unwrap(); // Safe-slice in test_case

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
    fn exif_iter(path: &str) {
        use core::fmt::Write;
        let buf = read_sample(path).unwrap();
        let (_, data) = extract_exif_data(&buf).unwrap();
        let data = data
            .and_then(|x| buf.subslice_range(x))
            .map(|x| Input::from_vec_range(buf, x))
            .unwrap();
        let parser = ExifParser::new(data);
        let iter = parser.parse_iter().unwrap();

        let mut expect = String::new();
        open_sample(&format!("{path}.txt"))
            .unwrap()
            .read_to_string(&mut expect)
            .unwrap();

        let mut result = String::new();
        for res in iter {
            writeln!(&mut result, "{res:?}").unwrap();
        }

        // open_sample_w(&format!("{path}.txt"))
        //     .unwrap()
        //     .write_all(result.as_bytes())
        //     .unwrap();

        assert_eq!(result.trim(), expect.trim());
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
        let iter = parser.parse_iter().unwrap();
        let gps = iter.parse_gps_info().unwrap().unwrap();
        assert_eq!(gps.format_iso6709(), "+22.53113+114.02148/");
    }

    #[test_case("exif.jpg")]
    fn clone_exif_iter_to_thread(path: &str) {
        use core::fmt::Write;
        let buf = read_sample(path).unwrap();
        let (_, data) = extract_exif_data(&buf).unwrap();
        let data = data
            .and_then(|x| buf.subslice_range(x))
            .map(|x| Input::from_vec_range(buf, x))
            .unwrap();
        let parser = ExifParser::new(data);
        let iter = parser.parse_iter().unwrap();
        let iter2 = iter.clone();

        let mut expect = String::new();
        open_sample(&format!("{path}.txt"))
            .unwrap()
            .read_to_string(&mut expect)
            .unwrap();

        let jh = thread::spawn(move || {
            let mut result = String::new();
            for res in iter2 {
                writeln!(&mut result, "{res:?}").unwrap();
            }
            result
        });

        let mut result = String::new();
        for res in iter {
            writeln!(&mut result, "{res:?}").unwrap();
        }

        assert_eq!(result.trim(), expect.trim());
        assert_eq!(jh.join().unwrap().trim(), expect.trim());
    }
}

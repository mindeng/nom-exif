use std::{
    borrow::{Borrow, BorrowMut},
    cell::RefCell,
    collections::HashMap,
    fmt::Debug,
    sync::{Arc, Mutex, RwLock},
};

use nom::{
    branch::alt,
    bytes::complete::{tag, take},
    combinator::{map, map_res, verify},
    multi::many_m_n,
    number::{
        complete::{u16, u32},
        Endianness,
    },
    sequence::tuple,
    IResult, Needed,
};

use crate::{
    exif::{ExifTag, GPSInfo},
    input::{self, Input},
    values::DataFormat,
    EntryValue, Error,
};

use super::{
    ifd::{self, ImageFileDirectoryIter, ParsedIdfEntry, ParsedImageFileDirectory},
    tags::ExifTagCode,
};

/// Parses Exif information from the `input` TIFF data.
///
/// Please note that Exif values are lazy-parsed, meaning that they are only
/// truly parsed when the `Exif::get_value` and `Exif::get_values` methods are
/// called.
///
/// This allows you to parse Exif values on-demand, reducing the parsing
/// overhead.
pub fn parse_exif<'a>(input: impl Into<input::Input<'a>>) -> crate::Result<Exif> {
    let input = input.into();
    let (_, header) = Header::parse(&input)?;

    let parser = ExifParser::new(input);
    let iter: ExifIter<'a> = parser.parse_iter()?;
    let gps_info = iter.parse_gps_info().ok().flatten();
    let mut exif = Exif::new(header, gps_info);

    for it in iter {
        exif.put(it);
    }

    Ok(exif)
}

pub struct ExifParser<'a> {
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

// /// Parses Exif information from the `input` TIFF data.
// ///
// /// All entries are lazy-parsed. That is, only when you iterate over
// /// [`ExifIter`] will the IFD entries be parsed one by one.
// pub fn parse_exif_iter<'a>(input: impl Into<input::Input<'a>>) -> crate::Result<ExifIter<'a>> {
//     let input: Input<'a> = input.into();
//     let (_, header) = Header::parse(&input)?;

//     // jump to ifd0

//     let (remain, _) =
//         take::<_, _, nom::error::Error<_>>((header.ifd0_offset) as usize)(input.as_ref())
//             .map_err(|_| "not enough bytes")?;
//     if remain.is_empty() {
//         return Ok(ExifIter::default());
//     }

//     let pos = input.len() - remain.len();
//     let mut ifd0 = match ImageFileDirectoryIter::try_new(
//         0,
//         input.make_associated(&input[..]),
//         pos,
//         header.endian,
//         None,
//     ) {
//         Ok(ifd0) => ifd0,
//         Err(e) => return Err(e),
//     };

//     let tz = ifd0.find_tz_offset();
//     ifd0.tz = tz.clone();

//     let iter = ExifIter::new(input, header.endian, tz, vec![ifd0]);

//     Ok(iter)
// }

/// Represents Exif information in a JPEG/HEIF file.
///
/// Please note that Exif values are lazy-parsed, meaning that they are only
/// truly parsed when the `Exif::get_value` and `Exif::get_values` methods are
/// called.

/// This allows you to parse Exif values on-demand, reducing the parsing
/// overhead.
#[derive(Clone, Debug, PartialEq)]
pub struct Exif {
    header: Header,
    ifds: Vec<ParsedImageFileDirectory>,
    gps_info: Option<GPSInfo>,
}

impl Exif {
    pub fn new(header: Header, gps_info: Option<GPSInfo>) -> Exif {
        Exif {
            header,
            ifds: Vec::new(),
            gps_info,
        }
    }
}

/// An iterator version of [`Exif`].
#[derive(Debug, Clone)]
pub struct ExifIter<'a> {
    input: Input<'a>,
    endian: Endianness,
    tz: Option<String>,
    ifd0: Option<ImageFileDirectoryIter>,

    // Iterating status
    ifds: Vec<ImageFileDirectoryIter>,
}

impl<'a> ExifIter<'a> {
    fn new(
        input: impl Into<Input<'a>>,
        endian: Endianness,
        tz: Option<String>,
        ifd0: Option<ImageFileDirectoryIter>,
    ) -> ExifIter<'a> {
        let mut ifds = Vec::new();
        if let Some(ref ifd0) = ifd0 {
            ifds.push(ifd0.clone());
        }
        ExifIter {
            input: input.into(),
            endian,
            tz,
            ifd0,
            ifds,
        }
    }

    /// Try to find and parse gps information.
    ///
    /// Returns:
    ///
    /// - An `Ok<Some<GPSInfo>>` if gps info is found and parsed successfully.
    /// - An `Ok<None>` if gps info is not found.
    /// - An `Err` if gps info is found but parsing failed.
    pub fn parse_gps_info(&self) -> crate::Result<Option<GPSInfo>> {
        let mut iter = self.shallow_clone();
        let Some(gps) = iter.find(|x| x.tag.tag() == ExifTag::GPSInfo) else {
            return Ok(None);
        };

        let offset = match gps.res {
            Ok(v) => v.as_u32().unwrap() as usize,
            Err(e) => return Err(e),
        };

        let data = &iter.input[..];
        let mut gps_subifd = match ImageFileDirectoryIter::try_new(
            gps.ifd,
            iter.input.make_associated(data),
            offset,
            iter.endian,
            iter.tz.clone(),
        ) {
            Ok(ifd0) => ifd0,
            Err(e) => return Err(e),
        };
        Ok(gps_subifd.parse_gps_info())
    }

    // Make sure we won't clone the owned data.
    fn shallow_clone(&'a self) -> Self {
        ExifIter::new(
            &self.input[..],
            self.endian,
            self.tz.clone(),
            self.ifd0.clone(),
        )
    }
}

impl Default for ExifIter<'static> {
    fn default() -> Self {
        Self::new(Input::default(), Endianness::Big, None, None)
    }
}

pub struct IfdEntryResult {
    /// 0: ifd0, 1: ifd1
    pub ifd: usize,
    pub tag: ExifTagCode,
    pub res: crate::Result<EntryValue>,
}

impl Debug for IfdEntryResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match &self.res {
            Ok(v) => format!("{v}"),
            Err(e) => format!("{e:?}"),
        };
        f.debug_struct("IfdEntryResult")
            .field("ifd", &format!("ifd{}", self.ifd))
            .field("tag", &self.tag)
            .field("value", &value)
            .finish()
    }
}

impl IfdEntryResult {
    fn ok(ifd: usize, tag: ExifTagCode, v: EntryValue) -> Self {
        Self {
            ifd,
            tag,
            res: Ok(v),
        }
    }

    fn err(ifd: usize, tag: ExifTagCode, e: ifd::Error) -> Self {
        Self {
            ifd,
            tag,
            res: Err(crate::Error::InvalidEntry(e.into())),
        }
    }
}

impl<'a> Iterator for ExifIter<'a> {
    type Item = IfdEntryResult;

    fn next(&mut self) -> Option<Self::Item> {
        let endian = self.endian;
        loop {
            if self.ifds.len() > MAX_IFD_DEPTH {
                self.ifds.pop();
            }

            let mut ifd = self.ifds.pop()?;
            match ifd.next() {
                Some((tag_code, entry)) => match entry {
                    super::ifd::IfdEntry::Ifd { idx, offset } => {
                        let is_subifd = if idx == ifd.ifd_idx {
                            // Push the current ifd before enter sub-ifd.
                            self.ifds.push(ifd);
                            true
                        } else {
                            // Otherwise this is a next ifd. It means that the
                            // current ifd has been parsed, so we don't need to
                            // push it.
                            false
                        };

                        if let Ok(ifd) = ImageFileDirectoryIter::try_new(
                            idx,
                            self.input.make_associated(&self.input[..]),
                            offset,
                            endian,
                            self.tz.clone(),
                        ) {
                            self.ifds.push(ifd);
                        }

                        if is_subifd {
                            // Return sub-ifd as an entry
                            return Some(IfdEntryResult::ok(
                                idx,
                                tag_code,
                                EntryValue::U32(offset as u32),
                            ));
                        }
                    }
                    super::ifd::IfdEntry::Entry(v) => {
                        let res = Some(IfdEntryResult::ok(ifd.ifd_idx, tag_code, v));
                        self.ifds.push(ifd);
                        return res;
                    }
                    super::ifd::IfdEntry::Err(e) => {
                        let res = Some(IfdEntryResult::err(ifd.ifd_idx, tag_code, e));
                        self.ifds.push(ifd);
                        return res;
                    }
                },
                None => continue,
            }
        }
    }
}

impl Exif {
    /// Get entry value for the specified `tag` in ifd0 (the main image).
    ///
    /// *Note*:
    ///
    /// - The parsing error related to this tag won't be reported by this
    ///   method. If this entry is not parsed successfully, this method will
    ///   return None.
    ///
    /// - If you want to handle parsing error, please consider to use
    ///   [`ExifIter`].
    pub fn get(&self, tag: ExifTag) -> Option<&EntryValue> {
        self.get_by_tag_code(tag.code())
    }

    /// Get entry value for the specified `tag` in ifd0 (the main image).
    ///
    /// *Note*:
    ///
    /// - The parsing error related to this tag won't be reported by this
    ///   method. If this entry is not parsed successfully, this method will
    ///   return None.
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
    #[deprecated(since = "1.4.2", note = "please use [`get`] or [`ExifIter`] instead")]
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
    #[deprecated(since = "1.4.2", note = "please use [`get`] instead")]
    pub fn get_value(&self, tag: &ExifTag) -> crate::Result<Option<EntryValue>> {
        #[allow(deprecated)]
        self.get_value_by_tag_code(tag.code())
    }

    /// Get entry value for the specified `tag` in ifd0 (the main image).
    #[deprecated(since = "1.4.2", note = "please use [`get_by_tag_code`] instead")]
    pub fn get_value_by_tag_code(&self, tag: u16) -> crate::Result<Option<EntryValue>> {
        Ok(self.get_by_tag_code(tag).map(|x| x.to_owned()))
    }

    /// Get parsed GPS information.
    pub fn get_gps_info(&self) -> crate::Result<Option<GPSInfo>> {
        Ok(self.gps_info.clone())
    }

    fn put(&mut self, res: IfdEntryResult) {
        while self.ifds.len() < res.ifd + 1 {
            self.ifds.push(ParsedImageFileDirectory::new());
        }
        if let Ok(v) = res.res {
            self.ifds[res.ifd].put(res.tag.code(), v);
        }
    }

    fn ifd0(&self) -> Option<&ParsedImageFileDirectory> {
        self.ifds.first()
    }
}

const ENTRY_SIZE: usize = 12;
const MAX_IFD_DEPTH: usize = 8;

type IfdResult = Result<Option<ParsedImageFileDirectory>, Error>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
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

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use test_case::test_case;

    use crate::exif::ExifTag::*;
    use crate::exif::{GPSInfo, LatLng};
    use crate::jpeg::extract_exif_data;
    use crate::slice::SubsliceRange;
    use crate::testkit::{open_sample, open_sample_w, read_sample};
    use crate::values::{IRational, URational};

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

    #[test_case("exif.jpg")]
    fn test_parse_exif(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        tracing::info!(bytes = buf.len(), "File size");

        // skip first 12 bytes
        let exif = parse_exif(&buf[12..]).unwrap(); // Safe-slice in test_case
                                                    // TODO:
    }

    #[test_case(
        "exif.jpg",
        'N',
        [(22, 1), (31, 1), (5208, 100)].into(),
        'E',
        [(114, 1), (1, 1), (1733, 100)].into(),
        0u8,
        (0, 1).into()
    )]
    fn gps_info(
        path: &str,
        latitude_ref: char,
        latitude: LatLng,
        longitude_ref: char,
        longitude: LatLng,
        altitude_ref: u8,
        altitude: IRational,
    ) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();

        // skip first 12 bytes
        let exif = parse_exif(&buf[12..]).unwrap(); // Safe-slice in test_case

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
            }
        )
    }

    #[test_case("exif.jpg")]
    fn exif_iter(path: &str) {
        use std::fmt::Write;
        let buf = read_sample(path).unwrap();
        let (_, data) = extract_exif_data(&buf).unwrap();
        let data = data
            .and_then(|x| buf.subslice_range(x))
            .map(|x| Input::from_vec(buf, x))
            .unwrap();
        let mut parser = ExifParser::new(data);
        let mut iter = parser.parse_iter().unwrap();

        let mut expect = String::new();
        open_sample(&format!("{path}.txt"))
            .unwrap()
            .read_to_string(&mut expect)
            .unwrap();

        let mut result = String::new();
        for res in iter {
            writeln!(&mut result, "{res:?}");
        }

        // open_sample_w(&format!("{path}.txt"))
        //     .unwrap()
        //     .write_all(result.as_bytes())
        //     .unwrap();

        assert_eq!(result.trim(), expect.trim());
    }

    #[test_case("exif.jpg")]
    fn exif_gps(path: &str) {
        let buf = read_sample(path).unwrap();
        let (_, data) = extract_exif_data(&buf).unwrap();
        let data = data
            .and_then(|x| buf.subslice_range(x))
            .map(|x| Input::from_vec(buf, x))
            .unwrap();
        let mut parser = ExifParser::new(data);
        let iter = parser.parse_iter().unwrap();
        let gps = iter.parse_gps_info().unwrap().unwrap();
        assert_eq!(gps.format_iso6709(), "+22.53113+114.02148/");
    }
}

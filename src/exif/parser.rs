use std::collections::HashMap;

use nom::{
    branch::alt,
    bytes::complete::{tag, take},
    combinator::{fail, map, map_res, verify},
    error::context,
    multi::many_m_n,
    number::{
        complete::{u16, u32},
        Endianness,
    },
    sequence::tuple,
    IResult, Needed,
};

use crate::{exif::ExifTag, exif::GPSInfo, EntryValue};

use super::ifd::{entry_component_size, get_gps_info, DirectoryEntry, ImageFileDirectory};

/// Parses Exif information from the `input` TIFF data.
///
/// Please note that Exif values are lazy-parsed, meaning that they are only
/// truly parsed when the `Exif::get_value` and `Exif::get_values` methods are
/// called.
///
/// This allows you to parse Exif values on-demand, reducing the parsing
/// overhead.
pub fn parse_exif(input: &[u8]) -> crate::Result<Exif> {
    let (_, header) = Header::parse(input)?;

    // jump to ifd0
    let skip = (header.ifd0_offset) as usize;
    let (remain, _) = take(skip)(input)?;

    if remain.is_empty() {
        return Err("ifd0 is empty".into());
    }

    let parser = Parser {
        data: input,
        endian: header.endian,
    };

    // parse ifd0
    let (_, ifd0) = parser
        .parse_ifd(input.len() - remain.len())
        .map_err(|e| format!("Parse exif failed; {e}"))?;

    let exif = Exif {
        header,
        ifd0,
        tz: None,
    };

    let tz = exif.get_tz_offset();

    Ok(Exif { tz, ..exif })
}

/// Represents Exif information in a JPEG/HEIF file.
///
/// Please note that Exif values are lazy-parsed, meaning that they are only
/// truly parsed when the `Exif::get_value` and `Exif::get_values` methods are
/// called.

/// This allows you to parse Exif values on-demand, reducing the parsing
/// overhead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exif {
    header: Header,
    ifd0: Option<ImageFileDirectory>,
    tz: Option<String>,
}

impl Exif {
    /// Searches for specified tags within the parsed Exif data, and parses the
    /// corresponding values within the found entries. The final result is
    /// returned in the form of a hash table.
    ///
    /// Please note that this method will ignore errors encountered during the
    /// search and parsing process, such as missing tags or errors in parsing
    /// values, and handle them silently.
    pub fn get_values<'b>(&self, tags: &'b [ExifTag]) -> Vec<(&'b ExifTag, EntryValue)> {
        tags.iter()
            .zip(tags.iter())
            .filter_map(|x| {
                self.get_value(x.0)
                    .map(|v| v.map(|v| (x.0, v)))
                    .unwrap_or(None)
            })
            .collect::<Vec<_>>()
    }

    /// Searches for specified `tag` within the parsed Exif structure, and
    /// parses the corresponding value within the found entry.
    pub fn get_value(&self, tag: &ExifTag) -> crate::Result<Option<EntryValue>> {
        self.get_value_by_tag_code(*tag as u16)
    }

    /// Searches for specified `tag` within the parsed Exif structure, and
    /// parses the corresponding value within the found entry.
    pub fn get_value_by_tag_code(&self, tag: u16) -> crate::Result<Option<EntryValue>> {
        self.ifd0
            .as_ref()
            .and_then(|ifd0| {
                ifd0.find(tag).map(|entry| {
                    EntryValue::parse(entry, self.endian(), &self.tz)
                        .map_err(|_| format!("parse value for exif tag {tag:?} failed").into())
                })
            })
            .transpose()
    }

    fn get_tz_offset(&self) -> Option<String> {
        let values = self.get_values(&[ExifTag::OffsetTimeOriginal, ExifTag::OffsetTime]);
        values.into_iter().find_map(|x| {
            if let EntryValue::Text(s) = x.1 {
                Some(s)
            } else {
                None
            }
        })
    }

    /// Searches and parses the found GPS information within the parsed Exif
    /// structure.
    pub fn get_gps_info(&self) -> crate::Result<Option<GPSInfo>> {
        self.ifd0
            .as_ref()
            .and_then(|ifd0| {
                ifd0.gps_ifd()
                    .map(|gps_ifd| get_gps_info(gps_ifd, self.endian()))
            })
            .transpose()
    }

    fn endian(&self) -> Endianness {
        self.header.endian
    }
}

const ENTRY_SIZE: usize = 12;
const MAX_IFD_DEPTH: usize = 20;

struct Parser<'a> {
    data: &'a [u8],
    endian: Endianness,
}

impl<'a> Parser<'a> {
    #[tracing::instrument(skip(self))]
    fn parse_ifd(&'a self, pos: usize) -> IResult<&'a [u8], Option<ImageFileDirectory>> {
        self.parse_ifd_recursively(pos, 1)
    }

    #[tracing::instrument(skip(self))]
    fn parse_ifd_recursively(
        &'a self,
        pos: usize,
        depth: usize,
    ) -> IResult<&'a [u8], Option<ImageFileDirectory>> {
        // Prevent stack overflow caused by infinite recursion, which will
        // occur when running fuzzing tests.
        if depth > MAX_IFD_DEPTH {
            tracing::error!(?depth, "Too many nested IFDs. Parsing aborted.");
            return fail(&self.data[pos..]); // Safe-slice
        }

        let input = self.data;
        let endian = self.endian;

        let (remain, entry_num) = u16(endian)(&input[pos..])?; // Safe-slice
        if entry_num == 0 {
            return Ok((remain, None));
        }

        // 12 bytes per entry
        let size = (entry_num as usize).checked_mul(ENTRY_SIZE);
        let Some(size) = size else {
            return context("ifd entry num is too big", fail)(remain);
        };
        if remain.len() < size {
            let need = Needed::new(size - remain.len());
            return IResult::Err(nom::Err::Incomplete(need));
        }

        let mut pos = input.len() - remain.len();
        let (remain, entries) =
            many_m_n(entry_num as usize, entry_num as usize, |_: &'a [u8]| {
                let (rem, entry) = self.parse_ifd_entry(pos, depth)?;
                pos = input.len() - rem.len();
                Ok((rem, entry))
            })(input)?;

        let entries = entries
            .into_iter()
            .flatten()
            .map(|x| (x.tag, x))
            .collect::<HashMap<_, _>>();

        Ok((remain, Some(ImageFileDirectory { entries })))
    }

    #[tracing::instrument(skip(self))]
    fn parse_ifd_entry(&self, pos: usize, depth: usize) -> IResult<&[u8], Option<DirectoryEntry>> {
        let input = self.data;
        let endian = self.endian;

        if pos + ENTRY_SIZE > input.len() {
            return Err(nom::Err::Incomplete(Needed::new(
                pos + ENTRY_SIZE - input.len(),
            )));
        }

        let entry_data = &input[pos..pos + ENTRY_SIZE]; // Safe-slice
        let remain = &input[pos + ENTRY_SIZE..];

        let (_, (_, entry)) = map_res(
            tuple((u16(endian), u16(endian), u32(endian), u32(endian))),
            |(tag, data_format, components_num, value_or_offset)| -> IResult<&[u8], Option<DirectoryEntry>> {
                // get component_size according to data format
                let Ok(component_size) = entry_component_size(data_format) else {
                    // tracing::error!(error = ?e, "Parse Exif entry failed.");
                    // return fail(input);
                    return Ok((remain, None))
                };

                // get entry data
                let size = components_num as usize * component_size;
                let data = if size <= 4 {
                    &entry_data[8..8 + size] // Safe-slice
                } else {
                    let start = value_or_offset as usize;
                    let end = start + size;
                    if end > input.len() {
                        // return Err(nom::Err::Incomplete(Needed::new(end - input.len())));
                        return Ok((remain, None));
                    }

                    // Is `start` should be greater than or equal to `pos + ENTRY_SIZE` ?
                    // if start < pos + ENTRY_SIZE {
                    //     return fail(input);
                    // }

                    &input[start..end] // Safe-slice
                };

                let data = Vec::from(data);

                let Ok(subifd) = self.parse_subifd(tag, value_or_offset as usize, depth) else {
                    return Ok((remain, None))
                };

                Ok((remain, Some(DirectoryEntry { // Safe-slice
                    tag,
                    data_format,
                    components_num,
                    data,
                    value: value_or_offset,
                    subifd,
                })))
            },
        )(entry_data)?;

        Ok((remain, entry)) // Safe-slice
    }

    fn parse_subifd(
        &self,
        tag: u16,
        offset: usize,
        depth: usize,
    ) -> Result<Option<ImageFileDirectory>, nom::Err<nom::error::Error<&[u8]>>> {
        let input = self.data;
        let subifd = if tag == ExifTag::ExifOffset as u16 || tag == ExifTag::GPSInfo as u16 {
            if offset > input.len() {
                let need = Needed::new(offset - input.len());
                return Err(nom::Err::Incomplete(need));
            }

            // load from offset
            let (_, ifd) = self.parse_ifd_recursively(offset, depth + 1)?;
            ifd
        } else {
            None
        };
        Ok(subifd)
    }
}

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
    use test_case::test_case;

    use crate::exif::ExifTag::*;
    use crate::exif::{GPSInfo, LatLng};
    use crate::testkit::read_sample;
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

    #[test_case("exif.jpg")]
    fn test_parse_exif(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        tracing::info!(bytes = buf.len(), "File size");

        // skip first 12 bytes
        let exif = parse_exif(&buf[12..]).unwrap(); // Safe-slice in test_case

        let entries = exif.get_values(&[
            Unknown,
            Make,
            Model,
            Orientation,
            ImageWidth,
            ImageHeight,
            ISOSpeedRatings,
            ShutterSpeedValue,
            ExposureTime,
            FNumber,
            ExifImageWidth,
            ExifImageHeight,
            DateTimeOriginal,
            CreateDate,
            ModifyDate,
            OffsetTimeOriginal,
            OffsetTime,
            GPSLatitudeRef,
            GPSLatitude,
            GPSLongitudeRef,
            GPSLongitude,
            GPSAltitudeRef,
            GPSAltitude,
            GPSVersionID,
            ImageDescription,
            XResolution,
            YResolution,
            ResolutionUnit,
            Software,
            HostComputer,
            WhitePoint,
            PrimaryChromaticities,
            YCbCrCoefficients,
            ReferenceBlackWhite,
            Copyright,
        ]);

        assert_eq!(
            {
                let mut x = entries
                    .into_iter()
                    .map(|x| (x.0.to_string(), x.1.to_string()))
                    .collect::<Vec<(String, String)>>();
                // Sort by alphabetical order of keys.
                x.sort_by(|a, b| a.0.cmp(&b.0));
                x
            },
            [
                ("CreateDate(0x9004)", "2023-07-09T20:36:33+08:00"),
                ("DateTimeOriginal(0x9003)", "2023-07-09T20:36:33+08:00"),
                ("ExifImageHeight(0xa003)", "4096"),
                ("ExifImageWidth(0xa002)", "3072"),
                ("ExposureTime(0x829a)", "9997/1000000 (0.0100)"),
                ("FNumber(0x829d)", "175/100 (1.7500)"),
                ("GPSAltitude(0x0006)", "0/1 (0.0000)"),
                ("GPSAltitudeRef(0x0005)", "0"),
                ("GPSLatitude(0x0002)", "22/1 (22.0000)"),
                ("GPSLatitudeRef(0x0001)", "N"),
                ("GPSLongitude(0x0004)", "114/1 (114.0000)"),
                ("GPSLongitudeRef(0x0003)", "E"),
                ("ISOSpeedRatings(0x8827)", "454"),
                ("ImageHeight(0x0101)", "4096"),
                ("ImageWidth(0x0100)", "3072"),
                ("Make(0x010f)", "vivo"),
                ("Model(0x0110)", "vivo X90 Pro+"),
                ("ModifyDate(0x0132)", "2023-07-09T20:36:33+08:00"),
                ("OffsetTime(0x9010)", "+08:00"),
                ("OffsetTimeOriginal(0x9011)", "+08:00"),
                ("ResolutionUnit(0x0128)", "2"),
                ("ShutterSpeedValue(0x9201)", "6644/1000 (6.6440)"),
                ("XResolution(0x011a)", "72/1 (72.0000)"),
                ("YResolution(0x011b)", "72/1 (72.0000)")
            ]
            .iter()
            .map(|x| (x.0.to_string(), x.1.to_string()))
            .collect::<Vec<_>>()
        );
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
        altitude: URational,
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
}

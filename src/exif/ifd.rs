use std::collections::HashMap;
use thiserror::Error;

use nom::{
    combinator::map,
    number::{complete, Endianness},
    sequence::tuple,
};
use std::convert::TryInto;

use crate::{
    exif::gps::LatLng,
    input::AssociatedInput,
    slice::SliceChecked,
    values::{decode_rational, DataFormat, EntryData, IRational, URational},
    EntryValue, ExifTag,
};

use super::{tags::ExifTagCode, GPSInfo};

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("Failed to parse IFD entry; size/offset is overflow")]
    Overflow,

    #[error("Failed to parse IFD entry; invalid data: {0}")]
    InvalidData(String),

    #[error("Failed to parse IFD entry; unsupported: {0}")]
    Unsupported(String),
}

impl From<Error> for crate::Error {
    fn from(value: Error) -> Self {
        Self::InvalidEntry(value.into())
    }
}

/// https://www.media.mit.edu/pia/Research/deepview/exif.html
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParsedImageFileDirectory {
    pub entries: HashMap<u16, ParsedIdfEntry>,
}

impl ParsedImageFileDirectory {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ImageFileDirectoryIter {
    pub ifd_idx: usize,
    pub input: AssociatedInput,
    pub pos: usize,
    pub endian: Endianness,
    pub tz: Option<String>,

    pub num_entries: u16,

    // Iterating status
    pub index: u16,
}

#[derive(Debug)]
pub(crate) enum IfdEntry {
    Ifd { idx: usize, offset: usize }, // ifd index
    Entry(EntryValue),
    Err(Error),
}

impl IfdEntry {
    pub fn as_u8(&self) -> Option<u8> {
        if let IfdEntry::Entry(entry) = self {
            if let EntryValue::U8(v) = entry {
                return Some(*v);
            }
        }
        None
    }

    pub fn as_char(&self) -> Option<char> {
        if let IfdEntry::Entry(entry) = self {
            if let EntryValue::Text(s) = entry {
                return s.chars().next();
            }
        }
        None
    }

    fn as_irational(&self) -> Option<&IRational> {
        match self {
            IfdEntry::Entry(v) => match v {
                EntryValue::IRational(v) => Some(v),
                _ => None,
            },
            _ => None,
        }
    }

    fn as_irational_array(&self) -> Option<&Vec<IRational>> {
        match self {
            IfdEntry::Entry(v) => match v {
                EntryValue::IRationalArray(v) => Some(v),
                _ => None,
            },
            _ => None,
        }
    }

    fn as_urational(&self) -> Option<&URational> {
        match self {
            IfdEntry::Entry(v) => match v {
                EntryValue::URational(v) => Some(v),
                _ => None,
            },
            _ => None,
        }
    }

    fn as_urational_array(&self) -> Option<&Vec<URational>> {
        match self {
            IfdEntry::Entry(v) => match v {
                EntryValue::URationalArray(v) => Some(v),
                _ => None,
            },
            _ => None,
        }
    }
}

const ENTRY_SIZE: usize = 12;
const SUBIFD_TAGS: &[u16] = &[ExifTag::ExifOffset.code(), ExifTag::GPSInfo.code()];
const TZ_OFFSET_TAGS: &[u16] = &[
    ExifTag::OffsetTimeOriginal.code(),
    ExifTag::OffsetTimeDigitized.code(),
    ExifTag::OffsetTime.code(),
];

impl Iterator for ImageFileDirectoryIter {
    type Item = (ExifTagCode, IfdEntry);

    fn next(&mut self) -> Option<Self::Item> {
        let endian = self.endian;
        if self.index >= self.num_entries {
            // next IFD
            let (_, offset) =
                complete::u32::<_, nom::error::Error<_>>(endian)(&self.input[self.pos..]).ok()?;
            let offset = offset as usize;

            if offset == 0 {
                return None;
            } else if offset >= self.input.len() {
                return None;
            } else {
                return Some((
                    ExifTagCode::Tag(ExifTag::Unknown),
                    IfdEntry::Ifd {
                        idx: self.ifd_idx + 1,
                        offset,
                    },
                ));
            }
        }

        let entry_data = self.input.slice_checked(self.pos..self.pos + ENTRY_SIZE)?;
        self.index += 1;
        self.pos += ENTRY_SIZE;

        let (tag, res) = self.parse_tag_entry(entry_data)?;

        Some((tag.into(), res)) // Safe-slice
    }
}

impl ImageFileDirectoryIter {
    pub fn try_new(
        ifd_idx: usize,
        input: AssociatedInput,
        pos: usize,
        endian: Endianness,
        tz: Option<String>,
    ) -> crate::Result<Self> {
        let num_entries = Self::parse_num_entries(endian, &input[pos..])?;
        Ok(Self {
            ifd_idx,
            endian,
            tz,
            num_entries,
            index: 0,
            input,
            pos: pos + 2,
        })
    }

    fn parse_num_entries(endian: Endianness, data: &[u8]) -> crate::Result<u16> {
        let (remain, num) = complete::u16(endian)(data)?; // Safe-slice
        if num == 0 {
            return Ok(num);
        }

        // 12 bytes per entry
        let size = (num as usize).checked_mul(ENTRY_SIZE);
        let Some(size) = size else {
            return Err("ifd entry num is too big".into());
        };
        if size > remain.len() {
            Err("ifd entry num is too big".into())
        } else {
            Ok(num)
        }
    }

    fn parse_tag_entry(&self, entry_data: &[u8]) -> Option<(u16, IfdEntry)> {
        let endian = self.endian;
        let (_, (tag, data_format, components_num, value_or_offset)) = tuple((
            complete::u16::<_, nom::error::Error<_>>(endian),
            complete::u16(endian),
            complete::u32(endian),
            complete::u32(endian),
        ))(entry_data)
        .ok()?;

        let df: DataFormat = match data_format.try_into() {
            Ok(df) => df,
            Err(e) => return Some((tag, IfdEntry::Err(e))),
        };
        let (tag, res) = self.parse_entry(tag, df, components_num, entry_data, value_or_offset);
        Some((tag, res))
    }

    fn parse_entry(
        &self,
        tag: u16,
        data_format: DataFormat,
        components_num: u32,
        entry_data: &[u8],
        value_or_offset: u32,
    ) -> (u16, IfdEntry) {
        // get component_size according to data format
        let component_size = data_format.component_size();

        // get entry data
        let size = components_num as usize * component_size;
        let data = if size <= 4 {
            &entry_data[8..8 + size] // Safe-slice
        } else {
            let start = value_or_offset as usize;
            let end = start + size;
            let Some(data) = self.input.slice_checked(start..end) else {
                return (tag, IfdEntry::Err(Error::Overflow));
            };

            // Is `start` should be greater than or equal to `pos + ENTRY_SIZE` ?

            data
        };

        if SUBIFD_TAGS.contains(&tag) {
            if (value_or_offset as usize) < self.input.len() {
                return (
                    tag,
                    IfdEntry::Ifd {
                        idx: self.ifd_idx,
                        offset: value_or_offset as usize,
                    },
                );
            } else {
                return (tag, IfdEntry::Err(Error::Overflow));
            }
        }

        let entry = EntryData {
            endian: self.endian,
            tag,
            data,
            data_format,
            components_num,
        };
        match EntryValue::parse(&entry, &self.tz) {
            Ok(v) => (tag, IfdEntry::Entry(v)),
            Err(e) => (tag, IfdEntry::Err(e)),
        }
    }

    pub fn find_tz_offset(&self) -> Option<String> {
        let endian = self.endian;
        // find ExifOffset
        for i in 0..self.num_entries {
            let pos = self.pos + i as usize * ENTRY_SIZE;
            let (remain, tag) =
                complete::u16::<_, nom::error::Error<_>>(endian)(&self.input[pos..]).ok()?;
            if tag == ExifTag::ExifOffset.code() {
                let (_, (_, _, offset)) = tuple((
                    complete::u16::<_, nom::error::Error<_>>(endian),
                    complete::u32(endian),
                    complete::u32(endian),
                ))(remain)
                .ok()?;

                // find tz offset
                return self.find_tz_offset_in_exif_subifd(offset as usize);
            }
        }
        None
    }

    fn find_tz_offset_in_exif_subifd(&self, offset: usize) -> Option<String> {
        let num_entries = Self::parse_num_entries(self.endian, &self.input[offset..]).ok()?;
        let pos = offset + 2;
        for i in 0..num_entries {
            let pos = pos + i as usize * ENTRY_SIZE;
            let entry_data = self.input.slice_checked(pos..pos + ENTRY_SIZE)?;
            let (tag, res) = self.parse_tag_entry(entry_data)?;
            if TZ_OFFSET_TAGS.contains(&tag) {
                return match res {
                    IfdEntry::Ifd { idx: _, offset: _ } => unreachable!(),
                    IfdEntry::Entry(v) => match v {
                        EntryValue::Text(v) => Some(v),
                        _ => unreachable!(),
                    },
                    IfdEntry::Err(_) => None,
                };
            }
        }
        None
    }

    // Assume the current ifd is GPSInfo subifd.
    pub fn parse_gps_info(&mut self) -> Option<GPSInfo> {
        let mut gps = GPSInfo::default();
        let mut has_data = false;
        for (tag, entry) in self {
            has_data = true;
            println!("{tag:?}: {entry:?}");
            match tag.tag() {
                ExifTag::GPSLatitudeRef => {
                    if let Some(c) = entry.as_char() {
                        gps.latitude_ref = c;
                    }
                }
                ExifTag::GPSLongitudeRef => {
                    if let Some(c) = entry.as_char() {
                        gps.longitude_ref = c;
                    }
                }
                ExifTag::GPSAltitudeRef => {
                    if let Some(c) = entry.as_u8() {
                        gps.altitude_ref = c;
                    }
                }
                ExifTag::GPSLatitude => {
                    if let Some(v) = entry.as_urational_array() {
                        gps.latitude = v.iter().collect();
                    } else if let Some(v) = entry.as_irational_array() {
                        gps.latitude = v.iter().collect();
                    }
                }
                ExifTag::GPSLongitude => {
                    if let Some(v) = entry.as_urational_array() {
                        gps.longitude = v.iter().collect();
                    } else if let Some(v) = entry.as_irational_array() {
                        gps.longitude = v.iter().collect();
                    }
                }
                ExifTag::GPSAltitude => {
                    if let Some(v) = entry.as_urational() {
                        gps.altitude = (*v).into();
                    } else if let Some(v) = entry.as_irational() {
                        gps.altitude = *v;
                    }
                }
                _ => (),
            }
        }

        if has_data {
            println!("gps: {:?}", gps);
            println!("gps: {}", gps.format_iso6709());
            Some(gps)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParsedIdfEntry {
    pub tag: u16,
    pub value: EntryValue,
    pub subifd: Option<ParsedImageFileDirectory>,
}

impl ParsedImageFileDirectory {
    pub fn find(&self, tag: u16) -> Option<&ParsedIdfEntry> {
        self.entries
            .get(&tag)
            .or_else(|| self.exif_ifd().and_then(|exif_ifd| exif_ifd.find(tag)))
            .or_else(|| self.gps_ifd().and_then(|gps_ifd| gps_ifd.find(tag)))
    }

    /// get exif sub ifd
    pub fn exif_ifd(&self) -> Option<&ParsedImageFileDirectory> {
        self.entries
            .get(&(ExifTag::ExifOffset.code()))
            .and_then(|entry| entry.subifd.as_ref())
    }

    /// get gps sub ifd
    pub fn gps_ifd(&self) -> Option<&ParsedImageFileDirectory> {
        self.entries
            .get(&(ExifTag::GPSInfo.code()))
            .and_then(|entry| entry.subifd.as_ref())
    }

    pub(crate) fn get(&self, tag: u16) -> Option<&EntryValue> {
        self.entries.get(&tag).map(|x| &x.value)
    }

    pub(crate) fn put(&mut self, code: u16, v: EntryValue) {
        self.entries.insert(
            code,
            ParsedIdfEntry {
                tag: code,
                value: v,
                subifd: None,
            },
        );
    }

    pub(crate) fn put_subifd(&mut self, code: u16, ifd: ParsedImageFileDirectory) {
        self.entries.insert(
            code,
            ParsedIdfEntry {
                tag: code,
                value: EntryValue::U32(0),
                subifd: Some(ifd),
            },
        );
    }
}

impl From<chrono::ParseError> for Error {
    fn from(value: chrono::ParseError) -> Self {
        Error::InvalidData(format!("invalid time format: {value}"))
    }
}

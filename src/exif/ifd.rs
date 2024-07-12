use std::{collections::HashMap, fmt::Display};

use chrono::{DateTime, Local, LocalResult, NaiveDateTime, TimeZone};
use nom::number::Endianness;

use crate::{
    exif::{tags::ExifTag, LatLng},
    values::{IRational, URational},
    EntryValue,
};
use std::convert::TryInto;

use super::GPSInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidData(String),
    Unsupported(String),
}

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidData(v) => write!(f, "invalid data format: {v}"),
            Error::Unsupported(v) => write!(f, "unsupported value of ifd entry: {v}"),
        }
    }
}

/// https://www.media.mit.edu/pia/Research/deepview/exif.html
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageFileDirectory {
    pub(crate) entries: HashMap<u16, DirectoryEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectoryEntry {
    pub tag: u16,
    pub data_format: u16,
    pub components_num: u32,
    pub data: Vec<u8>,
    pub value: u32,
    pub subifd: Option<ImageFileDirectory>,
}

impl ImageFileDirectory {
    pub fn find(&self, tag: u16) -> Option<&DirectoryEntry> {
        self.entries
            .get(&tag)
            .and_then(|entry| Some(entry))
            .or_else(|| self.exif_ifd().and_then(|exif_ifd| exif_ifd.find(tag)))
            .or_else(|| self.gps_ifd().and_then(|gps_ifd| gps_ifd.find(tag)))
    }

    /// get exif sub ifd
    pub fn exif_ifd(&self) -> Option<&ImageFileDirectory> {
        self.entries
            .get(&(ExifTag::ExifOffset as u16))
            .and_then(|entry| entry.subifd.as_ref())
    }

    /// get gps sub ifd
    pub fn gps_ifd(&self) -> Option<&ImageFileDirectory> {
        self.entries
            .get(&(ExifTag::GPSInfo as u16))
            .and_then(|entry| entry.subifd.as_ref())
    }
}

impl EntryValue {
    /// Parse an IFD entry value.
    ///
    /// # Structure of IFD Entry
    ///
    /// ```txt
    /// | 2   | 2           | 4              | 4                      |
    /// | tag | data format | components num | data (value or offset) |
    /// ```
    ///
    /// # Data size
    ///
    /// `data_size = components_num * bytes_per_component`
    ///
    /// `bytes_per_component` is determined by tag & data format.
    ///
    /// If data_size > 4, then the data area of entry stores the offset of the
    /// value, not the value itself.
    ///
    /// # Data format
    ///
    /// ```txt
    /// | Value           |             1 |             2 |              3 |               4 |                 5 |            6 |
    /// |-----------------+---------------+---------------+----------------+-----------------+-------------------+--------------|
    /// | Format          | unsigned byte | ascii strings | unsigned short |   unsigned long | unsigned rational |  signed byte |
    /// | Bytes/component |             1 |             1 |              2 |               4 |                 8 |            1 |
    ///
    /// | Value           |             7 |             8 |              9 |              10 |                11 |           12 |
    /// |-----------------+---------------+---------------+----------------+-----------------+-------------------+--------------|
    /// | Format          |     undefined |  signed short |    signed long | signed rational |      single float | double float |
    /// | Bytes/component |             1 |             2 |              4 |               8 |                 4 |            8 |
    /// ```
    ///
    /// See: [Exif](https://www.media.mit.edu/pia/Research/deepview/exif.html).
    pub(crate) fn parse(
        entry: &DirectoryEntry,
        endian: Endianness,
        tz: &Option<String>,
    ) -> Result<EntryValue, Error> {
        if entry.data.is_empty() {
            return Err(Error::Unsupported(
                "invalid DirectoryEntry: entry data is empty".into(),
            ));
        }

        let tag = entry.tag;

        let exif_tag: Result<ExifTag, _> = tag.try_into();
        if let Ok(tag) = exif_tag {
            if tag == ExifTag::ExifOffset || tag == ExifTag::GPSInfo {
                // load from offset
                return Err(Error::Unsupported(format!(
                    "tag {tag} is a sub ifd, not an entry"
                )));
            }

            if tag == ExifTag::DateTimeOriginal
                || tag == ExifTag::CreateDate
                || tag == ExifTag::ModifyDate
            {
                // assert_eq!(entry.data_format, 2);
                if entry.data_format != 2 {
                    return Err(Error::InvalidData(
                        "invalid DirectoryEntry: date format is invalid".into(),
                    ));
                }
                let s = get_cstr(&entry.data).map_err(|e| Error::InvalidData(e.to_string()))?;

                let t = if let Some(tz) = tz {
                    let s = format!("{s} {tz}");
                    DateTime::parse_from_str(&s, "%Y:%m:%d %H:%M:%S %z")?
                } else {
                    let t = NaiveDateTime::parse_from_str(&s, "%Y:%m:%d %H:%M:%S")?;
                    let t = Local.from_local_datetime(&t);
                    let t = if let LocalResult::Single(t) = t {
                        Ok(t)
                    } else {
                        Err(Error::InvalidData(format!("parse time failed: {s}")))
                    }?;

                    t.with_timezone(t.offset())
                };

                return Ok(EntryValue::Time(t));
            }
        }

        match entry.data_format {
            // string
            2 => Ok(EntryValue::Text(
                get_cstr(&entry.data).map_err(|e| Error::InvalidData(e.to_string()))?,
            )),

            // u8
            1 => match entry.components_num {
                0 => Err(Error::InvalidData(
                    "components num should'nt be 0".to_string(),
                )),
                1 => Ok(Self::U32(entry.data[0] as u32)),
                x => Err(Error::Unsupported(format!(
                    "usigned byte with {x} components num"
                ))),
            },
            // u16
            3 => {
                if entry.data.len() < 2 {
                    return Err(Error::InvalidData("invalid DirectoryEntry".into()));
                }
                Ok(Self::U32(bytes_to_u16(&entry.data[..2], endian) as u32)) // Safe-slice
            }
            // u32
            4 => {
                if entry.data.len() < 4 {
                    return Err(Error::InvalidData("invalid DirectoryEntry".into()));
                }
                Ok(Self::U32(bytes_to_u32(&entry.data[..4], endian))) // Safe-slice
            }

            // unsigned rational
            5 => {
                if entry.data.len() < 8 {
                    return Err(Error::InvalidData("invalid DirectoryEntry".into()));
                }
                let numerator = bytes_to_u32(&entry.data[..4], endian); // Safe-slice
                let denominator = bytes_to_u32(&entry.data[4..8], endian); // Safe-slice

                Ok(Self::URational(URational(numerator, denominator)))
            }

            // signed rational
            0xa => {
                if entry.data.len() < 8 {
                    return Err(Error::InvalidData("invalid DirectoryEntry".into()));
                }
                let numerator = bytes_to_i32(&entry.data[..4], endian); // Safe-slice
                let denominator = bytes_to_i32(&entry.data[4..8], endian); // Safe-slice

                Ok(Self::IRational(IRational(numerator, denominator)))
            }

            x => Err(Error::Unsupported(format!("data format {x}"))),
        }
    }
}

use std::string::FromUtf8Error;

fn get_cstr(data: &[u8]) -> std::result::Result<String, FromUtf8Error> {
    String::from_utf8(
        data.iter()
            .take_while(|b| **b != 0)
            .filter(|b| **b != 0)
            .cloned()
            .collect::<Vec<u8>>(),
    )
}

pub fn get_gps_info<'a>(
    gps_ifd: &ImageFileDirectory,
    endian: Endianness,
) -> crate::Result<GPSInfo> {
    fn get_ref(gps_ifd: &ImageFileDirectory, tag: ExifTag) -> crate::Result<char> {
        gps_ifd
            .find(tag as u16)
            .and_then(|entry| entry.data.first().map(|b| *b as char))
            .ok_or("invalid latitude_ref".into())
    }

    let get_latlng = |gps_ifd: &ImageFileDirectory, tag| -> crate::Result<LatLng> {
        Ok(if let Some(entry) = gps_ifd.find(tag as u16) {
            let rationals = decode_urationals(&entry.data, endian)?;
            if rationals.len() < 3 {
                return Err("invalid latitude".into());
            }
            LatLng(rationals[0], rationals[1], rationals[2])
        } else {
            LatLng::default()
        })
    };

    let latitude_ref = get_ref(gps_ifd, ExifTag::GPSLatitudeRef)?;
    let longitude_ref = get_ref(gps_ifd, ExifTag::GPSLongitudeRef)?;

    let latitude = get_latlng(gps_ifd, ExifTag::GPSLatitude)?;
    let longitude = get_latlng(gps_ifd, ExifTag::GPSLongitude)?;

    let altitude_ref = gps_ifd
        .find(ExifTag::GPSAltitudeRef as u16)
        .and_then(|entry| Some(entry.data[0]))
        .unwrap_or(0);

    let altitude = if let Some(entry) = gps_ifd.find(ExifTag::GPSAltitude as u16) {
        decode_urational(&entry.data, endian)?
    } else {
        URational::default()
    };

    Ok(GPSInfo {
        latitude_ref,
        latitude,
        longitude_ref,
        longitude,
        altitude_ref,
        altitude,
    })
}

pub fn decode_urationals(data: &[u8], endian: Endianness) -> crate::Result<Vec<URational>> {
    if data.len() < 8 {
        Err(format!(
            "unsigned rational need 8 bytes, {} bytes given",
            data.len()
        ))?;
    }

    let mut res = Vec::with_capacity(data.len() / 8);
    let mut remain = data;

    loop {
        if remain.len() < 8 {
            break Ok(res);
        }

        let rational = decode_urational(remain, endian)?;
        res.push(rational);

        remain = &remain[8..]; // Safe-slice
    }
}

pub fn decode_urational(remain: &[u8], endian: Endianness) -> crate::Result<URational> {
    if remain.len() < 8 {
        Err(format!(
            "unsigned rational need 8 bytes, {} bytes given",
            remain.len()
        ))?;
    }
    let numerator = bytes_to_u32(&remain[..4], endian); // Safe-slice
    let denominator = bytes_to_u32(&remain[4..8], endian); // Safe-slice

    Ok(URational(numerator, denominator))
}

pub fn entry_component_size(data_format: u16) -> Result<usize, Error> {
    let component_size = match data_format {
        // u8 | string | i8 | undefined
        1 | 2 | 6 | 7 => 1,

        // u16 | i16
        3 | 8 => 2,

        // u32 | i32 | f32
        4 | 9 | 0xb => 4,

        // unsigned rational | signed rational | f64
        5 | 0xa | 0xc => 8,

        x => return Err(Error::Unsupported(format!("data format {x}"))),
    };
    Ok(component_size)
}

fn bytes_to_u32(bs: &[u8], endian: Endianness) -> u32 {
    assert!(bs.len() >= 4);
    match endian {
        Endianness::Big => u32::from_be_bytes(bs[0..4].try_into().unwrap()), // Safe-slice
        Endianness::Little => u32::from_le_bytes(bs[0..4].try_into().unwrap()), // Safe-slice
        Endianness::Native => unimplemented!(),
    }
}

fn bytes_to_i32(bs: &[u8], endian: Endianness) -> i32 {
    assert!(bs.len() >= 4);
    match endian {
        Endianness::Big => i32::from_be_bytes(bs[0..4].try_into().unwrap()), // Safe-slice
        Endianness::Little => i32::from_le_bytes(bs[0..4].try_into().unwrap()), // Safe-slice
        Endianness::Native => unimplemented!(),
    }
}

fn bytes_to_u16(bs: &[u8], endian: Endianness) -> u16 {
    assert!(bs.len() >= 2);
    match endian {
        Endianness::Big => u16::from_be_bytes(bs[0..2].try_into().unwrap()), // Safe-slice
        Endianness::Little => u16::from_le_bytes(bs[0..2].try_into().unwrap()), // Safe-slice
        Endianness::Native => unimplemented!(),
    }
}

impl From<chrono::ParseError> for Error {
    fn from(value: chrono::ParseError) -> Self {
        Error::InvalidData(format!("parse time failed: {value}"))
    }
}

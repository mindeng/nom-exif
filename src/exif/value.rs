use std::fmt::Display;

use nom::number::Endianness;

use crate::exif::{tags::ExifTag, LatLng};
use std::convert::TryInto;

use super::{DirectoryEntry, GPSInfo, ImageFileDirectory};

#[cfg(feature = "serialize")]
use serde::{Deserialize, Serialize};

/// Represent a parsed IFD entry value.
///
/// # Structure of IFD Entry
///
/// | 2   | 2           | 4              | 4                      |
/// | tag | data format | components num | data (value or offset) |
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
/// | Value           |             1 |             2 |              3 |               4 |                 5 |            6 |
/// |-----------------+---------------+---------------+----------------+-----------------+-------------------+--------------|
/// | Format          | unsigned byte | ascii strings | unsigned short |   unsigned long | unsigned rational |  signed byte |
/// | Bytes/component |             1 |             1 |              2 |               4 |                 8 |            1 |
/// | Value           |             7 |             8 |              9 |              10 |                11 |           12 |
/// | Format          |     undefined |  signed short |    signed long | signed rational |      single float | double float |
/// | Bytes/component |             1 |             2 |              4 |               8 |                 4 |            8 |
///
/// See: [Exif](https://www.media.mit.edu/pia/Research/deepview/exif.html).
#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IfdEntryValue {
    Text(String),
    URational(URational),
    IRational(IRational),
    U32(u32),
}

impl Display for IfdEntryValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IfdEntryValue::Text(v) => v.fmt(f),
            IfdEntryValue::URational(v) => {
                write!(f, "{}/{} ({:.04})", v.0, v.1, v.0 as f64 / v.1 as f64)
            }
            IfdEntryValue::IRational(v) => {
                write!(f, "{}/{} ({:.04})", v.0, v.1, v.0 as f64 / v.1 as f64)
            }
            IfdEntryValue::U32(v) => v.fmt(f),
        }
    }
}

impl From<u32> for IfdEntryValue {
    fn from(value: u32) -> Self {
        IfdEntryValue::U32(value)
    }
}

impl From<String> for IfdEntryValue {
    fn from(value: String) -> Self {
        IfdEntryValue::Text(value)
    }
}

impl From<&str> for IfdEntryValue {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl From<(u32, u32)> for IfdEntryValue {
    fn from(value: (u32, u32)) -> Self {
        Self::URational(value.into())
    }
}

impl From<(i32, i32)> for IfdEntryValue {
    fn from(value: (i32, i32)) -> Self {
        Self::IRational(IRational(value.0, value.1))
    }
}

// Enable if parse_values feature is activated, or in test mode.
#[cfg(any(feature = "parse_values", test))]
use std::str::FromStr;

// Enable if parse_values feature is activated, or in test mode.
#[cfg(any(feature = "parse_values", test))]
impl FromStr for IfdEntryValue {
    type Err = Box<dyn std::error::Error>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let (prefix, _) = s
            .find('(')
            .map(|i| s.split_at(i))
            .ok_or_else(|| "invalid IfdEntryValue")?;

        let value = match prefix {
            "URational" => extract_in_parenthesis(s, "URational")
                .map(|x| x.parse::<URational>().map(|v| IfdEntryValue::URational(v)))??,

            "IRational" => extract_in_parenthesis(s, "IRational")
                .map(|x| x.parse::<IRational>().map(|v| IfdEntryValue::IRational(v)))??,

            "U32" => extract_in_parenthesis(s, "U32")
                .map(|x| x.parse::<u32>().map(|v| IfdEntryValue::U32(v)))??,

            "Text" => {
                let inner = extract_in_parenthesis(s, "Text")?;
                inner
                    .trim()
                    .strip_prefix('\"')
                    .and_then(|v| v.strip_suffix('\"'))
                    .map(|v| IfdEntryValue::Text(v.to_string()))
                    .ok_or_else(|| {
                        crate::error::ParseFailed("invalid Text value; quotes are missing".into())
                    })?
            }

            x => format!("invalid IfdEntryValue type: {x}").into(),
        };

        Ok(value)
    }
}

// Enable if parse_values feature is activated, or in test mode.
#[cfg(any(feature = "parse_values", test))]
impl FromStr for URational {
    type Err = Box<dyn std::error::Error>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let values = inner_to_vec::<u32, 2>(s, "URational")?;
        Ok(URational(values[0], values[1]))
    }
}

#[cfg(any(feature = "parse_values", test))]
impl FromStr for IRational {
    type Err = Box<dyn std::error::Error>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let values = inner_to_vec::<i32, 2>(s, "IRational")?;
        Ok(IRational(values[0], values[1]))
    }
}

#[cfg(any(feature = "parse_values", test))]
fn inner_to_vec<T: FromStr + Default + Copy, const N: usize>(
    s: &str,
    token: &str,
) -> Result<[T; N], Box<dyn std::error::Error>>
where
    <T as FromStr>::Err: std::error::Error + 'static,
{
    let inner = extract_in_parenthesis(s, token)?;
    let values = inner
        .split(',')
        .into_iter()
        .map(|x| x.trim())
        .map(|x| x.parse::<T>());
    // .collect::<Vec<_>>();

    let mut result = [T::default(); N];
    let mut last = 0;
    for (i, v) in values.enumerate() {
        result[i] = v?;
        last = i;
    }

    if last != N - 1 {
        return Err("parse URational failed; invalid body".into());
    }
    Ok(result)
}

#[cfg(any(feature = "parse_values", test))]
fn extract_in_parenthesis(s: &str, token: &str) -> crate::Result<String> {
    use regex::Regex;

    let s = s.trim();
    s.strip_prefix(token)
        .and_then(|remain| {
            let re = Regex::new(r"\((?<in_parenthesis>.+)\)").unwrap();
            re.captures(remain)
                .map(|caps| caps["in_parenthesis"].to_string())
        })
        .ok_or_else(|| "extract text in parenthesis failed".into())
}

#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct URational(pub u32, pub u32);

impl Default for URational {
    fn default() -> Self {
        URational(0, 0)
    }
}

impl URational {
    pub fn to_float(&self) -> f64 {
        self.0 as f64 / self.1 as f64
    }
}

impl From<(u32, u32)> for URational {
    fn from(value: (u32, u32)) -> Self {
        Self(value.0, value.1)
    }
}

impl Into<(u32, u32)> for URational {
    fn into(self) -> (u32, u32) {
        (self.0, self.1)
    }
}

#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct IRational(pub i32, pub i32);

impl Default for IRational {
    fn default() -> Self {
        IRational(0, 0)
    }
}

impl From<(i32, i32)> for IRational {
    fn from(value: (i32, i32)) -> Self {
        Self(value.0, value.1)
    }
}

impl Into<(i32, i32)> for IRational {
    fn into(self) -> (i32, i32) {
        (self.0, self.1)
    }
}

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

impl IfdEntryValue {
    pub(crate) fn parse<'a>(
        entry: &DirectoryEntry,
        endian: Endianness,
    ) -> Result<IfdEntryValue, Error> {
        let tag = entry.tag;
        if tag == ExifTag::ExifOffset as u16 || tag == ExifTag::GPSInfo as u16 {
            // load from offset
            return Err(Error::Unsupported(format!(
                "tag {tag} is a sub ifd, not an entry"
            )));
        }
        match entry.data_format {
            // string
            2 => Ok(IfdEntryValue::Text(
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
            3 => Ok(Self::U32(bytes_to_u16(&entry.data[..2], endian) as u32)),
            // u32
            4 => Ok(Self::U32(bytes_to_u32(&entry.data[..4], endian))),

            // unsigned rational
            5 => {
                let numerator = bytes_to_u32(&entry.data[..4], endian);
                let denominator = bytes_to_u32(&entry.data[4..], endian);

                Ok(Self::URational(URational(numerator, denominator)))
            }

            // signed rational
            0xa => {
                let numerator = bytes_to_i32(&entry.data[..4], endian);
                let denominator = bytes_to_i32(&entry.data[4..], endian);

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

        remain = &remain[8..];
    }
}

pub fn decode_urational(remain: &[u8], endian: Endianness) -> crate::Result<URational> {
    if remain.len() < 8 {
        Err(format!(
            "unsigned rational need 8 bytes, {} bytes given",
            remain.len()
        ))?;
    }
    let numerator = bytes_to_u32(&remain[..4], endian);
    let denominator = bytes_to_u32(&remain[4..8], endian);

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
    match endian {
        Endianness::Big => u32::from_be_bytes(bs[0..4].try_into().unwrap()),
        Endianness::Little => u32::from_le_bytes(bs[0..4].try_into().unwrap()),
        Endianness::Native => unimplemented!(),
    }
}

fn bytes_to_i32(bs: &[u8], endian: Endianness) -> i32 {
    match endian {
        Endianness::Big => i32::from_be_bytes(bs[0..4].try_into().unwrap()),
        Endianness::Little => i32::from_le_bytes(bs[0..4].try_into().unwrap()),
        Endianness::Native => unimplemented!(),
    }
}

fn bytes_to_u16(bs: &[u8], endian: Endianness) -> u16 {
    match endian {
        Endianness::Big => u16::from_be_bytes(bs[0..2].try_into().unwrap()),
        Endianness::Little => u16::from_le_bytes(bs[0..2].try_into().unwrap()),
        Endianness::Native => unimplemented!(),
    }
}

#[cfg(test)]
mod tests {
    use std::{fmt::Debug, str::FromStr};

    use super::*;

    #[test]
    fn parse_values() {
        test_parse_t::<URational>(URational(123, 456));
        test_parse_t::<IRational>(IRational(123, 456));

        test_parse_t::<IfdEntryValue>(IfdEntryValue::URational(URational(123, 456)));
        test_parse_t::<IfdEntryValue>(IfdEntryValue::IRational(IRational(123, 456)));

        test_parse_t::<IfdEntryValue>(IfdEntryValue::U32(123456));
        test_parse_t::<IfdEntryValue>(IfdEntryValue::Text("hello, world".into()));
    }

    fn test_parse_t<T: Debug + FromStr + PartialEq>(v: T)
    where
        <T as FromStr>::Err: Debug,
    {
        let s = format!("{v:?}");
        println!("s: {s}");
        assert_eq!(v, s.parse::<T>().unwrap());
    }
}

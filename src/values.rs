use std::{fmt::Display, string::FromUtf8Error};

use chrono::{DateTime, FixedOffset, NaiveDateTime, Offset, Utc};

use nom::{multi::many_m_n, number::Endianness, AsChar};
#[cfg(feature = "json_dump")]
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

use crate::ExifTag;

/// Represent a parsed entry value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EntryValue {
    Text(String),
    URational(URational),
    IRational(IRational),

    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),

    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),

    F32(f32),
    F64(f64),

    Time(DateTime<FixedOffset>),
    NaiveDateTime(NaiveDateTime),
    Undefined(Vec<u8>),

    URationalArray(Vec<URational>),
    IRationalArray(Vec<IRational>),

    U8Array(Vec<u8>),
    U16Array(Vec<u16>),
    U32Array(Vec<u32>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntryData<'a> {
    pub endian: Endianness,
    pub tag: u16,
    pub data: &'a [u8],
    pub data_format: DataFormat,
    pub components_num: u32,
}

#[derive(Debug, Clone, Error)]
pub(crate) enum ParseEntryError {
    #[error("size is too big")]
    EntrySizeTooBig,

    #[error("data is invalid: {0}")]
    InvalidData(String),

    #[error("data format is unsupported (please file a bug): {0}")]
    Unsupported(String),
}

impl From<chrono::ParseError> for ParseEntryError {
    fn from(value: chrono::ParseError) -> Self {
        ParseEntryError::InvalidData(format!("invalid time format: {value}"))
    }
}

use ParseEntryError as Error;

impl EntryData<'_> {
    // Ensure that the returned Vec is not empty.
    fn try_as_rationals<T: TryFromBytes>(&self) -> Result<Vec<Rational<T>>, Error> {
        if self.components_num == 0 {
            return Err(Error::InvalidData("components is 0".to_string()));
        }

        let mut vec = Vec::with_capacity(self.components_num as usize);
        for i in 0..self.components_num {
            let rational = decode_rational::<T>(&self.data[i as usize * 8..], self.endian)?;
            vec.push(rational);
        }
        Ok(vec)
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
    /// See: [`DataFormat`].
    pub(crate) fn parse(entry: &EntryData, tz: &Option<String>) -> Result<EntryValue, Error> {
        if entry.data.is_empty() {
            return Err(Error::InvalidData(
                "invalid DirectoryEntry: entry data is empty".into(),
            ));
        }

        let endian = entry.endian;
        let tag = entry.tag;
        let data_format = entry.data_format;
        let data = entry.data;
        let components_num = entry.components_num;

        if data.is_empty() || components_num == 0 {
            return Ok(EntryValue::variant_default(data_format));
        }

        let exif_tag: Result<ExifTag, _> = tag.try_into();
        if let Ok(tag) = exif_tag {
            if tag == ExifTag::DateTimeOriginal
                || tag == ExifTag::CreateDate
                || tag == ExifTag::ModifyDate
            {
                // assert_eq!(data_format, 2);
                // if data_format != 2 {
                //     return Err(Error::InvalidData(
                //         "invalid DirectoryEntry: date format is invalid".into(),
                //     ));
                // }
                let s = get_cstr(data).map_err(|e| Error::InvalidData(e.to_string()))?;

                let t = if let Some(tz) = tz {
                    let tz = repair_tz_str(tz);
                    let ss = format!("{s} {tz}");
                    match DateTime::parse_from_str(&ss, "%Y:%m:%d %H:%M:%S %z") {
                        Ok(t) => t,
                        Err(_) => return Ok(EntryValue::NaiveDateTime(parse_naive_time(s)?)),
                    }
                } else {
                    return Ok(EntryValue::NaiveDateTime(parse_naive_time(s)?));
                };

                return Ok(EntryValue::Time(t));
            }
        }

        match data_format {
            DataFormat::U8 => match components_num {
                1 => Ok(Self::U8(data[0])),
                _ => Ok(Self::U8Array(data.into())),
            },
            DataFormat::Text => Ok(EntryValue::Text(
                get_cstr(data).map_err(|e| Error::InvalidData(e.to_string()))?,
            )),
            DataFormat::U16 => {
                if components_num == 1 {
                    Ok(Self::U16(u16::try_from_bytes(data, endian)?))
                } else {
                    let (_, v) = many_m_n::<_, _, nom::error::Error<_>, _>(
                        components_num as usize,
                        components_num as usize,
                        nom::number::complete::u16(endian),
                    )(data)
                    .map_err(|e| {
                        ParseEntryError::InvalidData(format!("parse U16Array error: {e:?}"))
                    })?;
                    Ok(Self::U16Array(v))
                }
            }
            DataFormat::U32 => {
                if components_num == 1 {
                    Ok(Self::U32(u32::try_from_bytes(data, endian)?))
                } else {
                    let (_, v) = many_m_n::<_, _, nom::error::Error<_>, _>(
                        components_num as usize,
                        components_num as usize,
                        nom::number::complete::u32(endian),
                    )(data)
                    .map_err(|e| {
                        ParseEntryError::InvalidData(format!("parse U32Array error: {e:?}"))
                    })?;
                    Ok(Self::U32Array(v))
                }
            }
            DataFormat::URational => {
                let rationals = entry.try_as_rationals::<u32>()?;
                if rationals.len() == 1 {
                    Ok(Self::URational(rationals[0]))
                } else {
                    Ok(Self::URationalArray(rationals))
                }
            }
            DataFormat::I8 => match components_num {
                1 => Ok(Self::I8(data[0] as i8)),
                x => Err(Error::Unsupported(format!(
                    "signed byte with {x} components"
                ))),
            },
            DataFormat::Undefined => Ok(Self::Undefined(data.to_vec())),
            DataFormat::I16 => match components_num {
                1 => Ok(Self::I16(i16::try_from_bytes(data, endian)?)),
                x => Err(Error::Unsupported(format!(
                    "signed short with {x} components"
                ))),
            },
            DataFormat::I32 => match components_num {
                1 => Ok(Self::I32(i32::try_from_bytes(data, endian)?)),
                x => Err(Error::Unsupported(format!(
                    "signed long with {x} components"
                ))),
            },
            DataFormat::IRational => {
                let rationals = entry.try_as_rationals::<i32>()?;
                if rationals.len() == 1 {
                    Ok(Self::IRational(rationals[0]))
                } else {
                    Ok(Self::IRationalArray(rationals))
                }
            }
            DataFormat::F32 => match components_num {
                1 => Ok(Self::F32(f32::try_from_bytes(data, endian)?)),
                x => Err(Error::Unsupported(format!("float with {x} components"))),
            },
            DataFormat::F64 => match components_num {
                1 => Ok(Self::F64(f64::try_from_bytes(data, endian)?)),
                x => Err(Error::Unsupported(format!("double with {x} components"))),
            },
        }
    }

    fn variant_default(data_format: DataFormat) -> EntryValue {
        match data_format {
            DataFormat::U8 => Self::U8(0),
            DataFormat::Text => Self::Text(String::default()),
            DataFormat::U16 => Self::U16(0),
            DataFormat::U32 => Self::U32(0),
            DataFormat::URational => Self::URational(URational::default()),
            DataFormat::I8 => Self::I8(0),
            DataFormat::Undefined => Self::Undefined(Vec::default()),
            DataFormat::I16 => Self::I16(0),
            DataFormat::I32 => Self::I32(0),
            DataFormat::IRational => Self::IRational(IRational::default()),
            DataFormat::F32 => Self::F32(0.0),
            DataFormat::F64 => Self::F64(0.0),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            EntryValue::Text(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_time(&self) -> Option<DateTime<FixedOffset>> {
        match self {
            EntryValue::Time(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_u8(&self) -> Option<u8> {
        match self {
            EntryValue::U8(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i8(&self) -> Option<i8> {
        match self {
            EntryValue::I8(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_u16(&self) -> Option<u16> {
        match self {
            EntryValue::U16(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i16(&self) -> Option<i16> {
        match self {
            EntryValue::I16(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            EntryValue::U64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            EntryValue::U32(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            EntryValue::I32(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_urational(&self) -> Option<URational> {
        if let EntryValue::URational(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    pub fn as_irational(&self) -> Option<IRational> {
        if let EntryValue::IRational(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    pub fn as_urational_array(&self) -> Option<&[URational]> {
        if let EntryValue::URationalArray(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_irational_array(&self) -> Option<&[IRational]> {
        if let EntryValue::IRationalArray(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_u8array(&self) -> Option<&[u8]> {
        if let EntryValue::U8Array(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn to_u8array(self) -> Option<Vec<u8>> {
        if let EntryValue::U8Array(v) = self {
            Some(v)
        } else {
            None
        }
    }
}
fn parse_naive_time(s: String) -> Result<NaiveDateTime, ParseEntryError> {
    let t = NaiveDateTime::parse_from_str(&s, "%Y:%m:%d %H:%M:%S")?;
    Ok(t)
}
// fn parse_time_with_local_tz(s: String) -> Result<DateTime<FixedOffset>, ParseEntryError> {
//     let t = NaiveDateTime::parse_from_str(&s, "%Y:%m:%d %H:%M:%S")?;
//     let t = Local.from_local_datetime(&t);
//     let t = if let LocalResult::Single(t) = t {
//         Ok(t)
//     } else {
//         Err(Error::InvalidData(format!("parse time failed: {s}")))
//     }?;
//     Ok(t.with_timezone(t.offset()))
// }

fn repair_tz_str(tz: &str) -> String {
    if let Some(idx) = tz.find(":") {
        if tz[idx..].len() < 3 {
            // Add tailed 0
            return format!("{tz}0");
        }
    }
    tz.into()
}

/// # Exif Data format
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
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(unused)]
pub(crate) enum DataFormat {
    U8 = 1,
    Text = 2,
    U16 = 3,
    U32 = 4,
    URational = 5,
    I8 = 6,
    Undefined = 7,
    I16 = 8,
    I32 = 9,
    IRational = 10,
    F32 = 11,
    F64 = 12,
}

impl DataFormat {
    pub fn component_size(&self) -> usize {
        match self {
            Self::U8 | Self::I8 | Self::Text | Self::Undefined => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::URational | Self::IRational | Self::F64 => 8,
        }
    }
}

impl TryFrom<u16> for DataFormat {
    type Error = Error;
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        if v >= Self::U8 as u16 && v <= Self::F64 as u16 {
            Ok(unsafe { std::mem::transmute::<u16, Self>(v) })
        } else {
            Err(Error::InvalidData(format!("data format {v}")))
        }
    }
}

#[cfg(feature = "json_dump")]
impl Serialize for EntryValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl Display for EntryValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryValue::Text(v) => v.fmt(f),
            EntryValue::URational(v) => {
                format!("{}/{} ({:.04})", v.0, v.1, v.0 as f64 / v.1 as f64).fmt(f)
            }
            EntryValue::IRational(v) => {
                format!("{}/{} ({:.04})", v.0, v.1, v.0 as f64 / v.1 as f64).fmt(f)
            }
            EntryValue::U32(v) => Display::fmt(&v, f),
            EntryValue::U16(v) => Display::fmt(&v, f),
            EntryValue::U64(v) => Display::fmt(&v, f),
            EntryValue::I16(v) => Display::fmt(&v, f),
            EntryValue::I32(v) => Display::fmt(&v, f),
            EntryValue::I64(v) => Display::fmt(&v, f),
            EntryValue::F32(v) => Display::fmt(&v, f),
            EntryValue::F64(v) => Display::fmt(&v, f),
            EntryValue::U8(v) => Display::fmt(&v, f),
            EntryValue::I8(v) => Display::fmt(&v, f),
            EntryValue::Time(v) => Display::fmt(&v.to_rfc3339(), f),
            EntryValue::NaiveDateTime(v) => Display::fmt(&v.format("%Y-%m-%d %H:%M:%S"), f),
            EntryValue::Undefined(v) => {
                // Display up to MAX_DISPLAY_NUM components, and replace the rest with ellipsis
                const MAX_DISPLAY_NUM: usize = 8;
                let s = v
                    .iter()
                    .map(|x| format!("0x{x:02x}"))
                    .take(MAX_DISPLAY_NUM + 1)
                    .enumerate()
                    .map(|(i, x)| {
                        if i >= MAX_DISPLAY_NUM {
                            "...".to_owned()
                        } else {
                            x
                        }
                    })
                    .collect::<Vec<String>>()
                    .join(", ");
                format!("Undefined[{}]", s).fmt(f)
            }
            EntryValue::URationalArray(v) => {
                format!("URationalArray[{}]", rationals_to_string::<u32>(v)).fmt(f)
            }
            EntryValue::IRationalArray(v) => {
                format!("IRationalArray[{}]", rationals_to_string::<i32>(v)).fmt(f)
            }
            EntryValue::U8Array(v) => array_to_string("U8Array", v, f),
            EntryValue::U32Array(v) => array_to_string("U32Array", v, f),
            EntryValue::U16Array(v) => array_to_string("U16Array", v, f),
        }
    }
}

fn array_to_string<T: Display>(
    name: &str,
    v: &[T],
    f: &mut std::fmt::Formatter,
) -> Result<(), std::fmt::Error> {
    format!(
        "{}[{}]",
        name,
        v.iter()
            .map(|x| x.to_string())
            .collect::<Vec<String>>()
            .join(", ")
    )
    .fmt(f)
}

fn rationals_to_string<T>(rationals: &[Rational<T>]) -> String
where
    T: Display + Into<f64> + Copy,
{
    // Display up to MAX_DISPLAY_NUM components, and replace the rest with ellipsis
    const MAX_DISPLAY_NUM: usize = 3;
    rationals
        .iter()
        .map(|x| format!("{}/{} ({:.04})", x.0, x.1, x.0.into() / x.1.into()))
        .take(MAX_DISPLAY_NUM + 1)
        .enumerate()
        .map(|(i, x)| {
            if i >= MAX_DISPLAY_NUM {
                "...".to_owned()
            } else {
                x
            }
        })
        .collect::<Vec<String>>()
        .join(", ")
}

impl From<DateTime<Utc>> for EntryValue {
    fn from(value: DateTime<Utc>) -> Self {
        assert_eq!(value.offset().fix(), FixedOffset::east_opt(0).unwrap());
        EntryValue::Time(value.fixed_offset())
    }
}

impl From<DateTime<FixedOffset>> for EntryValue {
    fn from(value: DateTime<FixedOffset>) -> Self {
        EntryValue::Time(value)
    }
}

impl From<u8> for EntryValue {
    fn from(value: u8) -> Self {
        EntryValue::U8(value)
    }
}
impl From<u16> for EntryValue {
    fn from(value: u16) -> Self {
        EntryValue::U16(value)
    }
}
impl From<u32> for EntryValue {
    fn from(value: u32) -> Self {
        EntryValue::U32(value)
    }
}
impl From<u64> for EntryValue {
    fn from(value: u64) -> Self {
        EntryValue::U64(value)
    }
}

impl From<i8> for EntryValue {
    fn from(value: i8) -> Self {
        EntryValue::I8(value)
    }
}
impl From<i16> for EntryValue {
    fn from(value: i16) -> Self {
        EntryValue::I16(value)
    }
}
impl From<i32> for EntryValue {
    fn from(value: i32) -> Self {
        EntryValue::I32(value)
    }
}
impl From<i64> for EntryValue {
    fn from(value: i64) -> Self {
        EntryValue::I64(value)
    }
}

impl From<f32> for EntryValue {
    fn from(value: f32) -> Self {
        EntryValue::F32(value)
    }
}
impl From<f64> for EntryValue {
    fn from(value: f64) -> Self {
        EntryValue::F64(value)
    }
}

impl From<String> for EntryValue {
    fn from(value: String) -> Self {
        EntryValue::Text(value)
    }
}

impl From<&String> for EntryValue {
    fn from(value: &String) -> Self {
        EntryValue::Text(value.to_owned())
    }
}

impl From<&str> for EntryValue {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl From<(u32, u32)> for EntryValue {
    fn from(value: (u32, u32)) -> Self {
        Self::URational(value.into())
    }
}

impl From<(i32, i32)> for EntryValue {
    fn from(value: (i32, i32)) -> Self {
        Self::IRational((value.0, value.1).into())
    }
}

// #[cfg_attr(feature = "json_dump", derive(Serialize, Deserialize))]
// #[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
// pub struct URational(pub u32, pub u32);

pub type URational = Rational<u32>;
pub type IRational = Rational<i32>;

#[cfg_attr(feature = "json_dump", derive(Serialize, Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Rational<T>(pub T, pub T);

impl<T> Rational<T>
where
    T: Copy + Into<f64>,
{
    pub fn as_float(&self) -> f64 {
        std::convert::Into::<f64>::into(self.0) / std::convert::Into::<f64>::into(self.1)
    }
}

impl<T> From<(T, T)> for Rational<T>
where
    T: Copy,
{
    fn from(value: (T, T)) -> Self {
        Self(value.0, value.1)
    }
}

impl<T> From<Rational<T>> for (T, T)
where
    T: Copy,
{
    fn from(value: Rational<T>) -> Self {
        (value.0, value.1)
    }
}

impl From<IRational> for URational {
    fn from(value: IRational) -> Self {
        Self(value.0 as u32, value.1 as u32)
    }
}

pub(crate) fn get_cstr(data: &[u8]) -> std::result::Result<String, FromUtf8Error> {
    let vec = filter_zero(data);
    if let Ok(s) = String::from_utf8(vec) {
        Ok(s)
    } else {
        Ok(filter_zero(data)
            .into_iter()
            .map(|x| x.as_char())
            .collect::<String>())
    }
}

pub(crate) fn filter_zero(data: &[u8]) -> Vec<u8> {
    data.iter()
        // skip leading zero bytes
        .skip_while(|b| **b == 0)
        // ignore tailing zero bytes, and all bytes after zero bytes
        .take_while(|b| **b != 0)
        .cloned()
        .collect::<Vec<u8>>()
}

pub(crate) trait TryFromBytes: Sized {
    fn try_from_bytes(bs: &[u8], endian: Endianness) -> Result<Self, Error>;
}

impl TryFromBytes for u32 {
    fn try_from_bytes(bs: &[u8], endian: Endianness) -> Result<Self, Error> {
        fn make_err<T>() -> Error {
            Error::InvalidData(format!(
                "data is too small to convert to {}",
                std::any::type_name::<T>(),
            ))
        }
        match endian {
            Endianness::Big => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_be_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Little => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_le_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Native => unimplemented!(),
        }
    }
}

impl TryFromBytes for i32 {
    fn try_from_bytes(bs: &[u8], endian: Endianness) -> Result<Self, Error> {
        fn make_err<T>() -> Error {
            Error::InvalidData(format!(
                "data is too small to convert to {}",
                std::any::type_name::<T>(),
            ))
        }
        match endian {
            Endianness::Big => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_be_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Little => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_le_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Native => unimplemented!(),
        }
    }
}

impl TryFromBytes for u16 {
    fn try_from_bytes(bs: &[u8], endian: Endianness) -> Result<Self, Error> {
        fn make_err<T>() -> Error {
            Error::InvalidData(format!(
                "data is too small to convert to {}",
                std::any::type_name::<T>(),
            ))
        }
        match endian {
            Endianness::Big => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_be_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Little => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_le_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Native => unimplemented!(),
        }
    }
}

impl TryFromBytes for i16 {
    fn try_from_bytes(bs: &[u8], endian: Endianness) -> Result<Self, Error> {
        fn make_err<T>() -> Error {
            Error::InvalidData(format!(
                "data is too small to convert to {}",
                std::any::type_name::<T>(),
            ))
        }
        match endian {
            Endianness::Big => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_be_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Little => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_le_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Native => unimplemented!(),
        }
    }
}

impl TryFromBytes for f32 {
    fn try_from_bytes(bs: &[u8], endian: Endianness) -> Result<Self, Error> {
        fn make_err<T>() -> Error {
            Error::InvalidData(format!(
                "data is too small to convert to {}",
                std::any::type_name::<T>(),
            ))
        }
        match endian {
            Endianness::Big => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_be_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Little => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_le_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Native => unimplemented!(),
        }
    }
}

impl TryFromBytes for f64 {
    fn try_from_bytes(bs: &[u8], endian: Endianness) -> Result<Self, Error> {
        fn make_err<T>() -> Error {
            Error::InvalidData(format!(
                "data is too small to convert to {}",
                std::any::type_name::<T>(),
            ))
        }
        match endian {
            Endianness::Big => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_be_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Little => {
                let (int_bytes, _) = bs
                    .split_at_checked(std::mem::size_of::<Self>())
                    .ok_or_else(make_err::<Self>)?;
                Ok(Self::from_le_bytes(
                    int_bytes.try_into().map_err(|_| make_err::<Self>())?,
                ))
            }
            Endianness::Native => unimplemented!(),
        }
    }
}

pub(crate) fn decode_rational<T: TryFromBytes>(
    data: &[u8],
    endian: Endianness,
) -> Result<Rational<T>, Error> {
    if data.len() < 8 {
        return Err(Error::InvalidData(
            "data is too small to decode a rational".to_string(),
        ));
    }

    let numerator = T::try_from_bytes(data, endian)?;
    let denominator = T::try_from_bytes(&data[4..], endian)?; // Safe-slice
    Ok(Rational::<T>(numerator, denominator))
}

#[cfg(test)]
mod tests {
    use chrono::{Local, NaiveDateTime, TimeZone};

    use super::*;

    #[test]
    fn test_parse_time() {
        let tz = Local::now().format("%:z").to_string();

        let s = format!("2023:07:09 20:36:33 {tz}");
        let t1 = DateTime::parse_from_str(&s, "%Y:%m:%d %H:%M:%S %z").unwrap();

        let s = "2023:07:09 20:36:33";
        let t2 = NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").unwrap();
        let t2 = Local.from_local_datetime(&t2).unwrap();

        let t3 = t2.with_timezone(t2.offset());

        assert_eq!(t1, t2);
        assert_eq!(t1, t3);
    }

    #[test]
    fn test_iso_8601() {
        let s = "2023-11-02T19:58:34+0800";
        let t1 = DateTime::parse_from_str(s, "%+").unwrap();

        let s = "2023-11-02T19:58:34+08:00";
        let t2 = DateTime::parse_from_str(s, "%+").unwrap();

        let s = "2023-11-02T19:58:34.026490+08:00";
        let t3 = DateTime::parse_from_str(s, "%+").unwrap();

        assert_eq!(t1, t2);
        assert!(t3 > t2);
    }
}

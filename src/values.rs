use std::{
    fmt::{Display, LowerHex},
    string::FromUtf8Error,
};

use chrono::{DateTime, FixedOffset, NaiveDateTime};

use nom::{multi::many_m_n, number::Endianness, AsChar, Parser};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize, Serializer};

use crate::{error::EntryError, ExifTag};

/// EXIF datetime value with timezone awareness preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExifDateTime {
    /// Original value carried a timezone (e.g. assembled with `OffsetTimeOriginal`).
    Aware(DateTime<FixedOffset>),
    /// Original value had no timezone (raw `DateTime` tag).
    Naive(NaiveDateTime),
}

impl ExifDateTime {
    /// Returns the timezone-aware form only when the original value carried one.
    pub fn aware(&self) -> Option<DateTime<FixedOffset>> {
        match self {
            ExifDateTime::Aware(dt) => Some(*dt),
            ExifDateTime::Naive(_) => None,
        }
    }

    /// Always returns a `NaiveDateTime` — strips the timezone if present.
    pub fn into_naive(self) -> NaiveDateTime {
        match self {
            ExifDateTime::Aware(dt) => dt.naive_local(),
            ExifDateTime::Naive(ndt) => ndt,
        }
    }

    /// If naive, attaches `fallback`; if already aware, returns the original offset.
    pub fn or_offset(self, fallback: FixedOffset) -> DateTime<FixedOffset> {
        match self {
            ExifDateTime::Aware(dt) => dt,
            ExifDateTime::Naive(ndt) => ndt
                .and_local_timezone(fallback)
                .single()
                .unwrap_or_else(|| ndt.and_utc().with_timezone(&fallback)),
        }
    }
}

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

    DateTime(DateTime<FixedOffset>),
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


impl EntryData<'_> {
    // Ensure that the returned Vec is not empty.
    fn try_as_rationals<T: TryFromBytes + Copy>(&self) -> Result<Vec<Rational<T>>, EntryError> {
        if self.components_num == 0 {
            return Err(EntryError::InvalidShape {
                format: self.data_format as u16,
                count: self.components_num,
            });
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
    pub(crate) fn parse(
        entry: &EntryData,
        tz: &Option<String>,
    ) -> Result<EntryValue, EntryError> {
        if entry.data.is_empty() {
            return Err(EntryError::InvalidShape {
                format: entry.data_format as u16,
                count: entry.components_num,
            });
        }

        let endian = entry.endian;
        let tag = entry.tag;
        let data_format = entry.data_format;
        let data = entry.data;
        let components_num = entry.components_num;

        if data.is_empty() || components_num == 0 {
            return Ok(EntryValue::variant_default(data_format));
        }

        let exif_tag = ExifTag::from_code(tag);
        if let Some(tag) = exif_tag {
            if tag == ExifTag::DateTimeOriginal
                || tag == ExifTag::CreateDate
                || tag == ExifTag::ModifyDate
            {
                let s = get_cstr(data).map_err(|_| EntryError::InvalidValue("invalid utf-8"))?;

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

                return Ok(EntryValue::DateTime(t));
            }
        }

        match data_format {
            DataFormat::U8 => match components_num {
                1 => Ok(Self::U8(data[0])),
                _ => Ok(Self::U8Array(data.into())),
            },
            DataFormat::Text => Ok(EntryValue::Text(
                get_cstr(data).map_err(|_| EntryError::InvalidValue("invalid utf-8"))?,
            )),
            DataFormat::U16 => {
                if components_num == 1 {
                    Ok(Self::U16(u16::try_from_bytes(data, endian)?))
                } else {
                    let (_, v) = many_m_n::<_, nom::error::Error<_>, _>(
                        components_num as usize,
                        components_num as usize,
                        nom::number::complete::u16(endian),
                    ).parse(data)
                    .map_err(|_| EntryError::InvalidShape {
                        format: DataFormat::U16 as u16,
                        count: components_num,
                    })?;
                    Ok(Self::U16Array(v))
                }
            }
            DataFormat::U32 => {
                if components_num == 1 {
                    Ok(Self::U32(u32::try_from_bytes(data, endian)?))
                } else {
                    let (_, v) = many_m_n::<_, nom::error::Error<_>, _>(
                        components_num as usize,
                        components_num as usize,
                        nom::number::complete::u32(endian),
                    ).parse(data)
                    .map_err(|_| EntryError::InvalidShape {
                        format: DataFormat::U32 as u16,
                        count: components_num,
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
                x => Err(EntryError::InvalidShape {
                    format: data_format as u16,
                    count: x,
                }),
            },
            DataFormat::Undefined => Ok(Self::Undefined(data.to_vec())),
            DataFormat::I16 => match components_num {
                1 => Ok(Self::I16(i16::try_from_bytes(data, endian)?)),
                x => Err(EntryError::InvalidShape {
                    format: data_format as u16,
                    count: x,
                }),
            },
            DataFormat::I32 => match components_num {
                1 => Ok(Self::I32(i32::try_from_bytes(data, endian)?)),
                x => Err(EntryError::InvalidShape {
                    format: data_format as u16,
                    count: x,
                }),
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
                x => Err(EntryError::InvalidShape {
                    format: data_format as u16,
                    count: x,
                }),
            },
            DataFormat::F64 => match components_num {
                1 => Ok(Self::F64(f64::try_from_bytes(data, endian)?)),
                x => Err(EntryError::InvalidShape {
                    format: data_format as u16,
                    count: x,
                }),
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

    /// EXIF datetime accessor.
    ///
    /// Returns `Some(ExifDateTime::Aware)` when the parsed value carried a
    /// timezone (e.g. composed with `OffsetTimeOriginal`); returns
    /// `Some(ExifDateTime::Naive)` for tags that ship without timezone info;
    /// returns `None` for non-datetime values.
    ///
    /// ```rust
    /// use nom_exif::*;
    /// use chrono::{DateTime, NaiveDateTime, FixedOffset};
    ///
    /// let dt = DateTime::parse_from_str("2023-07-09T20:36:33+08:00", "%+").unwrap();
    /// let ev = EntryValue::DateTime(dt);
    /// assert!(matches!(ev.as_datetime(), Some(ExifDateTime::Aware(_))));
    ///
    /// let ndt = NaiveDateTime::parse_from_str("2023-07-09T20:36:33", "%Y-%m-%dT%H:%M:%S").unwrap();
    /// let ev = EntryValue::NaiveDateTime(ndt);
    /// assert!(matches!(ev.as_datetime(), Some(ExifDateTime::Naive(_))));
    /// ```
    pub fn as_datetime(&self) -> Option<ExifDateTime> {
        match self {
            EntryValue::DateTime(v) => Some(ExifDateTime::Aware(*v)),
            EntryValue::NaiveDateTime(v) => Some(ExifDateTime::Naive(*v)),
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

    pub fn as_i64(&self) -> Option<i64> {
        if let EntryValue::I64(v) = self { Some(*v) } else { None }
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let EntryValue::F64(v) = self { Some(*v) } else { None }
    }

    /// Widen any integer EntryValue to i64. Returns None for non-integer values
    /// (and for U64 values exceeding i64::MAX).
    pub fn try_as_integer(&self) -> Option<i64> {
        match self {
            EntryValue::U8(v) => Some(*v as i64),
            EntryValue::U16(v) => Some(*v as i64),
            EntryValue::U32(v) => Some(*v as i64),
            EntryValue::U64(v) => i64::try_from(*v).ok(),
            EntryValue::I8(v) => Some(*v as i64),
            EntryValue::I16(v) => Some(*v as i64),
            EntryValue::I32(v) => Some(*v as i64),
            EntryValue::I64(v) => Some(*v),
            _ => None,
        }
    }

    /// Widen any numeric EntryValue (integer / rational / float) to f64.
    /// Rationals with denominator=0 return None.
    pub fn try_as_float(&self) -> Option<f64> {
        match self {
            EntryValue::F32(v) => Some(*v as f64),
            EntryValue::F64(v) => Some(*v),
            EntryValue::URational(v) => v.to_f64(),
            EntryValue::IRational(v) => v.to_f64(),
            v => v.try_as_integer().map(|x| x as f64),
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

    pub fn as_urational_slice(&self) -> Option<&[URational]> {
        if let EntryValue::URationalArray(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_irational_slice(&self) -> Option<&[IRational]> {
        if let EntryValue::IRationalArray(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_u8_slice(&self) -> Option<&[u8]> {
        if let EntryValue::U8Array(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_u16_slice(&self) -> Option<&[u16]> {
        if let EntryValue::U16Array(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_u32_slice(&self) -> Option<&[u32]> {
        if let EntryValue::U32Array(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_undefined(&self) -> Option<&[u8]> {
        if let EntryValue::Undefined(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

// Convert time components to EntryValue
impl From<(NaiveDateTime, Option<FixedOffset>)> for EntryValue {
    fn from(value: (NaiveDateTime, Option<FixedOffset>)) -> Self {
        if let Some(offset) = value.1 {
            EntryValue::DateTime(value.0.and_local_timezone(offset).unwrap())
        } else {
            EntryValue::NaiveDateTime(value.0)
        }
    }
}

fn parse_naive_time(s: String) -> Result<NaiveDateTime, EntryError> {
    NaiveDateTime::parse_from_str(&s, "%Y:%m:%d %H:%M:%S")
        .map_err(|_| EntryError::InvalidValue("invalid time format"))
}

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
    /// On failure, returns the unrecognized format value so call sites can
    /// pair it with the entry's `count` and build a richer `EntryError`.
    type Error = u16;
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        if v >= Self::U8 as u16 && v <= Self::F64 as u16 {
            Ok(unsafe { std::mem::transmute::<u16, Self>(v) })
        } else {
            Err(v)
        }
    }
}

#[cfg(feature = "serde")]
impl Serialize for EntryValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

// impl std::fmt::Debug for EntryValue {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         Display::fmt(self, f)
//     }
// }

impl Display for EntryValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryValue::Text(v) => v.fmt(f),
            EntryValue::URational(v) => format!(
                "{}/{} ({:.04})",
                v.numerator(),
                v.denominator(),
                v.numerator() as f64 / v.denominator() as f64
            )
            .fmt(f),
            EntryValue::IRational(v) => format!(
                "{}/{} ({:.04})",
                v.numerator(),
                v.denominator(),
                v.numerator() as f64 / v.denominator() as f64
            )
            .fmt(f),
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
            EntryValue::DateTime(v) => Display::fmt(&v.to_rfc3339(), f),
            EntryValue::NaiveDateTime(v) => Display::fmt(&v.format("%Y-%m-%d %H:%M:%S"), f),
            EntryValue::Undefined(v) => fmt_array_to_string("Undefined", v, f),
            EntryValue::URationalArray(v) => {
                format!("URationalArray[{}]", rationals_to_string::<u32>(v)).fmt(f)
            }
            EntryValue::IRationalArray(v) => {
                format!("IRationalArray[{}]", rationals_to_string::<i32>(v)).fmt(f)
            }
            EntryValue::U8Array(v) => fmt_array_to_string("U8Array", v, f),
            EntryValue::U32Array(v) => fmt_array_to_string("U32Array", v, f),
            EntryValue::U16Array(v) => fmt_array_to_string("U16Array", v, f),
        }
    }
}

pub(crate) fn fmt_array_to_string<T: Display + LowerHex>(
    name: &str,
    v: &[T],
    f: &mut std::fmt::Formatter,
) -> Result<(), std::fmt::Error> {
    array_to_string(name, v).fmt(f)
    // format!(
    //     "{}[{}]",
    //     name,
    //     v.iter()
    //         .map(|x| x.to_string())
    //         .collect::<Vec<String>>()
    //         .join(", ")
    // )
    // .fmt(f)
}

pub(crate) fn array_to_string<T: Display + LowerHex>(name: &str, v: &[T]) -> String {
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
    format!("{}[{}]", name, s)
}

fn rationals_to_string<T>(rationals: &[Rational<T>]) -> String
where
    T: Display + Into<f64> + Copy,
{
    // Display up to MAX_DISPLAY_NUM components, and replace the rest with ellipsis
    const MAX_DISPLAY_NUM: usize = 3;
    rationals
        .iter()
        .map(|x| {
            format!(
                "{}/{} ({:.04})",
                x.numerator(),
                x.denominator(),
                x.numerator().into() / x.denominator().into()
            )
        })
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

impl From<DateTime<FixedOffset>> for EntryValue {
    fn from(value: DateTime<FixedOffset>) -> Self {
        EntryValue::DateTime(value)
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

// #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
// #[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
// pub struct URational(pub u32, pub u32);

pub type URational = Rational<u32>;
pub type IRational = Rational<i32>;

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Rational<T> {
    numerator: T,
    denominator: T,
}

impl<T: Copy> Rational<T> {
    pub const fn new(numerator: T, denominator: T) -> Self {
        Self { numerator, denominator }
    }

    pub const fn numerator(&self) -> T {
        self.numerator
    }

    pub const fn denominator(&self) -> T {
        self.denominator
    }
}

impl<T: Copy + Into<f64> + PartialEq + Default> Rational<T> {
    /// Returns `None` if the denominator is zero.
    #[allow(clippy::wrong_self_convention)]
    pub fn to_f64(&self) -> Option<f64> {
        if self.denominator == T::default() {
            None
        } else {
            Some(self.numerator.into() / self.denominator.into())
        }
    }
}

impl<T: Copy> From<(T, T)> for Rational<T> {
    fn from(value: (T, T)) -> Self {
        Self::new(value.0, value.1)
    }
}

impl<T: Copy> From<Rational<T>> for (T, T) {
    fn from(value: Rational<T>) -> Self {
        (value.numerator, value.denominator)
    }
}

impl TryFrom<IRational> for URational {
    type Error = crate::ConvertError;
    fn try_from(value: IRational) -> Result<Self, Self::Error> {
        let n = value.numerator();
        let d = value.denominator();
        if n < 0 || d < 0 {
            Err(crate::ConvertError::NegativeRational)
        } else {
            Ok(URational::new(n as u32, d as u32))
        }
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
    fn try_from_bytes(bs: &[u8], endian: Endianness) -> Result<Self, EntryError>;
}

macro_rules! impl_try_from_bytes {
    ($type:ty) => {
        impl TryFromBytes for $type {
            fn try_from_bytes(bs: &[u8], endian: Endianness) -> Result<Self, EntryError> {
                fn make_err<T>(available: usize) -> EntryError {
                    EntryError::Truncated {
                        needed: std::mem::size_of::<T>(),
                        available,
                    }
                }
                match endian {
                    Endianness::Big => {
                        let (int_bytes, _) = bs
                            .split_at_checked(std::mem::size_of::<Self>())
                            .ok_or_else(|| make_err::<Self>(bs.len()))?;
                        Ok(Self::from_be_bytes(
                            int_bytes.try_into().map_err(|_| make_err::<Self>(bs.len()))?,
                        ))
                    }
                    Endianness::Little => {
                        let (int_bytes, _) = bs
                            .split_at_checked(std::mem::size_of::<Self>())
                            .ok_or_else(|| make_err::<Self>(bs.len()))?;
                        Ok(Self::from_le_bytes(
                            int_bytes.try_into().map_err(|_| make_err::<Self>(bs.len()))?,
                        ))
                    }
                    Endianness::Native => unimplemented!(),
                }
            }
        }
    };
}

impl_try_from_bytes!(u32);
impl_try_from_bytes!(i32);
impl_try_from_bytes!(u16);
impl_try_from_bytes!(i16);
impl_try_from_bytes!(f32);
impl_try_from_bytes!(f64);

pub(crate) fn decode_rational<T: TryFromBytes + Copy>(
    data: &[u8],
    endian: Endianness,
) -> Result<Rational<T>, EntryError> {
    if data.len() < 8 {
        return Err(EntryError::Truncated {
            needed: 8,
            available: data.len(),
        });
    }

    let numerator = T::try_from_bytes(data, endian)?;
    let denominator = T::try_from_bytes(&data[4..], endian)?; // Safe-slice
    Ok(Rational::<T>::new(numerator, denominator))
}

#[cfg(test)]
mod tests {
    use chrono::{Local, NaiveDateTime, TimeZone};

    use super::*;

    #[test]
    fn test_parse_time() {
        let s = "2023:07:09 20:36:33";
        let t1 = NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").unwrap();
        let t1 = Local.from_local_datetime(&t1).unwrap();

        let tz = t1.format("%:z").to_string();

        let s = format!("2023:07:09 20:36:33 {tz}");
        let t2 = DateTime::parse_from_str(&s, "%Y:%m:%d %H:%M:%S %z").unwrap();

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

    #[test]
    fn test_date_time_components() {
        let dt = DateTime::parse_from_str("2023-07-09T20:36:33+08:00", "%+").unwrap();
        let ndt =
            NaiveDateTime::parse_from_str("2023-07-09T20:36:33", "%Y-%m-%dT%H:%M:%S").unwrap();
        let offset = FixedOffset::east_opt(8 * 3600).unwrap();

        let ev = EntryValue::DateTime(dt);
        let edt = ev.as_datetime().unwrap();
        assert_eq!(edt.aware(), Some(dt));
        assert_eq!(edt.into_naive(), ndt);
        assert_eq!(edt.or_offset(FixedOffset::east_opt(0).unwrap()), dt);

        let ev = EntryValue::NaiveDateTime(ndt);
        let edt = ev.as_datetime().unwrap();
        assert_eq!(edt.aware(), None);
        assert_eq!(edt.into_naive(), ndt);
        assert_eq!(edt.or_offset(offset), dt);
    }

    #[test]
    fn rational_to_f64_normal() {
        let r = URational::new(1, 2);
        assert_eq!(r.numerator(), 1);
        assert_eq!(r.denominator(), 2);
        assert_eq!(r.to_f64(), Some(0.5));
    }

    #[test]
    fn rational_to_f64_zero_denominator() {
        let r = URational::new(1, 0);
        assert_eq!(r.to_f64(), None);

        let r = IRational::new(-1, 0);
        assert_eq!(r.to_f64(), None);
    }

    #[test]
    fn rational_default() {
        let r = URational::default();
        assert_eq!(r.numerator(), 0);
        assert_eq!(r.denominator(), 0);
    }

    #[test]
    fn irational_to_urational_positive() {
        let i = IRational::new(3, 4);
        let u: URational = i.try_into().unwrap();
        assert_eq!(u, URational::new(3, 4));
    }

    #[test]
    fn irational_to_urational_negative_numerator() {
        let i = IRational::new(-3, 4);
        let err = URational::try_from(i).unwrap_err();
        assert!(matches!(err, crate::ConvertError::NegativeRational));
    }

    #[test]
    fn irational_to_urational_negative_denominator() {
        let i = IRational::new(3, -4);
        let err = URational::try_from(i).unwrap_err();
        assert!(matches!(err, crate::ConvertError::NegativeRational));
    }

    #[test]
    fn entry_value_as_i64_f64() {
        assert_eq!(EntryValue::I64(-7).as_i64(), Some(-7));
        assert_eq!(EntryValue::F64(2.5).as_f64(), Some(2.5));
        assert_eq!(EntryValue::I32(7).as_i64(), None);
        assert_eq!(EntryValue::F32(2.5).as_f64(), None);
    }

    #[test]
    fn entry_value_try_as_integer() {
        assert_eq!(EntryValue::U8(7).try_as_integer(), Some(7));
        assert_eq!(EntryValue::U32(0xffff_ffff).try_as_integer(), Some(0xffff_ffff_i64));
        assert_eq!(EntryValue::I32(-7).try_as_integer(), Some(-7));
        assert_eq!(EntryValue::U64(u64::MAX).try_as_integer(), None);
        assert_eq!(EntryValue::Text("x".into()).try_as_integer(), None);
    }

    #[test]
    fn entry_value_try_as_float() {
        assert_eq!(EntryValue::U8(7).try_as_float(), Some(7.0));
        assert_eq!(EntryValue::F32(1.5).try_as_float(), Some(1.5));
        assert_eq!(EntryValue::URational(URational::new(1, 2)).try_as_float(), Some(0.5));
        assert_eq!(EntryValue::URational(URational::new(1, 0)).try_as_float(), None);
        assert_eq!(EntryValue::Text("x".into()).try_as_float(), None);
    }

    #[test]
    fn entry_value_slice_accessors() {
        assert_eq!(EntryValue::U8Array(vec![1, 2]).as_u8_slice(), Some(&[1u8, 2][..]));
        assert_eq!(EntryValue::U16Array(vec![1, 2]).as_u16_slice(), Some(&[1u16, 2][..]));
        assert_eq!(EntryValue::U32Array(vec![1, 2]).as_u32_slice(), Some(&[1u32, 2][..]));
        assert_eq!(EntryValue::Undefined(vec![1, 2]).as_undefined(), Some(&[1u8, 2][..]));
        let r = URational::new(1, 2);
        assert_eq!(EntryValue::URationalArray(vec![r]).as_urational_slice(), Some(&[r][..]));
    }
}

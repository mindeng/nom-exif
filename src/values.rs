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
    pub(crate) fn parse(entry: &EntryData, tz: &Option<String>) -> Result<EntryValue, EntryError> {
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
                    )
                    .parse(data)
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
                    )
                    .parse(data)
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
        if let EntryValue::I64(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let EntryValue::F64(v) = self {
            Some(*v)
        } else {
            None
        }
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
    /// Structured per-variant serialization. Numeric variants serialize as
    /// JSON numbers, [`EntryValue::Text`] / [`EntryValue::DateTime`] /
    /// [`EntryValue::NaiveDateTime`] as strings, rationals as
    /// `{"numerator", "denominator"}` objects (and arrays thereof),
    /// [`EntryValue::Undefined`] as a continuous lowercase hex string with no
    /// truncation, and integer arrays as JSON arrays of numbers.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        match self {
            EntryValue::Text(s) => serializer.serialize_str(s),
            EntryValue::URational(r) => r.serialize(serializer),
            EntryValue::IRational(r) => r.serialize(serializer),
            EntryValue::U8(v) => serializer.serialize_u8(*v),
            EntryValue::U16(v) => serializer.serialize_u16(*v),
            EntryValue::U32(v) => serializer.serialize_u32(*v),
            EntryValue::U64(v) => serializer.serialize_u64(*v),
            EntryValue::I8(v) => serializer.serialize_i8(*v),
            EntryValue::I16(v) => serializer.serialize_i16(*v),
            EntryValue::I32(v) => serializer.serialize_i32(*v),
            EntryValue::I64(v) => serializer.serialize_i64(*v),
            EntryValue::F32(v) => serializer.serialize_f32(*v),
            EntryValue::F64(v) => serializer.serialize_f64(*v),
            EntryValue::DateTime(t) => serializer.serialize_str(&t.to_rfc3339()),
            EntryValue::NaiveDateTime(t) => {
                serializer.serialize_str(&t.format("%Y-%m-%d %H:%M:%S").to_string())
            }
            EntryValue::Undefined(bytes) => {
                let mut hex = String::with_capacity(bytes.len() * 2);
                for b in bytes {
                    use std::fmt::Write;
                    let _ = write!(&mut hex, "{b:02x}");
                }
                serializer.serialize_str(&hex)
            }
            EntryValue::URationalArray(v) => {
                let mut seq = serializer.serialize_seq(Some(v.len()))?;
                for r in v {
                    seq.serialize_element(r)?;
                }
                seq.end()
            }
            EntryValue::IRationalArray(v) => {
                let mut seq = serializer.serialize_seq(Some(v.len()))?;
                for r in v {
                    seq.serialize_element(r)?;
                }
                seq.end()
            }
            EntryValue::U8Array(v) => v.serialize(serializer),
            EntryValue::U16Array(v) => v.serialize(serializer),
            EntryValue::U32Array(v) => v.serialize(serializer),
        }
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
            EntryValue::Undefined(v) => fmt_undefined(v, f),
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
}

pub(crate) fn array_to_string<T: Display + LowerHex>(name: &str, v: &[T]) -> String {
    let s = v
        .iter()
        .map(|x| format!("0x{x:02x}"))
        .collect::<Vec<String>>()
        .join(", ");
    format!("{name}[{s}]")
}

fn rationals_to_string<T>(rationals: &[Rational<T>]) -> String
where
    T: Display + Into<f64> + Copy,
{
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
        .collect::<Vec<String>>()
        .join(", ")
}

/// Render `EntryValue::Undefined` for human display.
///
/// All bytes printable ASCII (`0x20..=0x7E`) → quoted text, e.g. `"0220"`.
/// Otherwise → continuous lowercase hex prefixed with `0x`, e.g. `0x01020300`.
/// Empty → `0x`. The lossy `Undefined[0xNN, 0xNN, ..., ...]` rendering with
/// the 9-element ellipsis cap from earlier versions is gone — callers that
/// need a length cap should impose it at their layer.
fn fmt_undefined(v: &[u8], f: &mut std::fmt::Formatter) -> std::fmt::Result {
    if !v.is_empty() && v.iter().all(|b| (0x20..=0x7e).contains(b)) {
        let s = std::str::from_utf8(v).expect("ASCII subset is valid UTF-8");
        write!(f, "\"{s}\"")
    } else {
        f.write_str("0x")?;
        for b in v {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
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
        Self {
            numerator,
            denominator,
        }
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
                            int_bytes
                                .try_into()
                                .map_err(|_| make_err::<Self>(bs.len()))?,
                        ))
                    }
                    Endianness::Little => {
                        let (int_bytes, _) = bs
                            .split_at_checked(std::mem::size_of::<Self>())
                            .ok_or_else(|| make_err::<Self>(bs.len()))?;
                        Ok(Self::from_le_bytes(
                            int_bytes
                                .try_into()
                                .map_err(|_| make_err::<Self>(bs.len()))?,
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
        assert_eq!(
            EntryValue::U32(0xffff_ffff).try_as_integer(),
            Some(0xffff_ffff_i64)
        );
        assert_eq!(EntryValue::I32(-7).try_as_integer(), Some(-7));
        assert_eq!(EntryValue::U64(u64::MAX).try_as_integer(), None);
        assert_eq!(EntryValue::Text("x".into()).try_as_integer(), None);
    }

    #[test]
    fn entry_value_try_as_float() {
        assert_eq!(EntryValue::U8(7).try_as_float(), Some(7.0));
        assert_eq!(EntryValue::F32(1.5).try_as_float(), Some(1.5));
        assert_eq!(
            EntryValue::URational(URational::new(1, 2)).try_as_float(),
            Some(0.5)
        );
        assert_eq!(
            EntryValue::URational(URational::new(1, 0)).try_as_float(),
            None
        );
        assert_eq!(EntryValue::Text("x".into()).try_as_float(), None);
    }

    #[test]
    fn entry_value_slice_accessors() {
        assert_eq!(
            EntryValue::U8Array(vec![1, 2]).as_u8_slice(),
            Some(&[1u8, 2][..])
        );
        assert_eq!(
            EntryValue::U16Array(vec![1, 2]).as_u16_slice(),
            Some(&[1u16, 2][..])
        );
        assert_eq!(
            EntryValue::U32Array(vec![1, 2]).as_u32_slice(),
            Some(&[1u32, 2][..])
        );
        assert_eq!(
            EntryValue::Undefined(vec![1, 2]).as_undefined(),
            Some(&[1u8, 2][..])
        );
        let r = URational::new(1, 2);
        assert_eq!(
            EntryValue::URationalArray(vec![r]).as_urational_slice(),
            Some(&[r][..])
        );
    }

    #[test]
    fn entry_parse_invalid_shape_for_each_format() {
        // Each non-array variant returns InvalidShape when components_num
        // doesn't match the format constraints (covers lines 195-197,
        // 212-214, 226-228, 234-236, 241-243, 256-258, 263-265).
        use crate::error::EntryError;

        // Note: for U16/U32 with count=1 and short data, the single-component
        // branch goes through try_from_bytes and yields Truncated, not
        // InvalidShape. To hit the InvalidShape arm in the many_m_n path
        // (lines 195-197 and 212-214) we pass count=2 with too few bytes for
        // 2 components but enough so the slice itself isn't empty.
        let cases: &[(DataFormat, &[u8], u32)] = &[
            (DataFormat::U16, &[0u8, 0], 2),
            (DataFormat::U32, &[0u8; 4], 2),
            (DataFormat::I8, &[0u8, 0], 2),
            (DataFormat::I16, &[0u8, 0], 2),
            (DataFormat::I32, &[0u8; 4], 2),
            (DataFormat::F32, &[0u8; 4], 2),
            (DataFormat::F64, &[0u8; 8], 2),
        ];
        for (fmt, data, count) in cases {
            let entry = EntryData {
                tag: 0,
                endian: Endianness::Little,
                data,
                data_format: *fmt,
                components_num: *count,
            };
            let err = EntryValue::parse(&entry, &None).unwrap_err();
            assert!(
                matches!(err, EntryError::InvalidShape { .. }),
                "{fmt:?} should yield InvalidShape, got {err:?}"
            );
        }
    }

    #[test]
    fn entry_parse_variant_default_for_each_format() {
        // Drive variant_default for every DataFormat variant by passing
        // components_num=0 with non-empty data (covers lines 149-151 and
        // the matching arms in variant_default at 273-288).
        let formats: &[(DataFormat, fn(&EntryValue) -> bool)] = &[
            (DataFormat::U8, |v| matches!(v, EntryValue::U8(0))),
            (
                DataFormat::Text,
                |v| matches!(v, EntryValue::Text(s) if s.is_empty()),
            ),
            (DataFormat::U16, |v| matches!(v, EntryValue::U16(0))),
            (DataFormat::U32, |v| matches!(v, EntryValue::U32(0))),
            (
                DataFormat::URational,
                |v| matches!(v, EntryValue::URational(r) if r.numerator() == 0 && r.denominator() == 0),
            ),
            (DataFormat::I8, |v| matches!(v, EntryValue::I8(0))),
            (
                DataFormat::Undefined,
                |v| matches!(v, EntryValue::Undefined(d) if d.is_empty()),
            ),
            (DataFormat::I16, |v| matches!(v, EntryValue::I16(0))),
            (DataFormat::I32, |v| matches!(v, EntryValue::I32(0))),
            (DataFormat::IRational, |v| {
                matches!(v, EntryValue::IRational(_))
            }),
            (DataFormat::F32, |v| matches!(v, EntryValue::F32(_))),
            (DataFormat::F64, |v| matches!(v, EntryValue::F64(_))),
        ];
        for (fmt, check) in formats {
            let entry = EntryData {
                tag: 0,
                endian: Endianness::Little,
                data: &[0u8],
                data_format: *fmt,
                components_num: 0,
            };
            let v = EntryValue::parse(&entry, &None).unwrap();
            assert!(check(&v), "variant_default for {fmt:?} returned {v:?}");
        }
    }

    #[test]
    fn entry_urational_truncated_data_errors() {
        // URational format needs 8 bytes per component; passing 1 byte with
        // components_num=1 should error (drives the rational decode path
        // through to an error result — covers parts of try_as_rationals).
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[0u8; 1],
            data_format: DataFormat::URational,
            components_num: 1,
        };
        let res = EntryValue::parse(&entry, &None);
        assert!(res.is_err(), "URational with truncated data should error");
    }

    #[test]
    fn entry_value_accessor_none_arms() {
        // Cover the `_ => None` arms in the various as_* accessors.
        let v = EntryValue::U16(5);
        assert!(v.as_str().is_none());
        assert!(v.as_datetime().is_none());
        assert!(v.as_u8().is_none());
    }

    #[test]
    fn entry_value_display_for_each_variant() {
        // Drive Display::fmt for every variant (covers lines 631-672 and
        // the helper fmt_array_to_string / rationals_to_string).
        assert_eq!(format!("{}", EntryValue::Text("hi".into())), "hi");
        assert_eq!(
            format!("{}", EntryValue::URational(URational::new(1, 2))),
            "1/2 (0.5000)"
        );
        assert_eq!(
            format!("{}", EntryValue::IRational(IRational::new(-1, 2))),
            "-1/2 (-0.5000)"
        );
        assert_eq!(format!("{}", EntryValue::U8(8)), "8");
        assert_eq!(format!("{}", EntryValue::U16(16)), "16");
        assert_eq!(format!("{}", EntryValue::U32(32)), "32");
        assert_eq!(format!("{}", EntryValue::U64(64)), "64");
        assert_eq!(format!("{}", EntryValue::I8(-8)), "-8");
        assert_eq!(format!("{}", EntryValue::I16(-16)), "-16");
        assert_eq!(format!("{}", EntryValue::I32(-32)), "-32");
        assert_eq!(format!("{}", EntryValue::I64(-64)), "-64");
        assert_eq!(format!("{}", EntryValue::F32(1.5)), "1.5");
        assert_eq!(format!("{}", EntryValue::F64(2.5)), "2.5");

        // DateTime / NaiveDateTime
        let ndt =
            NaiveDateTime::parse_from_str("2024-01-02 03:04:05", "%Y-%m-%d %H:%M:%S").unwrap();
        let dt = ndt
            .and_local_timezone(FixedOffset::east_opt(0).unwrap())
            .unwrap();
        assert!(format!("{}", EntryValue::DateTime(dt)).contains("2024-01-02"));
        assert_eq!(
            format!("{}", EntryValue::NaiveDateTime(ndt)),
            "2024-01-02 03:04:05"
        );

        // fmt_undefined: printable ASCII → quoted, non-printable → 0xhex.
        assert_eq!(
            format!("{}", EntryValue::Undefined(b"0220".to_vec())),
            "\"0220\""
        );
        assert_eq!(
            format!("{}", EntryValue::Undefined(vec![0x01, 0x02, 0x03])),
            "0x010203"
        );
        assert_eq!(format!("{}", EntryValue::Undefined(vec![])), "0x");

        // Arrays
        let s = format!(
            "{}",
            EntryValue::URationalArray(vec![URational::new(1, 2), URational::new(3, 4)])
        );
        assert!(s.starts_with("URationalArray["));
        let s = format!(
            "{}",
            EntryValue::IRationalArray(vec![IRational::new(-1, 2)])
        );
        assert!(s.starts_with("IRationalArray["));
        let s = format!("{}", EntryValue::U8Array(vec![1, 2, 3]));
        assert!(s.starts_with("U8Array"));
        let s = format!("{}", EntryValue::U16Array(vec![1, 2]));
        assert!(s.starts_with("U16Array"));
        let s = format!("{}", EntryValue::U32Array(vec![1, 2]));
        assert!(s.starts_with("U32Array"));
    }

    #[test]
    fn entry_value_from_impls() {
        // Cover all From<numeric> / From<String> / From<&str> /
        // From<DateTime<FixedOffset>> impls (lines 730-799).
        assert_eq!(EntryValue::from(1u8), EntryValue::U8(1));
        assert_eq!(EntryValue::from(1u16), EntryValue::U16(1));
        assert_eq!(EntryValue::from(1u32), EntryValue::U32(1));
        assert_eq!(EntryValue::from(1u64), EntryValue::U64(1));
        assert_eq!(EntryValue::from(-1i8), EntryValue::I8(-1));
        assert_eq!(EntryValue::from(-1i16), EntryValue::I16(-1));
        assert_eq!(EntryValue::from(-1i32), EntryValue::I32(-1));
        assert_eq!(EntryValue::from(-1i64), EntryValue::I64(-1));
        assert_eq!(EntryValue::from(1.5f32), EntryValue::F32(1.5));
        assert_eq!(EntryValue::from(1.5f64), EntryValue::F64(1.5));
        assert_eq!(
            EntryValue::from(String::from("abc")),
            EntryValue::Text("abc".into())
        );
        assert_eq!(EntryValue::from("abc"), EntryValue::Text("abc".into()));

        let ndt =
            NaiveDateTime::parse_from_str("2024-01-02 03:04:05", "%Y-%m-%d %H:%M:%S").unwrap();
        let dt = ndt
            .and_local_timezone(FixedOffset::east_opt(0).unwrap())
            .unwrap();
        assert_eq!(EntryValue::from(dt), EntryValue::DateTime(dt));
    }

    #[test]
    fn data_format_component_size_and_try_from() {
        // Cover DataFormat::component_size (lines 542-549) and the
        // TryFrom<u16> impl (lines 552-562) including the error arm.
        assert_eq!(DataFormat::U8.component_size(), 1);
        assert_eq!(DataFormat::I8.component_size(), 1);
        assert_eq!(DataFormat::Text.component_size(), 1);
        assert_eq!(DataFormat::Undefined.component_size(), 1);
        assert_eq!(DataFormat::U16.component_size(), 2);
        assert_eq!(DataFormat::I16.component_size(), 2);
        assert_eq!(DataFormat::U32.component_size(), 4);
        assert_eq!(DataFormat::I32.component_size(), 4);
        assert_eq!(DataFormat::F32.component_size(), 4);
        assert_eq!(DataFormat::URational.component_size(), 8);
        assert_eq!(DataFormat::IRational.component_size(), 8);
        assert_eq!(DataFormat::F64.component_size(), 8);

        for code in 1u16..=12 {
            assert!(DataFormat::try_from(code).is_ok(), "code {code} should map");
        }
        assert_eq!(DataFormat::try_from(0), Err(0));
        assert_eq!(DataFormat::try_from(13), Err(13));
        assert_eq!(DataFormat::try_from(0xFFFF), Err(0xFFFF));
    }

    #[test]
    fn entry_parse_single_component_success_paths() {
        // Cover the single-component success arms of parse() for the
        // numeric formats (lines 178, 186, 203, 227, 235, 242, 257, 264)
        // and Undefined (line 233).
        let cases: &[(DataFormat, &[u8], fn(&EntryValue) -> bool)] = &[
            (DataFormat::U8, &[42], |v| matches!(v, EntryValue::U8(42))),
            (DataFormat::U16, &[1, 0], |v| {
                matches!(v, EntryValue::U16(1))
            }),
            (DataFormat::U32, &[1, 0, 0, 0], |v| {
                matches!(v, EntryValue::U32(1))
            }),
            (DataFormat::I8, &[0xFF], |v| matches!(v, EntryValue::I8(-1))),
            (DataFormat::I16, &[0xFF, 0xFF], |v| {
                matches!(v, EntryValue::I16(-1))
            }),
            (DataFormat::I32, &[0xFF, 0xFF, 0xFF, 0xFF], |v| {
                matches!(v, EntryValue::I32(-1))
            }),
            (
                DataFormat::F32,
                &[0, 0, 0x80, 0x3F],
                |v| matches!(v, EntryValue::F32(x) if (*x - 1.0).abs() < 1e-6),
            ),
            (
                DataFormat::F64,
                &[0, 0, 0, 0, 0, 0, 0xF0, 0x3F],
                |v| matches!(v, EntryValue::F64(x) if (*x - 1.0).abs() < 1e-9),
            ),
            (
                DataFormat::Undefined,
                &[0xAA, 0xBB],
                |v| matches!(v, EntryValue::Undefined(d) if d == &[0xAA, 0xBB]),
            ),
        ];
        for (fmt, data, check) in cases {
            let entry = EntryData {
                tag: 0,
                endian: Endianness::Little,
                data,
                data_format: *fmt,
                components_num: 1,
            };
            let v = EntryValue::parse(&entry, &None).unwrap();
            assert!(check(&v), "{fmt:?} single-component returned {v:?}");
        }

        // Multi-component success paths for U16/U32/U8 arrays + rationals.
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[1, 0, 2, 0],
            data_format: DataFormat::U16,
            components_num: 2,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(matches!(v, EntryValue::U16Array(ref a) if a == &[1u16, 2]));

        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[1, 0, 0, 0, 2, 0, 0, 0],
            data_format: DataFormat::U32,
            components_num: 2,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(matches!(v, EntryValue::U32Array(ref a) if a == &[1u32, 2]));

        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[1, 2, 3],
            data_format: DataFormat::U8,
            components_num: 3,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(matches!(v, EntryValue::U8Array(ref a) if a == &[1u8, 2, 3]));

        // URational single + array.
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[1, 0, 0, 0, 2, 0, 0, 0],
            data_format: DataFormat::URational,
            components_num: 1,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(
            matches!(v, EntryValue::URational(r) if r.numerator() == 1 && r.denominator() == 2)
        );

        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0],
            data_format: DataFormat::URational,
            components_num: 2,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(matches!(v, EntryValue::URationalArray(ref a) if a.len() == 2));

        // IRational single + array.
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[0xFF, 0xFF, 0xFF, 0xFF, 2, 0, 0, 0],
            data_format: DataFormat::IRational,
            components_num: 1,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(
            matches!(v, EntryValue::IRational(r) if r.numerator() == -1 && r.denominator() == 2)
        );

        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[0xFF, 0xFF, 0xFF, 0xFF, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0],
            data_format: DataFormat::IRational,
            components_num: 2,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(matches!(v, EntryValue::IRationalArray(ref a) if a.len() == 2));

        // Text path.
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: b"hello\0",
            data_format: DataFormat::Text,
            components_num: 6,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(matches!(v, EntryValue::Text(ref s) if s == "hello"));
    }

    #[test]
    fn entry_parse_empty_data_errors() {
        // Cover lines 136-141: data.is_empty() returns InvalidShape.
        use crate::error::EntryError;
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            data: &[],
            data_format: DataFormat::U16,
            components_num: 1,
        };
        let err = EntryValue::parse(&entry, &None).unwrap_err();
        assert!(matches!(err, EntryError::InvalidShape { .. }));
    }

    #[test]
    fn get_cstr_non_utf8_falls_back() {
        // Hitting the fall-back branch in get_cstr (lines 858-861) by
        // routing invalid UTF-8 through the Text variant of parse().
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Little,
            // 0xFF is not valid UTF-8 alone.
            data: &[0xFFu8, b'a', 0],
            data_format: DataFormat::Text,
            components_num: 3,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(matches!(v, EntryValue::Text(_)));
    }

    #[test]
    fn try_from_bytes_big_endian_and_truncated() {
        // Cover the big-endian arm (lines 891-898) and the Truncated error
        // (lines 883-887, 893) of TryFromBytes.
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Big,
            data: &[0, 1],
            data_format: DataFormat::U16,
            components_num: 1,
        };
        let v = EntryValue::parse(&entry, &None).unwrap();
        assert!(matches!(v, EntryValue::U16(1)));

        // Truncated.
        let entry = EntryData {
            tag: 0,
            endian: Endianness::Big,
            data: &[0],
            data_format: DataFormat::U16,
            components_num: 1,
        };
        let err = EntryValue::parse(&entry, &None).unwrap_err();
        assert!(matches!(err, crate::error::EntryError::Truncated { .. }));
    }
}

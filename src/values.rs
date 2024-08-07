use std::fmt::Display;

use chrono::{DateTime, FixedOffset};

#[cfg(feature = "json_dump")]
use serde::{Deserialize, Serialize, Serializer};

/// Represent a parsed entry value.
#[derive(Debug, Clone, PartialEq)]
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
                write!(f, "{}/{} ({:.04})", v.0, v.1, v.0 as f64 / v.1 as f64)
            }
            EntryValue::IRational(v) => {
                write!(f, "{}/{} ({:.04})", v.0, v.1, v.0 as f64 / v.1 as f64)
            }
            EntryValue::U32(v) => v.fmt(f),
            EntryValue::U16(v) => v.fmt(f),
            EntryValue::U64(v) => v.fmt(f),
            EntryValue::I16(v) => v.fmt(f),
            EntryValue::I32(v) => v.fmt(f),
            EntryValue::I64(v) => v.fmt(f),
            EntryValue::F32(v) => v.fmt(f),
            EntryValue::F64(v) => v.fmt(f),
            EntryValue::U8(v) => v.fmt(f),
            EntryValue::I8(v) => v.fmt(f),
            EntryValue::Time(v) => v.to_rfc3339().fmt(f),
        }
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
        Self::IRational(IRational(value.0, value.1))
    }
}

#[cfg_attr(feature = "json_dump", derive(Serialize, Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct URational(pub u32, pub u32);

impl URational {
    pub fn as_float(&self) -> f64 {
        self.0 as f64 / self.1 as f64
    }

    #[deprecated(since = "1.2.3", note = "please use `as_float` instead")]
    #[allow(clippy::wrong_self_convention)]
    pub fn to_float(&self) -> f64 {
        self.as_float()
    }
}

impl From<(u32, u32)> for URational {
    fn from(value: (u32, u32)) -> Self {
        Self(value.0, value.1)
    }
}

impl From<URational> for (u32, u32) {
    fn from(val: URational) -> Self {
        (val.0, val.1)
    }
}

#[cfg_attr(feature = "json_dump", derive(Serialize, Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct IRational(pub i32, pub i32);

impl From<(i32, i32)> for IRational {
    fn from(value: (i32, i32)) -> Self {
        Self(value.0, value.1)
    }
}

impl From<IRational> for (i32, i32) {
    fn from(val: IRational) -> Self {
        (val.0, val.1)
    }
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

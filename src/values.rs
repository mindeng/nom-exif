use std::fmt::Display;

#[cfg(feature = "serialize")]
use serde::{Deserialize, Serialize};

/// Represent a parsed entry value.
#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum EntryValue {
    Text(String),
    URational(URational),
    IRational(IRational),

    U16(u16),
    U32(u32),
    U64(u64),

    I16(i16),
    I32(i32),
    I64(i64),

    F64(f64),
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
            EntryValue::F64(v) => v.fmt(f),
        }
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
        Self::IRational(IRational(value.0, value.1))
    }
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

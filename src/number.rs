use bytes::{BufMut as _, BytesMut};
use nom::number::Endianness;

pub(crate) struct Number<T: Sized>(pub T, pub Endianness);

impl From<Number<u16>> for [u8; 2] {
    fn from(value: Number<u16>) -> Self {
        match value.1 {
            Endianness::Big => value.0.to_be_bytes(),
            Endianness::Little => value.0.to_le_bytes(),
            Endianness::Native => value.0.to_ne_bytes(),
        }
    }
}

impl From<Number<i16>> for [u8; 2] {
    fn from(value: Number<i16>) -> Self {
        match value.1 {
            Endianness::Big => value.0.to_be_bytes(),
            Endianness::Little => value.0.to_le_bytes(),
            Endianness::Native => value.0.to_ne_bytes(),
        }
    }
}

impl From<Number<u32>> for [u8; 4] {
    fn from(value: Number<u32>) -> Self {
        match value.1 {
            Endianness::Big => value.0.to_be_bytes(),
            Endianness::Little => value.0.to_le_bytes(),
            Endianness::Native => value.0.to_ne_bytes(),
        }
    }
}

impl From<Number<i32>> for [u8; 4] {
    fn from(value: Number<i32>) -> Self {
        match value.1 {
            Endianness::Big => value.0.to_be_bytes(),
            Endianness::Little => value.0.to_le_bytes(),
            Endianness::Native => value.0.to_ne_bytes(),
        }
    }
}

impl From<Number<u64>> for [u8; 8] {
    fn from(value: Number<u64>) -> Self {
        match value.1 {
            Endianness::Big => value.0.to_be_bytes(),
            Endianness::Little => value.0.to_le_bytes(),
            Endianness::Native => value.0.to_ne_bytes(),
        }
    }
}

impl From<Number<i64>> for [u8; 8] {
    fn from(value: Number<i64>) -> Self {
        match value.1 {
            Endianness::Big => value.0.to_be_bytes(),
            Endianness::Little => value.0.to_le_bytes(),
            Endianness::Native => value.0.to_ne_bytes(),
        }
    }
}

impl From<Number<f32>> for [u8; 4] {
    fn from(value: Number<f32>) -> Self {
        match value.1 {
            Endianness::Big => value.0.to_be_bytes(),
            Endianness::Little => value.0.to_le_bytes(),
            Endianness::Native => value.0.to_ne_bytes(),
        }
    }
}

impl From<Number<f64>> for [u8; 8] {
    fn from(value: Number<f64>) -> Self {
        match value.1 {
            Endianness::Big => value.0.to_be_bytes(),
            Endianness::Little => value.0.to_le_bytes(),
            Endianness::Native => value.0.to_ne_bytes(),
        }
    }
}

pub(crate) fn put_u16(buf: &mut BytesMut, n: u16, endian: Endianness) {
    match endian {
        Endianness::Big => buf.put_u16(n),
        Endianness::Little => buf.put_u16_le(n),
        Endianness::Native => unreachable!(),
    }
}

pub(crate) fn put_i16(buf: &mut BytesMut, n: i16, endian: Endianness) {
    match endian {
        Endianness::Big => buf.put_i16(n),
        Endianness::Little => buf.put_i16_le(n),
        Endianness::Native => unreachable!(),
    }
}

pub(crate) fn put_u32(buf: &mut BytesMut, n: u32, endian: Endianness) {
    match endian {
        Endianness::Big => buf.put_u32(n),
        Endianness::Little => buf.put_u32_le(n),
        Endianness::Native => unreachable!(),
    }
}

pub(crate) fn put_i32(buf: &mut BytesMut, n: i32, endian: Endianness) {
    match endian {
        Endianness::Big => buf.put_i32(n),
        Endianness::Little => buf.put_i32_le(n),
        Endianness::Native => unreachable!(),
    }
}

pub(crate) fn put_u64(buf: &mut BytesMut, n: u64, endian: Endianness) {
    match endian {
        Endianness::Big => buf.put_u64(n),
        Endianness::Little => buf.put_u64_le(n),
        Endianness::Native => unreachable!(),
    }
}

pub(crate) fn put_i64(buf: &mut BytesMut, n: i64, endian: Endianness) {
    match endian {
        Endianness::Big => buf.put_i64(n),
        Endianness::Little => buf.put_i64_le(n),
        Endianness::Native => unreachable!(),
    }
}

pub(crate) fn put_f32(buf: &mut BytesMut, n: f32, endian: Endianness) {
    match endian {
        Endianness::Big => buf.put_f32(n),
        Endianness::Little => buf.put_f32_le(n),
        Endianness::Native => unreachable!(),
    }
}

pub(crate) fn put_f64(buf: &mut BytesMut, n: f64, endian: Endianness) {
    match endian {
        Endianness::Big => buf.put_f64(n),
        Endianness::Little => buf.put_f64_le(n),
        Endianness::Native => unreachable!(),
    }
}

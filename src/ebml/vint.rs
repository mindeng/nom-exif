use std::io::Cursor;

use bytes::Buf;
use thiserror::Error;

#[derive(Debug)]
pub(crate) struct VInt;

#[derive(Debug, Error)]
pub(crate) enum ParseVIntFailed {
    #[error("invalid VInt: {0}")]
    InvalidVInt(&'static str),

    #[error("need more bytes: {0}")]
    Need(usize),
}

impl VInt {
    pub fn as_u64_with_marker(data: &mut Cursor<&[u8]>) -> Result<u64, ParseVIntFailed> {
        let (remain, v) = Self::parse_unsigned(&data.get_ref()[data.position() as usize..], true)?;
        data.set_position(data.position() + (data.remaining() - remain.len()) as u64);
        Ok(v)
    }

    pub fn as_usize(data: &mut Cursor<&[u8]>) -> Result<usize, ParseVIntFailed> {
        let (remain, v) = Self::parse_unsigned(&data.get_ref()[data.position() as usize..], false)
            .map(|(d, v)| (d, v as usize))?;
        data.set_position(data.position() + (data.remaining() - remain.len()) as u64);
        Ok(v)
    }

    pub(crate) fn parse_unsigned(
        data: &[u8],
        reserve_marker: bool,
    ) -> Result<(&[u8], u64), ParseVIntFailed> {
        if data.is_empty() {
            return Err(ParseVIntFailed::Need(1));
        }

        let n = data[0].leading_zeros() as usize + 1;
        if n > data.len() {
            return Err(ParseVIntFailed::Need(n - data.len()));
        }
        if n > 8 {
            return Err(ParseVIntFailed::InvalidVInt("size > 8 is not supported"));
        }
        // println!("n: {n}");

        let mut octets = [0u8; 8];
        let start = 8 - n;
        octets[start..].copy_from_slice(&data[..n]);

        // remove the marker
        if !reserve_marker {
            if n == 8 {
                octets[0] = 0;
            } else {
                // println!("first byte: {:08b}", data[0]);
                let first = data[0] & (0xFF >> n);
                // println!("first byte: {:08b}", first);
                octets[start] = first;
            }
        }

        let v = u64::from_be_bytes(octets);

        Ok((&data[n..], v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case(&[0b1000_0010], Some((&[], 2)))]
    #[test_case(&[0b0100_0000, 0b0000_0010], Some((&[], 2)))]
    #[test_case(&[0b0010_0000, 0b0000_0000, 0b0000_0010], Some((&[], 2)))]
    #[test_case(&[0b0001_0000, 0b0000_0000, 0b0000_0000, 0b0000_0010], Some((&[], 2)))]
    #[test_case(&[0b0001_0000, 0b0000_0000, 0b1000_0000, 0b0000_0000, 0xFF], Some((&[0xFF], 0x8000)))]
    #[test_case(&[0b0000_0001, 0b1000_0000, 0b1000_0000, 0b0000_0001], None)]
    #[test_case(&[0b0000_0010, 0b1000_1000, 0b1000_1000, 0b0000_0000, 0, 0, 0x80, 0x08], Some((&[0x08], 0x0000_8888_0000_0080)))]
    #[test_case(&[0b0000_0001, 0b1000_1000, 0b1000_1000, 0b0000_0000, 0, 0, 0x80, 0x08], Some((&[], 0x0088_8800_0000_8008)))]
    #[test_case(&[0b0000_0001, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff], Some((&[], 0x00ff_ffff_ffff_ffff)))]
    fn vint_parse_u(data: &[u8], expect: Option<(&[u8], u64)>) {
        let actual = VInt::parse_unsigned(data, false);
        if let Some(expect) = expect {
            assert_eq!(actual.unwrap(), expect);
        } else {
            actual.unwrap_err();
        }
    }
}

use std::ops::Range;

use nom::{bytes::streaming, IResult};

use crate::bbox::BoxHeader;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdatBox<'a> {
    header: BoxHeader,
    data: &'a [u8],
}

#[allow(unused)]
impl<'a> IdatBox<'a> {
    pub fn parse(input: &'a [u8]) -> IResult<&'a [u8], IdatBox> {
        let (remain, header) = BoxHeader::parse(input)?;
        let (remain, data) = streaming::take(header.box_size - header.header_size as u64)(remain)?;

        Ok((remain, IdatBox { header, data }))
    }

    pub fn get_data(&self, range: Range<usize>) -> crate::Result<&[u8]> {
        if range.len() > self.data.len() {
            Err("idat data is too small".into())
        } else {
            Ok(&self.data[range])
        }
    }
}

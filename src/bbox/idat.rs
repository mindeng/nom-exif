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
    #[tracing::instrument(skip(input))]
    pub fn parse(input: &'a [u8]) -> IResult<&'a [u8], IdatBox> {
        let (remain, header) = BoxHeader::parse(input)?;

        let box_size = usize::try_from(header.box_size).expect("box size must fit into a `usize`.");

        // let header_size =
        //     u64::try_from(header.header_size).expect("header size should always fit into a `u64`.");

        let (remain, data) = streaming::take(box_size - header.header_size)(remain)?;

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

use core::ops::Range;

use nom::{bytes::streaming, IResult};

use crate::bbox::BoxHeader;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(unused)]
struct IdatBox<'a> {
    header: BoxHeader,
    data: &'a [u8],
}

impl<'a> IdatBox<'a> {
    #[allow(unused)]
    pub(crate) fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (remain, header) = BoxHeader::parse(input)?;
        let ct = TryInto::<usize>::try_into(header.box_size - header.header_size as u64);

        let Ok(ct) = ct else {
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )));
        };

        let (remain, data) = streaming::take(ct)(remain)?;

        Ok((remain, IdatBox { header, data }))
    }

    #[allow(unused)]
    pub(crate) fn get_data(&self, range: Range<usize>) -> crate::Result<&[u8]> {
        if range.len() > self.data.len() {
            Err("idat data is too small".into())
        } else {
            Ok(&self.data[range])
        }
    }
}

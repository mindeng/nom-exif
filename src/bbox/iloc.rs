use std::collections::HashMap;

use nom::{
    combinator::{cond, fail, map_res},
    error::context,
    multi::many_m_n,
    number::streaming::{be_u16, be_u32, be_u64, be_u8},
    IResult,
};

use crate::bbox::FullBoxHeader;

use super::{Error, ParseBody};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IlocBox {
    header: FullBoxHeader,
    offset_size: u8,      // 4 bits
    length_size: u8,      // 4 bits
    base_offset_size: u8, // 4 bits
    index_size: u8,       // 4 bits, version 1/2, reserved in version 0
    items: HashMap<u32, ItemLocation>,
}

const MAX_ILOC_EXTENTS_PER_ITEM: u16 = 32;

impl ParseBody<IlocBox> for IlocBox {
    fn parse_body<'a>(remain: &'a [u8], header: FullBoxHeader) -> IResult<&'a [u8], IlocBox> {
        let version = header.version;

        let (remain, (offset_size, length_size)) =
            map_res(be_u8, |res| Ok::<(u8, u8), ()>((res >> 4, res & 0xF)))(remain)?;

        let (remain, (base_offset_size, index_size)) =
            map_res(be_u8, |res| Ok::<(u8, u8), ()>((res >> 4, res & 0xF)))(remain)?;

        let (remain, item_count) = if version < 2 {
            map_res(be_u16, |x| Ok::<u32, ()>(x as u32))(remain)?
        } else {
            be_u32(remain)?
        };

        let (remain, items) = many_m_n(item_count as usize, item_count as usize, |remain| {
            let (remain, item_id) = if version < 2 {
                map_res(be_u16, |x| Ok::<u32, ()>(x as u32))(remain)?
            } else {
                be_u32(remain)?
            };

            let (remain, construction_method) = cond(
                version >= 1,
                map_res(be_u16, |res| Ok::<u8, ()>((res & 0xF) as u8)),
            )(remain)?;

            let (remain, data_ref_index) = be_u16(remain)?;

            let (remain, base_offset) =
                parse_base_offset(base_offset_size, remain, "base_offset_size is not 4 or 8")?;

            let (remain, extent_count) = be_u16(remain)?;
            if extent_count > MAX_ILOC_EXTENTS_PER_ITEM {
                // eprintln!("extent_count: {extent_count}");
                context("extent_count > 32", fail::<_, (), _>)(remain)?;
            }

            let (remain, extents) =
                many_m_n(extent_count as usize, extent_count as usize, |remain| {
                    let (remain, index) =
                        parse_base_offset(index_size, remain, "index_size is not 4 or 8")?;
                    let (remain, offset) =
                        parse_base_offset(offset_size, remain, "offset_size is not 4 or 8")?;
                    let (remain, length) =
                        parse_base_offset(length_size, remain, "length_size is not 4 or 8")?;

                    Ok((
                        remain,
                        ItemLocationExtent {
                            index,
                            offset,
                            length,
                        },
                    ))
                })(remain)?;

            Ok((
                remain,
                ItemLocation {
                    extents,
                    id: item_id,
                    construction_method,
                    base_offset,
                    data_ref_index,
                },
            ))
        })(remain)?;

        Ok((
            remain,
            IlocBox {
                header,
                offset_size,
                length_size,
                base_offset_size,
                index_size,
                items: items.into_iter().map(|x| (x.id, x)).collect(),
            },
        ))
    }
}

impl IlocBox {
    pub fn item_offset_len(&self, id: u32) -> Option<(u8, u64, u64)> {
        self.items
            .get(&id)
            .map(|item| (item, item.extents.first()))
            .and_then(|(item, extent)| {
                extent.map(|extent| {
                    (
                        item.construction_method.unwrap_or(0),
                        item.base_offset + extent.offset,
                        extent.length,
                    )
                })
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ItemLocationExtent {
    index: u64,
    offset: u64,
    length: u64,
}

fn parse_base_offset<'a>(size: u8, remain: &'a [u8], msg: &'static str) -> IResult<&'a [u8], u64> {
    Ok(if size == 4 {
        map_res(be_u32, |x| Ok::<u64, ()>(x as u64))(remain)?
    } else if size == 8 {
        be_u64(remain)?
    } else if size == 0 {
        (remain, 0)
    } else {
        context(msg, fail)(remain)?
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLocation {
    id: u32,
    /// 0: file offset, 1: idat offset, 2: item offset (currently not supported)
    construction_method: Option<u8>,
    data_ref_index: u16,
    base_offset: u64,
    extents: Vec<ItemLocationExtent>,
}

enum ConstructionMethod {
    FileOffset = 0,
    IdatOffset = 1,
    ItemOffset = 2,
}

impl TryFrom<u8> for ConstructionMethod {
    type Error = Error;
    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::FileOffset),
            1 => Ok(Self::IdatOffset),
            2 => Ok(Self::ItemOffset),
            other => Err(Error::UnsupportedConstructionMethod(other)),
        }
    }
}

use std::collections::HashMap;

use nom::{
    bytes::streaming,
    combinator::{cond, fail, map_res},
    error::context,
    multi::many_m_n,
    number::streaming::{be_u16, be_u32},
    IResult,
};

use crate::bbox::FullBoxHeader;

use super::{parse_cstr, ParseBody, ParseBox};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IinfBox {
    header: FullBoxHeader,
    entries: HashMap<String, InfeBox>,
}

impl ParseBody<Self> for IinfBox {
    fn parse_body(remain: &[u8], header: FullBoxHeader) -> IResult<&[u8], Self> {
        let version = header.version;

        let (remain, item_count) = if version > 0 {
            be_u32(remain)?
        } else {
            map_res(be_u16, |x| Ok::<u32, ()>(x as u32))(remain)?
        };

        let (remain, entries) =
            many_m_n(item_count as usize, item_count as usize, InfeBox::parse_box)(remain)?;

        let entries = entries
            .into_iter()
            .map(|e| (e.key().to_owned(), e))
            .collect::<HashMap<_, _>>();

        Ok((remain, Self { header, entries }))
    }
}

impl IinfBox {
    pub fn get_infe(&self, item_type: &'static str) -> Option<&InfeBox> {
        self.entries.get(item_type)
    }
}

/// Info entry box
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InfeBox {
    pub header: FullBoxHeader,
    pub id: u32,
    pub protection_index: u16,
    pub item_type: Option<String>, // version >= 2
    pub item_name: String,
    content_type: Option<String>,
    content_encoding: Option<String>,
    uri_type: Option<String>,
}

impl ParseBody<Self> for InfeBox {
    #[tracing::instrument(skip_all)]
    fn parse_body<'a>(remain: &'a [u8], header: FullBoxHeader) -> IResult<&'a [u8], Self> {
        let version = header.version;

        let (remain, id) = if version > 2 {
            be_u32(remain)?
        } else {
            map_res(be_u16, |x| Ok::<u32, ()>(x as u32))(remain)?
        };

        let (remain, protection_index) = be_u16(remain)?;

        let (remain, item_type) = cond(
            version >= 2,
            map_res(streaming::take(4_usize), |res: &'a [u8]| {
                String::from_utf8(res.to_vec())
            }),
        )(remain)?;

        tracing::debug!(?header.box_type, ?item_type, ?version, "Got");

        let (remain, item_name) = parse_cstr(remain).map_err(|e| {
            if e.is_incomplete() {
                context("no enough bytes for infe item name", fail::<_, (), _>)(remain).unwrap_err()
            } else {
                e
            }
        })?;

        let (remain, content_type, content_encoding) =
            if version <= 1 || (version >= 2 && item_type.as_ref().unwrap() == "mime") {
                let (remain, content_type) = parse_cstr(remain)?;
                let (remain, content_encoding) = cond(!remain.is_empty(), parse_cstr)(remain)?;
                (remain, Some(content_type), content_encoding)
            } else {
                (remain, None, None)
            };

        let (remain, uri_type) = if version >= 2 && item_type.as_ref().unwrap() == "uri" {
            let (remain, uri_type) = parse_cstr(remain)?;
            (remain, Some(uri_type))
        } else {
            (remain, None)
        };

        Ok((
            remain,
            Self {
                header,
                id,
                protection_index,
                item_type,
                item_name,
                content_type,
                content_encoding,
                uri_type,
            },
        ))
    }
}

impl InfeBox {
    fn key(&self) -> &String {
        self.item_type.as_ref().unwrap_or(&self.item_name)
    }
}

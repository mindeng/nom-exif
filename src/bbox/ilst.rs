use nom::bytes::complete::{tag, take};
use nom::combinator::{fail, map_res};
use nom::error::context;
use nom::multi::many0;
use nom::number::complete::{
    be_f32, be_f64, be_i16, be_i24, be_i32, be_i64, be_u16, be_u24, be_u32, be_u64, u8,
};
use nom::sequence::tuple;

use crate::EntryValue;

use super::BoxHeader;

/// Represents an [item list atom][1].
///
/// ilst is not a fullbox, it doesn't have version & flags.
///
/// atom-path: moov/meta/ilst
///
/// [1]: https://developer.apple.com/documentation/quicktime-file-format/metadata_item_list_atom
#[derive(Debug, Clone, PartialEq)]
pub struct IlstBox {
    header: BoxHeader,
    pub items: Vec<IlstItem>,
}

impl IlstBox {
    pub fn parse_box(input: &[u8]) -> nom::IResult<&[u8], IlstBox> {
        let (remain, header) = BoxHeader::parse(input)?;
        let (remain, items) = many0(IlstItem::parse)(remain)?;

        Ok((remain, IlstBox { header, items }))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IlstItem {
    size: u32,
    index: u32,    // 1-based index (start from 1)
    data_len: u32, // including self size

    /// Type indicator, see [type
    /// indicator](https://developer.apple.com/documentation/quicktime-file-format/type_indicator)
    type_set: u8,
    type_code: u32, // 24-bits

    local: u32,
    pub value: EntryValue, // len: data_len - 16
}

impl IlstItem {
    fn parse<'a>(input: &'a [u8]) -> nom::IResult<&'a [u8], IlstItem> {
        let (remain, (size, index, data_len, _, type_set, type_code, local)) =
            tuple((be_u32, be_u32, be_u32, tag("data"), u8, be_u24, be_u32))(input)?;

        if size < 24 || data_len < 16 {
            context("invalid ilst item", fail::<_, (), _>)(remain)?;
        }

        // assert_eq!(size - 24, data_len - 16);
        if size - 24 != data_len - 16 {
            context("invalid ilst item", fail::<_, (), _>)(remain)?;
        }

        let (remain, value) = map_res(take(data_len - 16), |bs: &'a [u8]| {
            parse_value(type_code, bs)
        })(remain)?;

        Ok((
            remain,
            IlstItem {
                size,
                index,
                data_len,
                type_set,
                type_code,
                local,
                value,
            },
        ))
    }
}

/// Parse ilst item data to value, see [Well-known
/// types](https://developer.apple.com/documentation/quicktime-file-format/well-known_types)
fn parse_value(type_code: u32, data: &[u8]) -> crate::Result<EntryValue> {
    use EntryValue::*;
    let v = match type_code {
        1 => {
            let s = String::from_utf8(data.to_vec())?;
            Text(s)
        }
        21 => match data.len() {
            1 => data[0].into(),
            2 => be_i16(data)?.1.into(),
            3 => be_i24(data)?.1.into(),
            4 => be_i32(data)?.1.into(),
            8 => be_i64(data)?.1.into(),
            x => {
                let msg = format!("Invalid ilst item data; data type is BE Signed Integer while data len is : {x}");
                // eprintln!("{msg}");
                return Err(msg.into());
            }
        },
        22 => match data.len() {
            1 => data[0].into(),
            2 => be_u16(data)?.1.into(),
            3 => be_u24(data)?.1.into(),
            4 => be_u32(data)?.1.into(),
            8 => be_u64(data)?.1.into(),
            x => {
                let msg = format!("Invalid ilst item data; data type is BE Unsigned Integer while data len is : {x}");
                // eprintln!("{msg}");
                return Err(msg.into());
            }
        },
        23 => be_f32(data)?.1.into(),
        24 => be_f64(data)?.1.into(),
        o => {
            let msg = format!("Unsupported ilst item data type: {o}");
            // eprintln!("{msg}");
            return Err(msg.into());
        }
    };
    Ok(v)
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read, path::Path};

    use crate::bbox::travel_while;

    use super::*;
    use test_case::test_case;

    #[test_case("meta.mov")]
    fn ilst_box(path: &str) {
        let mut f = open_sample(path);
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();

        let (_, bbox) = travel_while(&buf, |b| b.box_type() != "moov").unwrap();
        let (_, bbox) = travel_while(bbox.body_data(), |b| b.box_type() != "meta").unwrap();
        let (_, bbox) = travel_while(bbox.body_data(), |b| b.box_type() != "ilst").unwrap();

        let (rem, ilst) = IlstBox::parse_box(bbox.data).unwrap();
        println!("ilst: {ilst:?}");
        assert_eq!(rem, b"");

        assert_eq!(
                    ilst.items
                        .iter()
                        .map(|x| format!("{x:?}"))
                        .collect::<Vec<_>>(),
[
"IlstItem { size: 29, index: 1, data_len: 21, type_set: 0, type_code: 1, local: 0, value: Text(\"Apple\") }",
"IlstItem { size: 32, index: 2, data_len: 24, type_set: 0, type_code: 1, local: 0, value: Text(\"iPhone X\") }",
"IlstItem { size: 30, index: 3, data_len: 22, type_set: 0, type_code: 1, local: 0, value: Text(\"12.1.2\") }",
"IlstItem { size: 50, index: 4, data_len: 42, type_set: 0, type_code: 1, local: 0, value: Text(\"+27.1281+100.2508+000.000/\") }",
"IlstItem { size: 49, index: 5, data_len: 41, type_set: 0, type_code: 1, local: 0, value: Text(\"2019-02-12T15:27:12+08:00\") }"
],
                );
    }

    #[test_case("embedded-in-heic.mov")]
    fn heic_mov_ilst(path: &str) {
        let mut f = open_sample(path);
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();

        let (_, moov) = travel_while(&buf, |b| b.box_type() != "moov").unwrap();
        let (_, meta) = travel_while(moov.body_data(), |b| b.box_type() != "meta").unwrap();
        let (_, ilst) = travel_while(meta.body_data(), |b| b.box_type() != "ilst").unwrap();

        let (rem, ilst) = IlstBox::parse_box(ilst.data).unwrap();
        assert_eq!(rem.len(), 0);

        let mut s = ilst
            .items
            .iter()
            .map(|x| format!("{x:?}"))
            .collect::<Vec<_>>()
            .join("\n");
        s.insert(0, '\n');

        assert_eq!(
            s,
"
IlstItem { size: 33, index: 1, data_len: 25, type_set: 0, type_code: 1, local: 0, value: Text(\"14.235563\") }
IlstItem { size: 25, index: 2, data_len: 17, type_set: 0, type_code: 22, local: 0, value: U8(1) }
IlstItem { size: 60, index: 3, data_len: 52, type_set: 0, type_code: 1, local: 0, value: Text(\"DA1A7EE8-0925-4C9F-9266-DDA3F0BB80F0\") }
IlstItem { size: 28, index: 4, data_len: 20, type_set: 0, type_code: 23, local: 0, value: F32(0.93884003) }
IlstItem { size: 32, index: 5, data_len: 24, type_set: 0, type_code: 21, local: 0, value: I64(4) }
IlstItem { size: 50, index: 6, data_len: 42, type_set: 0, type_code: 1, local: 0, value: Text(\"+22.5797+113.9380+028.396/\") }
IlstItem { size: 29, index: 7, data_len: 21, type_set: 0, type_code: 1, local: 0, value: Text(\"Apple\") }
IlstItem { size: 37, index: 8, data_len: 29, type_set: 0, type_code: 1, local: 0, value: Text(\"iPhone 15 Pro\") }
IlstItem { size: 28, index: 9, data_len: 20, type_set: 0, type_code: 1, local: 0, value: Text(\"17.1\") }
IlstItem { size: 48, index: 10, data_len: 40, type_set: 0, type_code: 1, local: 0, value: Text(\"2023-11-02T19:58:34+0800\") }"
            );
    }

    fn open_sample(path: &str) -> File {
        File::open(Path::new("./testdata").join(path)).unwrap()
    }
}

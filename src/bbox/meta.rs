use std::collections::HashMap;

use nom::{combinator::fail, multi::many0, IResult, Needed};

use crate::bbox::FullBoxHeader;

use super::{iinf::IinfBox, iloc::IlocBox, BoxHolder, ParseBody, ParseBox};

/// Representing the `meta` box in a HEIF/HEIC file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaBox {
    header: FullBoxHeader,
    iinf: Option<IinfBox>,
    iloc: Option<IlocBox>,
    // idat: Option<IdatBox<'a>>,
}

impl ParseBody<MetaBox> for MetaBox {
    fn parse_body<'a>(remain: &'a [u8], header: FullBoxHeader) -> IResult<&'a [u8], MetaBox> {
        let (remain, boxes) = many0(|remain: &'a [u8]| {
            if remain.is_empty() {
                // stop many0 parsing to prevent Incomplete error
                fail::<_, (), _>(remain)?;
            }
            let (remain, bbox) = BoxHolder::parse(remain)?;
            Ok((remain, bbox))
        })(remain)?;

        if !remain.is_empty() {
            // body is invalid
            return fail(remain);
        }

        let boxes = boxes
            .into_iter()
            .map(|b| (b.header.box_type.to_owned(), b))
            .collect::<HashMap<_, _>>();

        // parse iinf box
        let iinf = boxes
            .get("iinf")
            .and_then(|iinf| Some(IinfBox::parse_box(iinf.data)))
            .transpose()?
            .map(|x| x.1);

        // parse iloc box
        let iloc = boxes
            .get("iloc")
            .and_then(|iloc| Some(IlocBox::parse_box(iloc.data)))
            .transpose()?
            .map(|x| x.1);

        // parse idat box
        // let idat = boxes
        //     .get("idat")
        //     .and_then(|idat| Some(IdatBox::parse(idat.data)))
        //     .transpose()?
        //     .map(|x| x.1);

        Ok((
            remain,
            MetaBox {
                header,
                iinf,
                iloc,
                // idat,
            },
        ))
    }
}

impl MetaBox {
    pub fn exif_data<'a>(&self, input: &'a [u8]) -> IResult<&'a [u8], Option<&'a [u8]>> {
        self.iinf
            .as_ref()
            .and_then(|iinf| iinf.get_infe("Exif"))
            .and_then(|exif_infe| {
                self.iloc
                    .as_ref()
                    .and_then(|iloc| iloc.item_offset_len(exif_infe.id))
            })
            .and_then(|(construction_method, offset, length)| {
                let start = offset as usize;
                let end = (offset + length) as usize;
                if construction_method == 0 {
                    // file offset
                    if end > input.len() {
                        Some(Err(nom::Err::Incomplete(Needed::new(end - input.len()))))
                    } else {
                        Some(Ok((&input[end..], Some(&input[start..end])))) // Safe-slice
                    }
                } else if construction_method == 1 {
                    // idat offset
                    eprintln!("idat offset construction method is not supported yet");
                    Some(fail(input))
                } else {
                    eprintln!("item offset construction method is not supported yet");
                    Some(fail(input))
                }
            })
            .unwrap_or(Ok((input, None)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ItemLocationExtent {
    index: u64,
    offset: u64,
    length: u64,
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

#[cfg(test)]
mod tests {
    use std::io::Read;

    use crate::{bbox::travel_while, testkit::open_sample};

    use super::*;
    use test_case::test_case;

    #[test_case("exif.heic", 2618)]
    fn meta(path: &str, meta_size: usize) {
        let mut reader = open_sample(path).unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(buf.as_mut()).unwrap();
        assert_eq!(buf.len() as u64, reader.metadata().unwrap().len());

        let (_, bbox) = travel_while(&buf, |bbox| {
            // println!("got {}", bbox.header.box_type);
            bbox.box_type() != "meta"
        })
        .unwrap();

        assert_eq!(bbox.data.len() as u64, bbox.box_size());
        let (remain, meta) = MetaBox::parse_box(bbox.data).unwrap();
        assert_eq!(remain, b"");
        assert_eq!(meta.header.box_type, "meta");
        assert_eq!(meta.exif_data(&buf).unwrap().1.unwrap().len(), meta_size);
    }
}

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

impl ParseBody<Self> for MetaBox {
    fn parse_body<'a>(remain: &'a [u8], header: FullBoxHeader) -> IResult<&'a [u8], Self> {
        let (remain, boxes) = many0(|remain: &'a [u8]| {
            if remain.is_empty() {
                // stop many0 parsing to prevent Incomplete error
                fail::<_, (), _>(remain)?;
            }
            let (remain, bbox) = BoxHolder::parse(remain)?;
            Ok((remain, bbox))
        })(remain)?;

        let boxes = boxes
            .into_iter()
            .map(|b| (b.header.box_type.clone(), b))
            .collect::<HashMap<_, _>>();

        // parse iinf box
        let iinf = boxes
            .get("iinf")
            .map(|iinf| IinfBox::parse_box(iinf.data))
            .transpose()?
            .map(|x| x.1);

        // parse iloc box
        let iloc = boxes
            .get("iloc")
            .map(|iloc| IlocBox::parse_box(iloc.data))
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
            Self {
                header,
                iinf,
                iloc,
                // idat,
            },
        ))
    }
}

impl MetaBox {
    #[tracing::instrument(skip_all)]
    pub fn exif_data<'a>(&self, input: &'a [u8]) -> IResult<&'a [u8], Option<&'a [u8]>> {
        self.iinf
            .as_ref()
            .and_then(|iinf| iinf.get_infe("Exif"))
            .and_then(|exif_infe| {
                self.iloc
                    .as_ref()
                    .and_then(|iloc| iloc.item_offset_len(exif_infe.id))
            })
            .map(|(construction_method, offset, length)| {
                let (Ok(start), Ok(end)) = (offset.try_into(), (offset + length).try_into()) else {
                    return Err(nom::Err::Failure(nom::error::Error::new(
                        input,
                        nom::error::ErrorKind::TooLarge,
                    )));
                };

                if construction_method == 0 {
                    // file offset
                    if end > input.len() {
                        Err(nom::Err::Incomplete(Needed::new(end - input.len())))
                    } else {
                        Ok((&input[end..], Some(&input[start..end]))) // Safe-slice
                    }
                } else if construction_method == 1 {
                    // idat offset
                    tracing::debug!("idat offset construction method is not supported yet");
                    fail(input)
                } else {
                    tracing::debug!("item offset construction method is not supported yet");
                    fail(input)
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
    use crate::{bbox::travel_while, testkit::read_sample};

    use super::*;
    use test_case::test_case;

    #[test_case("exif.heic", 2618)]
    fn meta(path: &str, meta_size: usize) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let (_, bbox) = travel_while(&buf, |bbox| {
            tracing::info!(bbox.header.box_type, "Got");
            bbox.box_type() != "meta"
        })
        .unwrap();
        let bbox = bbox.unwrap();

        assert_eq!(bbox.data.len() as u64, bbox.box_size());
        let (remain, meta) = MetaBox::parse_box(bbox.data).unwrap();
        assert_eq!(remain, b"");
        assert_eq!(meta.header.box_type, "meta");
        assert_eq!(meta.exif_data(&buf).unwrap().1.unwrap().len(), meta_size);
    }
}

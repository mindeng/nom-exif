use std::fmt::{Debug, Display};

use nom::{
    bytes::streaming,
    combinator::{fail, map_res},
    error::context,
    number, AsChar, IResult, Needed,
};

mod idat;
mod iinf;
mod iloc;
mod ilst;
mod keys;
mod meta;
mod mvhd;
mod tkhd;
pub use ilst::IlstBox;
pub use keys::KeysBox;
pub use meta::MetaBox;
pub use mvhd::MvhdBox;
pub use tkhd::parse_video_tkhd_in_moov;

const MAX_BODY_LEN: usize = 2000 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub enum Error {
    UnsupportedConstructionMethod(u8),
}

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnsupportedConstructionMethod(x) => {
                Debug::fmt(&format!("unsupported construction method ({x})"), f)
            }
        }
    }
}

/// Representing an ISO base media file format box header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxHeader {
    pub box_size: u64,
    pub box_type: String,
    pub header_size: usize, // include size, type
}

impl BoxHeader {
    pub fn parse<'a>(input: &'a [u8]) -> IResult<&'a [u8], BoxHeader> {
        let (remain, size) = number::streaming::be_u32(input)?;

        let (remain, box_type) = map_res(streaming::take(4_usize), |res: &'a [u8]| {
            // String::from_utf8 will fail on "©xyz"
            Ok::<String, ()>(res.iter().map(|b| b.as_char()).collect::<String>())
            // String::from_utf8(res.to_vec()).map_err(|error| {
            //     tracing::error!(?error, ?res, "Failed to construct string");
            //     error
            // })
        })(remain)?;

        let (remain, box_size) = if size == 1 {
            number::streaming::be_u64(remain)?
        } else if size < 8 {
            context("invalid box header: box_size is too small", fail)(remain)?
        } else {
            (remain, size as u64)
        };

        let header_size = input.len() - remain.len();
        assert!(header_size == 8 || header_size == 16);

        if box_size < header_size as u64 {
            return fail(remain);
        }

        Ok((
            remain,
            BoxHeader {
                box_size,
                box_type,
                header_size,
            },
        ))
    }

    pub fn body_size(&self) -> u64 {
        self.box_size - self.header_size as u64
    }
}

/// Representing an ISO base media file format full box header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullBoxHeader {
    pub box_size: u64,
    pub box_type: String,
    pub header_size: usize, // include size, type, version, flags

    version: u8, // 8 bits
    flags: u32,  // 24 bits
}

impl FullBoxHeader {
    fn parse(input: &[u8]) -> IResult<&[u8], FullBoxHeader> {
        let (remain, header) = BoxHeader::parse(input)?;

        let (remain, version) = number::streaming::u8(remain)?;
        let (remain, flags) = number::streaming::be_u24(remain)?;

        let header_size = input.len() - remain.len();
        assert!(header_size == 12 || header_size == 20);

        if header.box_size < header_size as u64 {
            return fail(remain);
        }

        Ok((
            remain,
            FullBoxHeader {
                box_type: header.box_type,
                box_size: header.box_size,
                header_size,
                version,
                flags,
            },
        ))
    }

    pub fn body_size(&self) -> u64 {
        self.box_size - self.header_size as u64
    }
}

/// Representing a generic ISO base media file format box.
#[derive(Clone, PartialEq, Eq)]
pub struct BoxHolder<'a> {
    pub header: BoxHeader,
    // Including header
    pub data: &'a [u8],
}

impl Debug for BoxHolder<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxHolder")
            .field("header", &self.header)
            .field("body_size", &self.body_data().len())
            .field(
                "data",
                &(self
                    .body_data()
                    .iter()
                    .take(64)
                    .map(|x| x.as_char())
                    .collect::<String>()),
            )
            .finish()
    }
}

impl<'a> BoxHolder<'a> {
    #[tracing::instrument(skip_all)]
    pub fn parse(input: &'a [u8]) -> IResult<&'a [u8], BoxHolder<'a>> {
        let (_, header) = BoxHeader::parse(input)?;
        tracing::debug!(box_type = header.box_type, ?header, "Got");

        let box_size = usize::try_from(header.box_size)
            .expect("header box size should always fit into a `usize`.");

        let (remain, data) = streaming::take(box_size)(input)?;

        Ok((remain, BoxHolder { header, data }))
    }

    #[allow(unused)]
    pub fn box_size(&self) -> u64 {
        self.header.box_size
    }

    pub fn box_type(&self) -> &str {
        &self.header.box_type
    }

    pub fn header_size(&self) -> usize {
        self.header.header_size
    }

    pub fn body_data(&self) -> &'a [u8] {
        &self.data[self.header_size()..] // Safe-slice
    }
}

type BoxResult<'a> = IResult<&'a [u8], Option<BoxHolder<'a>>>;

pub fn to_boxes(input: &[u8]) -> crate::Result<Vec<BoxHolder<'_>>> {
    let mut res = Vec::new();
    let mut remain = input;
    loop {
        if remain.is_empty() {
            break;
        }

        let (rem, bbox) = BoxHolder::parse(remain)?;
        res.push(bbox);
        // Sanity check, to avoid infinite loops caused by unexpected errors.
        assert!(rem.len() < remain.len());
        remain = rem;
    }
    Ok(res)
}

/// Parses every top level box while `predicate` returns true, then returns the
/// last parsed box.
pub fn travel_while<'a, F>(input: &'a [u8], mut predicate: F) -> BoxResult<'a>
where
    F: FnMut(&BoxHolder<'a>) -> bool,
{
    let mut remain = input;
    loop {
        if remain.is_empty() {
            return Ok((remain, None));
        }

        let (rem, bbox) = BoxHolder::parse(remain)?;
        // Sanity check, to avoid infinite loops caused by unexpected errors.
        assert!(rem.len() < remain.len());
        remain = rem;

        if !predicate(&bbox) {
            return Ok((remain, Some(bbox)));
        }
    }
}

pub fn travel_header<'a, F>(input: &'a [u8], mut predicate: F) -> IResult<&'a [u8], BoxHeader>
where
    F: FnMut(&BoxHeader, &'a [u8]) -> bool,
{
    let mut remain = input;
    loop {
        let (rem, header) = BoxHeader::parse(remain)?;
        // Sanity check, to avoid infinite loops caused by unexpected errors.
        assert!(rem.len() < remain.len());
        remain = rem;

        if !predicate(&header, rem) {
            break Ok((rem, header));
        }

        if remain.len() < header.body_size() as usize {
            return Err(nom::Err::Incomplete(Needed::new(
                header.body_size() as usize - remain.len(),
            )));
        }

        // skip box body
        remain = &remain[header.body_size() as usize..]; // Safe-slice
    }
}

#[allow(unused)]
/// Find a box by atom `path`, which is separated by '/', e.g.: "meta/iloc".
pub fn find_box<'a>(input: &'a [u8], path: &str) -> IResult<&'a [u8], Option<BoxHolder<'a>>> {
    if path.is_empty() {
        return Ok((input, None));
    }

    let mut bbox = None;
    let mut remain = input;
    let mut data = input;

    for box_type in path.split('/').filter(|x| !x.is_empty()) {
        assert!(!box_type.is_empty());

        let (rem, b) = find_box_by_type(data, box_type)?;
        let Some(b) = b else {
            return Ok((rem, None));
        };

        data = b.body_data();
        (remain, bbox) = (rem, Some(b));
    }

    Ok((remain, bbox))
}

fn find_box_by_type<'a>(
    input: &'a [u8],
    box_type: &str,
) -> IResult<&'a [u8], Option<BoxHolder<'a>>> {
    let mut remain = input;
    loop {
        if remain.is_empty() {
            return Ok((remain, None));
        }

        let (rem, bbox) = BoxHolder::parse(remain)?;
        // Sanity check, to avoid infinite loops caused by unexpected errors.
        assert!(rem.len() < remain.len());
        remain = rem;

        if bbox.box_type() == box_type {
            return Ok((rem, Some(bbox)));
        }
    }
}

trait ParseBody<O> {
    fn parse_body(body: &[u8], header: FullBoxHeader) -> IResult<&[u8], O>;
}

pub trait ParseBox<O> {
    fn parse_box(input: &[u8]) -> IResult<&[u8], O>;
}

/// auto implements parse_box for each Box which implements ParseBody
impl<O, T: ParseBody<O>> ParseBox<O> for T {
    #[tracing::instrument(skip_all)]
    fn parse_box(input: &[u8]) -> IResult<&[u8], O> {
        let (remain, header) = FullBoxHeader::parse(input)?;
        assert_eq!(input.len(), header.header_size + remain.len());
        assert!(
            header.box_size >= header.header_size as u64,
            "box_size = {}, header_size = {}",
            header.box_size,
            header.header_size
        );

        // limit parsing size
        let box_size = header.body_size() as usize;
        if box_size > MAX_BODY_LEN {
            tracing::error!(?header.box_type, ?box_size, "Box is too big");
            return fail(remain);
        }
        let (remain, data) = streaming::take(box_size)(remain)?;
        assert_eq!(input.len(), header.header_size + data.len() + remain.len());

        let (rem, bbox) = Self::parse_body(data, header)?;

        if !rem.is_empty() {
            // TODO: Body data is not exhausted, should report this error with
            // tracing
        }

        Ok((remain, bbox))
    }
}

#[cfg(test)]
mod tests {
    use crate::testkit::read_sample;

    use super::*;
    use nom::error::make_error;
    use test_case::test_case;

    #[test_case("exif.heic")]
    fn travel_heic(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let mut boxes = Vec::new();

        let (remain, bbox) = travel_while(&buf, |bbox| {
            tracing::info!(bbox.header.box_type, "Got");
            boxes.push((bbox.header.box_type.to_owned(), bbox.to_owned()));
            bbox.box_type() != "mdat"
        })
        .unwrap();
        let bbox = bbox.unwrap();

        assert_eq!(bbox.header.box_type, "mdat");
        assert_eq!(remain, b"");

        let (types, _): (Vec<_>, Vec<_>) = boxes.iter().cloned().unzip();

        // top level boxes
        assert_eq!(types, ["ftyp", "meta", "mdat"],);

        let (_, meta) = boxes.remove(1);
        assert_eq!(meta.box_type(), "meta");

        let mut boxes = Vec::new();
        let (remain, bbox) = travel_while(
            &meta.body_data()[4..], // Safe-slice in test_case
            |bbox| {
                tracing::info!(bbox.header.box_type, "Got");
                boxes.push(bbox.header.box_type.to_owned());
                bbox.box_type() != "iloc"
            },
        )
        .unwrap();
        let bbox = bbox.unwrap();
        assert_eq!(bbox.box_type(), "iloc");
        assert_eq!(remain, b"");

        // sub-boxes in meta
        assert_eq!(
            boxes,
            ["hdlr", "dinf", "pitm", "iinf", "iref", "iprp", "idat", "iloc"],
        );
    }

    #[test_case("meta.mov")]
    fn travel_mov(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let mut boxes = Vec::new();

        let (remain, bbox) = travel_while(&buf, |bbox| {
            tracing::info!(bbox.header.box_type, "Got");
            boxes.push((bbox.header.box_type.to_owned(), bbox.to_owned()));
            bbox.box_type() != "moov"
        })
        .unwrap();
        let bbox = bbox.unwrap();

        assert_eq!(bbox.header.box_type, "moov");
        assert_eq!(remain, b"");

        let (types, _): (Vec<_>, Vec<_>) = boxes.iter().cloned().unzip();

        // top level boxes
        assert_eq!(types, ["ftyp", "wide", "mdat", "moov"],);

        let (_, moov) = boxes.pop().unwrap();
        assert_eq!(moov.box_type(), "moov");

        let mut boxes = Vec::new();
        let (remain, bbox) = travel_while(moov.body_data(), |bbox| {
            tracing::info!(bbox.header.box_type, "Got");
            boxes.push(bbox.header.box_type.to_owned());
            bbox.box_type() != "meta"
        })
        .unwrap();
        let bbox = bbox.unwrap();

        assert_eq!(bbox.box_type(), "meta");
        assert_eq!(remain, b"");

        // sub-boxes in moov
        assert_eq!(boxes, ["mvhd", "trak", "trak", "trak", "trak", "meta"],);

        let meta = bbox;
        let mut boxes = Vec::new();
        let (remain, _) = travel_while(meta.body_data(), |bbox| {
            tracing::info!(bbox.header.box_type, "Got");
            boxes.push(bbox.header.box_type.to_owned());
            bbox.box_type() != "ilst"
        })
        .unwrap();
        assert_eq!(remain, b"");

        // sub-boxes in meta
        assert_eq!(boxes, ["hdlr", "keys", "ilst"],);
    }

    #[test_case("meta.mp4")]
    fn travel_mp4(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let mut boxes = Vec::new();

        let (remain, bbox) = travel_while(&buf, |bbox| {
            tracing::info!(bbox.header.box_type, "Got");
            boxes.push((bbox.header.box_type.to_owned(), bbox.to_owned()));
            bbox.box_type() != "moov"
        })
        .unwrap();
        let bbox = bbox.unwrap();

        assert_eq!(bbox.header.box_type, "moov");
        assert_eq!(remain, b"");

        let (types, _): (Vec<_>, Vec<_>) = boxes.iter().cloned().unzip();

        // top level boxes
        assert_eq!(types, ["ftyp", "mdat", "moov"],);

        let (_, moov) = boxes.pop().unwrap();
        assert_eq!(moov.box_type(), "moov");

        let mut boxes = Vec::new();
        let (remain, bbox) = travel_while(moov.body_data(), |bbox| {
            tracing::info!(bbox.header.box_type, "Got");
            boxes.push((bbox.header.box_type.to_owned(), bbox.to_owned()));
            bbox.box_type() != "udta"
        })
        .unwrap();
        let bbox = bbox.unwrap();

        assert_eq!(bbox.box_type(), "udta");
        assert_eq!(remain, b"");

        // sub-boxes in moov
        assert_eq!(
            boxes.iter().map(|x| x.0.to_owned()).collect::<Vec<_>>(),
            ["mvhd", "trak", "trak", "udta"],
        );

        let (_, trak) = boxes.iter().find(|x| x.0 == "trak").unwrap();

        let meta = bbox;
        let mut boxes = Vec::new();
        let (remain, _) = travel_while(meta.body_data(), |bbox| {
            tracing::info!(bbox.header.box_type, "Got");
            boxes.push(bbox.header.box_type.to_owned());
            bbox.box_type() != "©xyz"
        })
        .unwrap();
        assert_eq!(remain, b"");

        // sub-boxes in udta
        assert_eq!(boxes, ["©xyz"],);

        let mut boxes = Vec::new();
        let (remain, bbox) = travel_while(trak.body_data(), |bbox| {
            tracing::info!(bbox.header.box_type, "Got");
            boxes.push(bbox.header.box_type.to_owned());
            bbox.box_type() != "mdia"
        })
        .unwrap();
        assert_eq!(remain, b"");

        // sub-boxes in trak
        assert_eq!(boxes, ["tkhd", "edts", "mdia"],);

        let mdia = bbox.unwrap();
        let mut boxes = Vec::new();
        let (remain, _) = travel_while(mdia.body_data(), |bbox| {
            tracing::info!(bbox.header.box_type, "Got");
            boxes.push(bbox.header.box_type.to_owned());
            bbox.box_type() != "minf"
        })
        .unwrap();
        assert_eq!(remain, b"");

        // sub-boxes in mdia
        assert_eq!(boxes, ["mdhd", "hdlr", "minf"],);
    }

    // For mp4 files, Android phones store GPS info in the `moov/udta/©xyz`
    // atom.
    #[test_case("meta.mp4")]
    fn find_android_gps_box(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let (_, bbox) = find_box(&buf, "moov/udta/©xyz").unwrap();
        let bbox = bbox.unwrap();
        tracing::info!(?bbox.header, "bbox");

        // gps info
        assert_eq!(
            "+27.2939+112.6932/",
            std::str::from_utf8(&bbox.body_data()[4..]).unwrap() // Safe-slice in test_case
        );
    }

    #[test]
    fn box_header() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let data = [
            0x00, 0x00, 0x01, 0xdd, 0x6d, 0x65, 0x74, 0x61, 0x02, 0x04, 0x04, 0x00,
        ];
        let (remain, header) = FullBoxHeader::parse(&data).unwrap();
        assert_eq!(header.box_type, "meta");
        assert_eq!(header.box_size, 0x01dd);
        assert_eq!(header.version, 0x2);
        assert_eq!(header.flags, 0x40400,);
        assert_eq!(header.header_size, 12);
        assert_eq!(remain, b"");

        let data = [
            0x00, 0x00, 0x00, 0x01, 0x6d, 0x64, 0x61, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0e,
            0xfa, 0x74, 0x01, 0x04, 0x04, 0x00,
        ];
        let (remain, header) = FullBoxHeader::parse(&data).unwrap();
        assert_eq!(header.box_type, "mdat");
        assert_eq!(header.box_size, 0xefa74);
        assert_eq!(header.version, 0x1);
        assert_eq!(header.flags, 0x40400,);
        assert_eq!(header.header_size, 20);
        assert_eq!(remain, b"");

        let data = [0x00, 0x00, 0x01, 0xdd, 0x6d, 0x65, 0x74];
        let err = BoxHeader::parse(&data).unwrap_err();
        assert!(err.is_incomplete());

        let data = [0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00];
        let err = BoxHeader::parse(&data).unwrap_err();
        assert_eq!(
            err,
            nom::Err::Error(make_error(&[] as &[u8], nom::error::ErrorKind::Fail))
        );
    }
}

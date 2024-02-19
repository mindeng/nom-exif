use std::fmt::Display;

use nom::{
    bytes::{complete, streaming},
    combinator::{fail, map_res},
    error::context,
    number, AsChar, IResult,
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

#[derive(Debug, PartialEq)]
pub enum Error {
    UnsupportedConstructionMethod(u8),
}

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnsupportedConstructionMethod(x) => {
                write!(f, "unsupported construction method ({x})")
            }
        }
    }
}

/// Representing an ISO base media file format box header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxHeader {
    pub box_size: u64,
    pub box_type: String,
    pub header_size: usize,
}

impl BoxHeader {
    pub fn parse<'a>(input: &'a [u8]) -> IResult<&'a [u8], BoxHeader> {
        let (remain, size) = number::streaming::be_u32(input)?;

        let (remain, box_type) = map_res(streaming::take(4 as usize), |res: &'a [u8]| {
            // String::from_utf8 will fail on "©xyz"
            Ok::<String, ()>(res.iter().map(|b| b.as_char()).collect::<String>())
            // String::from_utf8(res.to_vec()).map_err(|e| {
            //     eprintln!("{e:?}");
            //     e
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
    pub header_size: usize,

    version: u8, // 8 bits
    flags: u32,  // 24 bits
}

impl FullBoxHeader {
    fn parse<'a>(input: &'a [u8]) -> IResult<&'a [u8], FullBoxHeader> {
        let (remain, header) = BoxHeader::parse(input)?;

        let (remain, version) = number::streaming::u8(remain)?;
        let (remain, flags) = number::streaming::be_u24(remain)?;

        let header_size = input.len() - remain.len();
        assert!(header_size == 12 || header_size == 20);

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
}

/// Representing a generic ISO base media file format box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxHolder<'a> {
    pub header: BoxHeader,
    // Including header
    pub data: &'a [u8],
}

impl<'a> BoxHolder<'a> {
    pub fn parse(input: &'a [u8]) -> IResult<&'a [u8], BoxHolder<'a>> {
        let (_, header) = BoxHeader::parse(input)?;
        let (remain, data) = streaming::take(header.box_size)(input)?;
        // println!("got {} {}", header.box_type, data.len());

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
        &self.data[self.header.header_size..]
    }
}

/// Parses every top level box while `predicate` returns true, then returns the
/// last parsed box.
pub fn travel_while<'a, F>(input: &'a [u8], mut predicate: F) -> IResult<&'a [u8], BoxHolder<'a>>
where
    F: FnMut(&BoxHolder<'a>) -> bool,
{
    let mut remain = input;
    loop {
        let (rem, bbox) = BoxHolder::parse(remain)?;
        // Sanity check, to avoid infinite loops caused by unexpected errors.
        assert!(rem.len() < remain.len());
        remain = rem;

        if predicate(&bbox) == false {
            break Ok((rem, bbox));
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

        if predicate(&header, rem) == false {
            break Ok((rem, header));
        }

        // skip box body
        remain = &remain[(header.box_size - header.header_size as u64) as usize..];
    }
}

#[allow(unused)]
/// Find a box by atom `path`, which is separated by '/', e.g.: "meta/iloc".
pub fn find_box<'a>(input: &'a [u8], path: &str) -> IResult<&'a [u8], Option<BoxHolder<'a>>> {
    if path.is_empty() {
        context("path is empty", fail::<_, BoxHolder<'a>, _>)(input)?;
    }

    let mut bbox = None;
    let mut remain = input;
    let mut data = input;

    for box_type in path.split('/') {
        if box_type.is_empty() {
            continue;
        }
        let (rem, b) = travel_while(data, |b| b.box_type() != box_type)?;
        data = &b.data[b.header_size()..];
        (remain, bbox) = (rem, Some(b));
    }

    Ok((remain, bbox))

    // let (box_type, remain_path) = path.split_once('/').unwrap_or_else(|| (path, ""));

    // let data = if !box_type.is_empty() {
    //     let (remain, bbox) = travel_while(input, |b| b.box_type() != box_type)?;
    //     println!("got box: {:?}", bbox.header);

    //     if remain_path.is_empty() {
    //         return Ok((remain, bbox));
    //     }

    //     &bbox.data[bbox.header_size()..]
    // } else {
    //     input
    // };

    // find_box(data, remain_path)
}

trait ParseBody<O> {
    fn parse_body<'a>(body: &'a [u8], header: FullBoxHeader) -> IResult<&'a [u8], O>;
}

pub trait ParseBox<O> {
    fn parse_box<'a>(input: &'a [u8]) -> IResult<&'a [u8], O>;
}

/// auto implements parse_box for each Box which implements ParseBody
impl<O, T: ParseBody<O>> ParseBox<O> for T {
    fn parse_box<'a>(input: &'a [u8]) -> IResult<&'a [u8], O> {
        let (remain, header) = FullBoxHeader::parse(input)?;
        assert_eq!(input.len(), header.header_size as usize + remain.len());

        // limit parsing size
        let body_len = header.box_size as usize - header.header_size;
        let (remain, data) = complete::take(body_len)(remain)?;
        assert_eq!(
            input.len(),
            header.header_size as usize + data.len() + remain.len()
        );

        let (rem, bbox) = Self::parse_body(data, header)?;
        assert_eq!(rem.len(), 0);

        Ok((remain, bbox))
    }
}

fn parse_cstr(input: &[u8]) -> IResult<&[u8], String> {
    let (remain, s) = map_res(streaming::take_till(|b| b == 0), |bs: &[u8]| {
        if bs.len() == 0 {
            Ok("".to_owned())
        } else {
            String::from_utf8(bs.to_vec())
        }
    })(input)?;

    // consumes the zero byte
    Ok((&remain[1..], s))
}

pub fn get_ftyp(input: &[u8]) -> crate::Result<Option<&[u8]>> {
    let (remain, header) = travel_header(input, |h, _| {
        // MOV files that extracted from HEIC starts with `wide` & `mdat` atoms
        h.box_type != "ftyp" && h.box_type != "mdat"
    })?;

    assert!(header.box_type == "ftyp" || header.box_type == "mdat");

    if header.box_type == "ftyp" {
        if header.body_size() < 4 {
            return Err(format!(
                "Invalid ftyp box; body size should greater than 4, got {}",
                header.body_size()
            )
            .into());
        }
        let (_, ftyp) = streaming::take(4 as usize)(remain)?;
        Ok(Some(ftyp))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use crate::testkit::open_sample;

    use super::*;
    use nom::error::make_error;
    use test_case::test_case;

    #[test_case("exif.heic")]
    fn travel_heic(path: &str) {
        let mut reader = open_sample(path).unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(buf.as_mut()).unwrap();
        let mut boxes = Vec::new();

        let (remain, bbox) = travel_while(&buf, |bbox| {
            // println!("got {}", bbox.header.box_type);
            boxes.push((bbox.header.box_type.to_owned(), bbox.to_owned()));
            bbox.box_type() != "mdat"
        })
        .unwrap();

        assert_eq!(bbox.header.box_type, "mdat");
        assert_eq!(remain, b"");

        let (types, _): (Vec<_>, Vec<_>) = boxes.iter().cloned().unzip();

        // top level boxes
        assert_eq!(types, ["ftyp", "meta", "mdat"],);

        let (_, meta) = boxes.remove(1);
        assert_eq!(meta.box_type(), "meta");

        let mut boxes = Vec::new();
        let (remain, bbox) = travel_while(&meta.body_data()[4..], |bbox| {
            println!("got {}", bbox.header.box_type);
            boxes.push(bbox.header.box_type.to_owned());
            bbox.box_type() != "iloc"
        })
        .unwrap();
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
        let mut reader = open_sample(path).unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(buf.as_mut()).unwrap();
        let mut boxes = Vec::new();

        let (remain, bbox) = travel_while(&buf, |bbox| {
            println!("got {}", bbox.header.box_type);
            boxes.push((bbox.header.box_type.to_owned(), bbox.to_owned()));
            bbox.box_type() != "moov"
        })
        .unwrap();

        assert_eq!(bbox.header.box_type, "moov");
        assert_eq!(remain, b"");

        let (types, _): (Vec<_>, Vec<_>) = boxes.iter().cloned().unzip();

        // top level boxes
        assert_eq!(types, ["ftyp", "wide", "mdat", "moov"],);

        let (_, moov) = boxes.pop().unwrap();
        assert_eq!(moov.box_type(), "moov");

        let mut boxes = Vec::new();
        let (remain, bbox) = travel_while(moov.body_data(), |bbox| {
            println!("got {}", bbox.header.box_type);
            boxes.push(bbox.header.box_type.to_owned());
            bbox.box_type() != "meta"
        })
        .unwrap();
        assert_eq!(bbox.box_type(), "meta");
        assert_eq!(remain, b"");

        // sub-boxes in moov
        assert_eq!(boxes, ["mvhd", "trak", "trak", "trak", "trak", "meta"],);

        let meta = bbox;
        let mut boxes = Vec::new();
        let (remain, _) = travel_while(meta.body_data(), |bbox| {
            println!("got {}", bbox.header.box_type);
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
        let mut reader = open_sample(path).unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(buf.as_mut()).unwrap();
        let mut boxes = Vec::new();

        let (remain, bbox) = travel_while(&buf, |bbox| {
            println!("got {}", bbox.header.box_type);
            boxes.push((bbox.header.box_type.to_owned(), bbox.to_owned()));
            bbox.box_type() != "moov"
        })
        .unwrap();

        assert_eq!(bbox.header.box_type, "moov");
        assert_eq!(remain, b"");

        let (types, _): (Vec<_>, Vec<_>) = boxes.iter().cloned().unzip();

        // top level boxes
        assert_eq!(types, ["ftyp", "mdat", "moov"],);

        let (_, moov) = boxes.pop().unwrap();
        assert_eq!(moov.box_type(), "moov");

        let mut boxes = Vec::new();
        let (remain, bbox) = travel_while(moov.body_data(), |bbox| {
            println!("got {}", bbox.header.box_type);
            boxes.push((bbox.header.box_type.to_owned(), bbox.to_owned()));
            bbox.box_type() != "udta"
        })
        .unwrap();
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
            println!("got {}", bbox.header.box_type);
            boxes.push(bbox.header.box_type.to_owned());
            bbox.box_type() != "©xyz"
        })
        .unwrap();
        assert_eq!(remain, b"");

        // sub-boxes in udta
        assert_eq!(boxes, ["©xyz"],);

        let mut boxes = Vec::new();
        let (remain, bbox) = travel_while(trak.body_data(), |bbox| {
            println!("got {}", bbox.header.box_type);
            boxes.push(bbox.header.box_type.to_owned());
            bbox.box_type() != "mdia"
        })
        .unwrap();
        assert_eq!(remain, b"");

        // sub-boxes in trak
        assert_eq!(boxes, ["tkhd", "edts", "mdia"],);

        let mdia = bbox;
        let mut boxes = Vec::new();
        let (remain, _) = travel_while(mdia.body_data(), |bbox| {
            println!("got {}", bbox.header.box_type);
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
        let mut f = open_sample(path).unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();

        // let (_, bbox) = travel_while(&buf, |b| b.box_type() != "moov").unwrap();
        // println!("bbox: {:?}", bbox.header);
        // let (_, bbox) = travel_while(bbox.body_data(), |b| b.box_type() != "udta").unwrap();
        // println!("bbox: {:?}", bbox.header);
        // let (_, bbox) = travel_while(bbox.body_data(), |b| b.box_type() != "©xyz").unwrap();

        let (_, bbox) = find_box(&buf, "moov/udta/©xyz").unwrap();
        let bbox = bbox.unwrap();
        println!("bbox: {:?}", bbox.header);

        // gps info
        assert_eq!(
            "+27.2939+112.6932/",
            std::str::from_utf8(&bbox.body_data()[4..]).unwrap()
        );
    }

    #[test]
    fn box_header() {
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

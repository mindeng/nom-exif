use nom::{
    bytes::complete::take,
    number::complete::{be_u16, be_u32, be_u64},
    sequence::tuple,
};

use super::{FullBoxHeader, ParseBody};

/// Represents a [movie header atom][1].
///
/// tkhd is a fullbox which contains version & flags.
///
/// atom-path: moov/trak/tkhd
///
/// [1]: https://developer.apple.com/documentation/quicktime-file-format/track_header_atom
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TkhdBox {
    header: FullBoxHeader,

    /// seconds since midnight, January 1, 1904
    creation_time: u32,

    /// seconds since midnight, January 1, 1904
    modification_time: u32,

    track_id: u32,
    // reserved: u32,
    duration: u32,
    // reserved2: u64,
    layer: u16,
    alt_group: u16,
    volume: u16,
    // reserved3: u16,

    // matrix: [u8; 36],
    width: u32,
    height: u32,
}

impl ParseBody<TkhdBox> for TkhdBox {
    fn parse_body<'a>(body: &'a [u8], header: FullBoxHeader) -> nom::IResult<&'a [u8], TkhdBox> {
        let (
            remain,
            (
                creation_time,
                modification_time,
                track_id,
                _,
                duration,
                _,
                layer,
                alt_group,
                volume,
                _,
                _,
                width,
                _,
                height,
                _,
            ),
        ) = tuple((
            be_u32,
            be_u32,
            be_u32,
            be_u32,
            be_u32,
            be_u64,
            be_u16,
            be_u16,
            be_u16,
            be_u16,
            take(36usize),
            be_u16,
            be_u16,
            be_u16,
            be_u16,
        ))(body)?;

        Ok((
            remain,
            TkhdBox {
                header,
                creation_time,
                modification_time,
                track_id,
                duration,
                layer,
                alt_group,
                volume,
                width: width as u32,
                height: height as u32,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read, path::Path};

    use crate::bbox::{find_box, travel_while, ParseBox};

    use super::*;
    use test_case::test_case;

    #[test_case("meta.mov", 720, 1280)]
    #[test_case("meta.mp4", 1920, 1080)]
    fn tkhd_box(path: &str, width: u32, height: u32) {
        let mut f = open_sample(path);
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();

        let (_, bbox) = travel_while(&buf, |b| b.box_type() != "moov").unwrap();
        let (_, bbox) = travel_while(bbox.body_data(), |b| {
            // find video track
            if b.box_type() != "trak" {
                true
            } else {
                let Some(hdlr) = find_box(b.body_data(), "mdia/hdlr").unwrap().1 else {
                    return true;
                };

                // component subtype
                let subtype = &hdlr.body_data()[8..12];
                if subtype == b"vide" {
                    false
                } else {
                    true
                }
            }
        })
        .unwrap();
        let (_, bbox) = travel_while(bbox.body_data(), |b| b.box_type() != "tkhd").unwrap();
        let (remain, tkhd) = TkhdBox::parse_box(bbox.data).unwrap();
        assert_eq!(remain.len(), 0);

        assert_eq!(tkhd.width, width);
        assert_eq!(tkhd.height, height);
    }

    fn open_sample(path: &str) -> File {
        let p = Path::new(path);
        if p.is_absolute() {
            File::open(p).unwrap()
        } else {
            File::open(Path::new("./testdata").join(p)).unwrap()
        }
    }
}

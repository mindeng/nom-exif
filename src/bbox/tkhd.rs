use nom::{
    bytes::complete::take,
    number::complete::{be_u16, be_u32, be_u64},
    sequence::tuple,
};

use super::{find_box, travel_while, BoxHolder, FullBoxHeader, ParseBody, ParseBox};

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
    pub width: u32,
    pub height: u32,
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

/// Try to find a video track's tkhd in moov body. atom-path: "moov/trak/tkhd".
pub fn parse_video_tkhd_in_moov<'a>(input: &'a [u8]) -> crate::Result<TkhdBox> {
    let bbox = find_video_track(input)?;
    let (_, bbox) = travel_while(bbox.body_data(), |b| b.box_type() != "tkhd")
        .map_err(|e| format!("find tkhd failed: {e:?}"))?;
    let (remain, tkhd) = TkhdBox::parse_box(bbox.data).unwrap();
    assert_eq!(remain.len(), 0);
    Ok(tkhd)
}

fn find_video_track(input: &[u8]) -> crate::Result<BoxHolder> {
    let (_, bbox) = travel_while(input, |b| {
        // find video track
        if b.box_type() != "trak" {
            true
        } else {
            // got a 'trak', to check if it's a 'vide' trak

            let found = find_box(b.body_data(), "mdia/hdlr");
            let Ok(bbox) = found else {
                return true;
            };
            let Some(hdlr) = bbox.1 else {
                return true;
            };

            // component subtype
            if hdlr.body_data().len() < 4 {
                return true;
            }
            let subtype = &hdlr.body_data()[8..12]; // Safe-slice
            if subtype == b"vide" {
                // found it!
                false
            } else {
                true
            }
        }
    })
    .map_err(|e| format!("find vide trak failed: {e:?}"))?;

    Ok(bbox)
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read, path::Path};

    use crate::bbox::travel_while;

    use super::*;
    use test_case::test_case;

    #[test_case("meta.mov", 720, 1280)]
    #[test_case("meta.mp4", 1920, 1080)]
    fn tkhd_box(path: &str, width: u32, height: u32) {
        let mut f = open_sample(path);
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();

        let (_, bbox) = travel_while(&buf, |b| b.box_type() != "moov").unwrap();
        let tkhd = parse_video_tkhd_in_moov(bbox.body_data()).unwrap();

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

use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use nom::{bytes::complete::take, number::complete::be_u32, sequence::tuple};

use super::{FullBoxHeader, ParseBody};

/// Represents a [movie header atom][1].
///
/// mvhd is a fullbox which contains version & flags.
///
/// atom-path: moov/mvhd
///
/// [1]: https://developer.apple.com/documentation/quicktime-file-format/movie_header_atom
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MvhdBox {
    header: FullBoxHeader,

    /// seconds since midnight, January 1, 1904
    creation_time: u32,

    /// seconds since midnight, January 1, 1904
    modification_time: u32,

    /// The number of time units that pass per second in its time coordinate
    /// system.
    time_scale: u32,

    /// Indicates the duration of the movie in time scale units.
    ///
    /// # convert to seconds
    ///
    /// seconds = duration / time_scale
    duration: u32,
    // omit 76 bytes...
    next_track_id: u32,
}

impl MvhdBox {
    pub fn duration_ms(&self) -> u32 {
        (self.duration * 1000) / (self.time_scale)
    }

    fn creation_time_naive(&self) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(1904, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            + Duration::seconds(self.creation_time as i64)
    }

    pub fn creation_time(&self) -> DateTime<FixedOffset> {
        self.creation_time_utc().fixed_offset()
    }

    #[allow(dead_code)]
    pub fn creation_time_local(&self) -> DateTime<Local> {
        Local.from_utc_datetime(&self.creation_time_naive())
    }

    pub fn creation_time_utc(&self) -> DateTime<Utc> {
        self.creation_time_naive().and_utc()
    }
}

impl ParseBody<MvhdBox> for MvhdBox {
    fn parse_body<'a>(body: &'a [u8], header: FullBoxHeader) -> nom::IResult<&'a [u8], MvhdBox> {
        let (remain, (creation_time, modification_time, time_scale, duration, _, next_track_id)) =
            tuple((be_u32, be_u32, be_u32, be_u32, take(76usize), be_u32))(body)?;

        Ok((
            remain,
            MvhdBox {
                header,
                creation_time,
                modification_time,
                time_scale,
                duration,
                next_track_id,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read, path::Path};

    use crate::bbox::{travel_while, ParseBox};

    use super::*;
    use chrono::FixedOffset;
    use test_case::test_case;

    #[test_case(
        "meta.mov",
        "2024-02-02T08:09:57.000000Z",
        "2024-02-02T16:09:57+08:00",
        500
    )]
    #[test_case(
        "meta.mp4",
        "2024-02-03T07:05:38.000000Z",
        "2024-02-03T15:05:38+08:00",
        1063
    )]
    fn mvhd_box(path: &str, time_utc: &str, time_east8: &str, milliseconds: u32) {
        let mut f = open_sample(path);
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();

        let (_, bbox) = travel_while(&buf, |b| b.box_type() != "moov").unwrap();
        let (_, bbox) = travel_while(bbox.body_data(), |b| b.box_type() != "mvhd").unwrap();
        let (_, mvhd) = MvhdBox::parse_box(bbox.data).unwrap();

        assert_eq!(mvhd.duration_ms(), milliseconds);

        // time is represented in seconds since midnight, January 1, 1904,
        // preferably using coordinated universal time (UTC).
        let created = mvhd.creation_time_utc();
        assert_eq!(created, mvhd.creation_time());
        assert_eq!(
            created.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            time_utc
        );
        assert_eq!(
            created
                .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            time_east8
        );
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

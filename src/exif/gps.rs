use std::str::FromStr;

use iso6709parse::ISO6709Coord;

use crate::values::{IRational, URational};

/// Represents gps information stored in [`GPSInfo`](crate::ExifTag::GPSInfo)
/// subIFD.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GPSInfo {
    /// N, S
    pub latitude_ref: char,
    /// degree, minute, second,
    pub latitude: LatLng,

    /// E, W
    pub longitude_ref: char,
    /// degree, minute, second,
    pub longitude: LatLng,

    /// 0: Above Sea Level
    /// 1: Below Sea Level
    pub altitude_ref: u8,
    /// meters
    pub altitude: URational,

    /// Speed unit
    /// - K: kilometers per hour
    /// - M: miles per hour
    /// - N: knots
    pub speed_ref: Option<char>,
    pub speed: Option<URational>,
}

/// degree, minute, second,
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LatLng(pub URational, pub URational, pub URational);

impl GPSInfo {
    /// Returns an ISO 6709 geographic point location string such as
    /// `+48.8577+002.295/`.
    pub fn format_iso6709(&self) -> String {
        let latitude = self.latitude.0.as_float()
            + self.latitude.1.as_float() / 60.0
            + self.latitude.2.as_float() / 3600.0;
        let longitude = self.longitude.0.as_float()
            + self.longitude.1.as_float() / 60.0
            + self.longitude.2.as_float() / 3600.0;
        let altitude = self.altitude.as_float();
        format!(
            "{}{latitude:08.5}{}{longitude:09.5}{}/",
            if self.latitude_ref == 'N' { '+' } else { '-' },
            if self.longitude_ref == 'E' { '+' } else { '-' },
            if self.altitude.0 == 0 {
                "".to_string()
            } else {
                format!(
                    "{}{}CRSWGS_84",
                    if self.altitude_ref == 0 { "+" } else { "-" },
                    Self::format_float(altitude)
                )
            }
        )
    }

    fn format_float(f: f64) -> String {
        if f.fract() == 0.0 {
            f.to_string()
        } else {
            format!("{f:.3}")
        }
    }

    /// Returns an ISO 6709 geographic point location string such as
    /// `+48.8577+002.295/`.
    #[deprecated(since = "1.2.3", note = "please use `format_iso6709` instead")]
    #[allow(clippy::wrong_self_convention)]
    pub fn to_iso6709(&self) -> String {
        self.format_iso6709()
    }
}

impl From<[(u32, u32); 3]> for LatLng {
    fn from(value: [(u32, u32); 3]) -> Self {
        let res: [URational; 3] = value.map(|x| x.into());
        res.into()

        // value
        //     .into_iter()
        //     .map(|x| x.into())
        //     .collect::<Vec<URational>>()
        //     .try_into()
        //     .unwrap()
    }
}

impl From<[URational; 3]> for LatLng {
    fn from(value: [URational; 3]) -> Self {
        Self(value[0], value[1], value[2])
    }
}

impl FromIterator<(u32, u32)> for LatLng {
    fn from_iter<T: IntoIterator<Item = (u32, u32)>>(iter: T) -> Self {
        let rationals: Vec<URational> = iter.into_iter().take(3).map(|x| x.into()).collect();
        assert!(rationals.len() >= 3);
        rationals.try_into().unwrap()
    }
}

impl TryFrom<Vec<URational>> for LatLng {
    type Error = crate::Error;

    fn try_from(value: Vec<URational>) -> Result<Self, Self::Error> {
        if value.len() < 3 {
            Err("convert to LatLng failed; need at least 3 (u32, u32)".into())
        } else {
            Ok(Self(value[0], value[1], value[2]))
        }
    }
}

impl FromIterator<URational> for LatLng {
    fn from_iter<T: IntoIterator<Item = URational>>(iter: T) -> Self {
        let mut values = iter.into_iter();
        Self(
            values.next().unwrap(),
            values.next().unwrap(),
            values.next().unwrap(),
        )
    }
}

impl TryFrom<&Vec<URational>> for LatLng {
    type Error = crate::Error;
    fn try_from(value: &Vec<URational>) -> Result<Self, Self::Error> {
        if value.len() < 3 {
            Err(crate::Error::ParseFailed("invalid URational data".into()))
        } else {
            Ok(Self(value[0], value[1], value[2]))
        }
    }
}
impl TryFrom<&Vec<IRational>> for LatLng {
    type Error = crate::Error;
    fn try_from(value: &Vec<IRational>) -> Result<Self, Self::Error> {
        if value.len() < 3 {
            Err(crate::Error::ParseFailed("invalid URational data".into()))
        } else {
            Ok(Self(value[0].into(), value[1].into(), value[2].into()))
        }
    }
}
pub struct InvalidISO6709Coord;

impl FromStr for GPSInfo {
    type Err = InvalidISO6709Coord;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut coord: ISO6709Coord = iso6709parse::parse(s).map_err(|_| InvalidISO6709Coord)?;
        // `iso6709parse` only decodes the altitude field when a `CRS` tag
        // follows it, so Apple's default `±lat±lon±alt/` form (no CRS) loses
        // the altitude. Recover it ourselves. See #66.
        if coord.altitude.is_none() {
            coord.altitude = parse_iso6709_altitude(s);
        }
        Ok(coord.into())
    }
}

/// Extract the altitude (third signed numeric field) from an ISO 6709 string
/// representation such as `+47.7199-117.4931+522.171/`. Each field is a `+`/`-`
/// sign followed by a run of digits and dots, so a trailing `CRS…` suffix and
/// the closing `/` are skipped naturally. Returns `None` when there is no third
/// field (a plain `±lat±lon/` pair), so a two-field string never mistakes the
/// longitude for an altitude.
fn parse_iso6709_altitude(s: &str) -> Option<f64> {
    let bytes = s.as_bytes();
    let mut fields = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' || bytes[i] == b'-' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            fields += 1;
            if fields == 3 {
                return s[start..i].parse::<f64>().ok();
            }
        } else {
            i += 1;
        }
    }
    None
}

impl From<ISO6709Coord> for GPSInfo {
    fn from(v: ISO6709Coord) -> Self {
        // let latitude = self.latitude.0.as_float()
        //     + self.latitude.1.as_float() / 60.0
        //     + self.latitude.2.as_float() / 3600.0;

        Self {
            latitude_ref: if v.lat >= 0.0 { 'N' } else { 'S' },
            latitude: v.lat.abs().into(),
            longitude_ref: if v.lon >= 0.0 { 'E' } else { 'W' },
            longitude: v.lon.abs().into(),
            altitude_ref: v
                .altitude
                .map(|x| if x >= 0.0 { 0 } else { 1 })
                .unwrap_or(0),
            altitude: v
                .altitude
                .map(|x| ((x.abs() * 1000.0).trunc() as u32, 1000).into())
                .unwrap_or_default(),
            ..Default::default()
        }
    }
}

impl From<f64> for LatLng {
    fn from(v: f64) -> Self {
        let mins = v.fract() * 60.0;
        [
            (v.trunc() as u32, 1),
            (mins.trunc() as u32, 1),
            // The leftover fraction is a fraction *of a minute*, so it must be
            // scaled by 60 to reach seconds before storing hundredths. See #66.
            ((mins.fract() * 60.0 * 100.0).round() as u32, 100),
        ]
        .into()
    }
}

// impl<T: AsRef<[(u32, u32)]>> From<T> for LatLng {
//     fn from(value: T) -> Self {
//         assert!(value.as_ref().len() >= 3);
//         value.as_ref().iter().take(3).map(|x| x.into()).collect()
//     }
// }

// impl<T: AsRef<[URational]>> From<T> for LatLng {
//     fn from(value: T) -> Self {
//         assert!(value.as_ref().len() >= 3);
//         let s = value.as_ref();
//         Self(s[0], s[1], s[2])
//     }
// }

#[cfg(test)]
mod tests {
    use crate::values::Rational;

    use super::*;

    #[test]
    fn gps_iso6709() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let palace = GPSInfo {
            latitude_ref: 'N',
            latitude: LatLng(
                Rational::<u32>(39, 1),
                Rational::<u32>(55, 1),
                Rational::<u32>(0, 1),
            ),
            longitude_ref: 'E',
            longitude: LatLng(
                Rational::<u32>(116, 1),
                Rational::<u32>(23, 1),
                Rational::<u32>(27, 1),
            ),
            altitude_ref: 0,
            altitude: Rational::<u32>(0, 1),
            ..Default::default()
        };
        assert_eq!(palace.format_iso6709(), "+39.91667+116.39083/");

        let liberty = GPSInfo {
            latitude_ref: 'N',
            latitude: LatLng(
                Rational::<u32>(40, 1),
                Rational::<u32>(41, 1),
                Rational::<u32>(21, 1),
            ),
            longitude_ref: 'W',
            longitude: LatLng(
                Rational::<u32>(74, 1),
                Rational::<u32>(2, 1),
                Rational::<u32>(40, 1),
            ),
            altitude_ref: 0,
            altitude: Rational::<u32>(0, 1),
            ..Default::default()
        };
        assert_eq!(liberty.format_iso6709(), "+40.68917-074.04444/");

        let above = GPSInfo {
            latitude_ref: 'N',
            latitude: LatLng(
                Rational::<u32>(40, 1),
                Rational::<u32>(41, 1),
                Rational::<u32>(21, 1),
            ),
            longitude_ref: 'W',
            longitude: LatLng(
                Rational::<u32>(74, 1),
                Rational::<u32>(2, 1),
                Rational::<u32>(40, 1),
            ),
            altitude_ref: 0,
            altitude: Rational::<u32>(123, 1),
            ..Default::default()
        };
        assert_eq!(above.format_iso6709(), "+40.68917-074.04444+123CRSWGS_84/");

        let below = GPSInfo {
            latitude_ref: 'N',
            latitude: LatLng(
                Rational::<u32>(40, 1),
                Rational::<u32>(41, 1),
                Rational::<u32>(21, 1),
            ),
            longitude_ref: 'W',
            longitude: LatLng(
                Rational::<u32>(74, 1),
                Rational::<u32>(2, 1),
                Rational::<u32>(40, 1),
            ),
            altitude_ref: 1,
            altitude: Rational::<u32>(123, 1),
            ..Default::default()
        };
        assert_eq!(below.format_iso6709(), "+40.68917-074.04444-123CRSWGS_84/");

        let below = GPSInfo {
            latitude_ref: 'N',
            latitude: LatLng(
                Rational::<u32>(40, 1),
                Rational::<u32>(41, 1),
                Rational::<u32>(21, 1),
            ),
            longitude_ref: 'W',
            longitude: LatLng(
                Rational::<u32>(74, 1),
                Rational::<u32>(2, 1),
                Rational::<u32>(40, 1),
            ),
            altitude_ref: 1,
            altitude: Rational::<u32>(100, 3),
            ..Default::default()
        };
        assert_eq!(
            below.format_iso6709(),
            "+40.68917-074.04444-33.333CRSWGS_84/"
        );
    }

    #[test]
    fn gps_iso6709_altitude_without_crs() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        // `iso6709parse` itself drops the altitude when no `CRS` tag follows...
        let iso: ISO6709Coord = iso6709parse::parse("+26.5322-078.1969+019.099/").unwrap();
        assert_eq!(iso.lat, 26.5322);
        assert_eq!(iso.lon, -78.1969);
        assert_eq!(iso.altitude, None);

        // ...but GPSInfo::from_str recovers it, and the minute-fraction is now
        // scaled into the seconds slot correctly. See #66.
        let iso: GPSInfo = "+26.5322-078.1969+019.099/".parse().ok().unwrap();
        assert_eq!(iso.latitude_ref, 'N');
        assert_eq!(
            iso.latitude,
            LatLng(
                Rational::<u32>(26, 1),
                Rational::<u32>(31, 1),
                Rational::<u32>(5592, 100),
            )
        );

        assert_eq!(iso.longitude_ref, 'W');
        assert_eq!(
            iso.longitude,
            LatLng(
                Rational::<u32>(78, 1),
                Rational::<u32>(11, 1),
                Rational::<u32>(4884, 100),
            )
        );

        assert_eq!(iso.altitude_ref, 0);
        assert_eq!(iso.altitude, Rational::<u32>(19099, 1000));
    }

    #[test]
    fn gps_iso6709_apple_altitude_without_crs_issue_66() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        // Apple's default QuickTime location form: signed altitude, no CRS
        // suffix. 2.8.0 had two defects here — the minute-fraction landed
        // unscaled in the seconds slot, and the altitude was dropped. See #66.
        let gps: GPSInfo = "+47.7199-117.4931+522.171/".parse().ok().unwrap();
        assert_eq!(gps.latitude_ref, 'N');
        assert_eq!(
            gps.latitude,
            LatLng(
                Rational::<u32>(47, 1),
                Rational::<u32>(43, 1),
                Rational::<u32>(1164, 100),
            )
        );
        assert_eq!(gps.longitude_ref, 'W');
        assert_eq!(
            gps.longitude,
            LatLng(
                Rational::<u32>(117, 1),
                Rational::<u32>(29, 1),
                Rational::<u32>(3516, 100),
            )
        );
        assert_eq!(gps.altitude_ref, 0);
        assert_eq!(gps.altitude, Rational::<u32>(522171, 1000));

        // Negative altitude, still no CRS.
        let gps: GPSInfo = "+47.7199-117.4931-12.5/".parse().ok().unwrap();
        assert_eq!(gps.altitude_ref, 1);
        assert_eq!(gps.altitude, Rational::<u32>(12500, 1000));

        // No altitude field at all -> the trailing longitude must not be
        // mistaken for an altitude.
        let gps: GPSInfo = "+47.7199-117.4931/".parse().ok().unwrap();
        assert_eq!(gps.altitude, URational::default());
    }
}

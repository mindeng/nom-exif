use std::str::FromStr;

use iso6709parse::ISO6709Coord;

use crate::values::{IRational, URational};

/// Parsed GPS information from the GPSInfo subIFD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GPSInfo {
    pub latitude_ref: LatRef,
    pub latitude: LatLng,
    pub longitude_ref: LonRef,
    pub longitude: LatLng,
    pub altitude: Altitude,
    pub speed: Option<Speed>,
}

impl Default for GPSInfo {
    fn default() -> Self {
        Self {
            latitude_ref: LatRef::North,
            latitude: LatLng::default(),
            longitude_ref: LonRef::East,
            longitude: LatLng::default(),
            altitude: Altitude::Unknown,
            speed: None,
        }
    }
}

/// Latitude or longitude expressed as degrees / minutes / seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LatLng {
    pub degrees: URational,
    pub minutes: URational,
    pub seconds: URational,
}

/// Latitude hemisphere reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatRef {
    North,
    South,
}

impl LatRef {
    /// Construct from the 'N' / 'S' character carried in EXIF GPSLatitudeRef.
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'N' | 'n' => Some(Self::North),
            'S' | 's' => Some(Self::South),
            _ => None,
        }
    }

    pub fn as_char(self) -> char {
        match self {
            Self::North => 'N',
            Self::South => 'S',
        }
    }

    /// +1.0 or -1.0 — useful when assembling decimal-degrees latitude.
    pub fn sign(self) -> f64 {
        match self {
            Self::North => 1.0,
            Self::South => -1.0,
        }
    }
}

/// Longitude hemisphere reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LonRef {
    East,
    West,
}

impl LonRef {
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'E' | 'e' => Some(Self::East),
            'W' | 'w' => Some(Self::West),
            _ => None,
        }
    }

    pub fn as_char(self) -> char {
        match self {
            Self::East => 'E',
            Self::West => 'W',
        }
    }

    pub fn sign(self) -> f64 {
        match self {
            Self::East => 1.0,
            Self::West => -1.0,
        }
    }
}

/// Altitude relative to sea level.
///
/// Combines EXIF's `GPSAltitudeRef` (0 = above, 1 = below) with the magnitude
/// from `GPSAltitude` so the two cannot drift out of sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Altitude {
    /// Absent or unparseable.
    #[default]
    Unknown,
    AboveSeaLevel(URational),
    BelowSeaLevel(URational),
}

impl Altitude {
    /// Signed altitude in meters; `None` when Unknown or denominator=0.
    pub fn meters(&self) -> Option<f64> {
        match self {
            Altitude::Unknown => None,
            Altitude::AboveSeaLevel(r) => r.to_f64(),
            Altitude::BelowSeaLevel(r) => r.to_f64().map(|m| -m),
        }
    }

    /// The underlying magnitude rational, regardless of sign. None for `Unknown`.
    pub fn magnitude(&self) -> Option<URational> {
        match self {
            Altitude::Unknown => None,
            Altitude::AboveSeaLevel(r) | Altitude::BelowSeaLevel(r) => Some(*r),
        }
    }
}

/// EXIF GPS speed reference unit (`GPSSpeedRef`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedUnit {
    KmPerHour,
    MilesPerHour,
    Knots,
}

impl SpeedUnit {
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'K' | 'k' => Some(Self::KmPerHour),
            'M' | 'm' => Some(Self::MilesPerHour),
            'N' | 'n' => Some(Self::Knots),
            _ => None,
        }
    }

    pub fn as_char(self) -> char {
        match self {
            Self::KmPerHour => 'K',
            Self::MilesPerHour => 'M',
            Self::Knots => 'N',
        }
    }
}

/// EXIF GPS speed: unit + value paired so they cannot drift out of sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Speed {
    pub unit: SpeedUnit,
    pub value: URational,
}

impl LatLng {
    pub const fn new(degrees: URational, minutes: URational, seconds: URational) -> Self {
        Self { degrees, minutes, seconds }
    }

    /// Convert to decimal degrees. Returns `None` if any component has a zero
    /// denominator.
    pub fn to_decimal_degrees(&self) -> Option<f64> {
        let d = self.degrees.to_f64()?;
        let m = self.minutes.to_f64()?;
        let s = self.seconds.to_f64()?;
        Some(d + m / 60.0 + s / 3600.0)
    }

    /// Construct from decimal degrees. Rejects NaN / ±inf and values whose
    /// magnitude exceeds 180° with `ConvertError::InvalidDecimalDegrees`.
    pub fn try_from_decimal_degrees(degrees: f64) -> Result<Self, crate::ConvertError> {
        if !degrees.is_finite() || degrees.abs() > 180.0 {
            return Err(crate::ConvertError::InvalidDecimalDegrees(degrees));
        }
        let abs = degrees.abs();
        let d = abs.trunc() as u32;
        let mins_total = (abs - d as f64) * 60.0;
        let m = mins_total.trunc() as u32;
        let secs_hundredths = ((mins_total - m as f64) * 60.0 * 100.0).round() as u32;
        Ok(Self::new(
            URational::new(d, 1),
            URational::new(m, 1),
            URational::new(secs_hundredths, 100),
        ))
    }
}

impl GPSInfo {
    /// Latitude in decimal degrees, signed by `latitude_ref` (positive = north).
    pub fn latitude_decimal(&self) -> Option<f64> {
        Some(self.latitude.to_decimal_degrees()? * self.latitude_ref.sign())
    }

    /// Longitude in decimal degrees, signed by `longitude_ref` (positive = east).
    pub fn longitude_decimal(&self) -> Option<f64> {
        Some(self.longitude.to_decimal_degrees()? * self.longitude_ref.sign())
    }

    /// Signed altitude in meters; `None` if altitude is `Unknown` or denominator=0.
    pub fn altitude_meters(&self) -> Option<f64> {
        self.altitude.meters()
    }

    /// Returns an ISO 6709 geographic point location string such as
    /// `+48.8577+002.295/`.
    pub fn to_iso6709(&self) -> String {
        let latitude = self.latitude.to_decimal_degrees().unwrap_or(0.0);
        let longitude = self.longitude.to_decimal_degrees().unwrap_or(0.0);
        let altitude_meters = self.altitude.meters();
        format!(
            "{}{latitude:08.5}{}{longitude:09.5}{}/",
            match self.latitude_ref {
                LatRef::North => '+',
                LatRef::South => '-',
            },
            match self.longitude_ref {
                LonRef::East => '+',
                LonRef::West => '-',
            },
            match altitude_meters {
                None | Some(0.0) => String::new(),
                Some(m) => format!(
                    "{}{}CRSWGS_84",
                    if m >= 0.0 { "+" } else { "-" },
                    Self::format_float(m.abs())
                ),
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
}

impl TryFrom<&[URational]> for LatLng {
    type Error = crate::Error;
    fn try_from(value: &[URational]) -> Result<Self, Self::Error> {
        if value.len() < 3 {
            return Err(crate::Error::Malformed {
                kind: crate::error::MalformedKind::IfdEntry,
                message: "need at least 3 URational components for LatLng".into(),
            });
        }
        Ok(Self { degrees: value[0], minutes: value[1], seconds: value[2] })
    }
}

impl TryFrom<&[IRational]> for LatLng {
    type Error = crate::Error;
    fn try_from(value: &[IRational]) -> Result<Self, Self::Error> {
        if value.len() < 3 {
            return Err(crate::Error::Malformed {
                kind: crate::error::MalformedKind::IfdEntry,
                message: "need at least 3 IRational components for LatLng".into(),
            });
        }
        let map_negative = |_| crate::Error::Malformed {
            kind: crate::error::MalformedKind::IfdEntry,
            message: "negative LatLng component".into(),
        };
        Ok(Self {
            degrees: URational::try_from(value[0]).map_err(map_negative)?,
            minutes: URational::try_from(value[1]).map_err(map_negative)?,
            seconds: URational::try_from(value[2]).map_err(map_negative)?,
        })
    }
}

impl TryFrom<&Vec<URational>> for LatLng {
    type Error = crate::Error;
    fn try_from(value: &Vec<URational>) -> Result<Self, Self::Error> {
        Self::try_from(value.as_slice())
    }
}

impl TryFrom<&Vec<IRational>> for LatLng {
    type Error = crate::Error;
    fn try_from(value: &Vec<IRational>) -> Result<Self, Self::Error> {
        Self::try_from(value.as_slice())
    }
}

impl FromStr for GPSInfo {
    type Err = crate::ConvertError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        iso6709parse::parse::<ISO6709Coord>(s)
            .map(GPSInfo::from_iso6709_coord)
            .map_err(|_| crate::ConvertError::InvalidIso6709(s.to_string()))
    }
}

impl GPSInfo {
    /// Build a `GPSInfo` from a parsed ISO 6709 coordinate. Crate-internal:
    /// the public path is [`GPSInfo::from_str`] / `<GPSInfo as FromStr>::from_str`,
    /// which keeps `iso6709parse::ISO6709Coord` out of the public API surface
    /// (so an `iso6709parse` major-version bump does not force one here).
    pub(crate) fn from_iso6709_coord(v: ISO6709Coord) -> Self {
        let latitude_ref = if v.lat >= 0.0 { LatRef::North } else { LatRef::South };
        let longitude_ref = if v.lon >= 0.0 { LonRef::East } else { LonRef::West };
        let latitude = LatLng::try_from_decimal_degrees(v.lat.abs()).unwrap_or_default();
        let longitude = LatLng::try_from_decimal_degrees(v.lon.abs()).unwrap_or_default();
        let altitude = match v.altitude {
            None => Altitude::Unknown,
            Some(x) => {
                let mag = URational::new((x.abs() * 1000.0).trunc() as u32, 1000);
                if x >= 0.0 {
                    Altitude::AboveSeaLevel(mag)
                } else {
                    Altitude::BelowSeaLevel(mag)
                }
            }
        };
        Self {
            latitude_ref,
            latitude,
            longitude_ref,
            longitude,
            altitude,
            speed: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gps_iso6709() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let palace = GPSInfo {
            latitude_ref: LatRef::North,
            latitude: LatLng::new(
                URational::new(39, 1),
                URational::new(55, 1),
                URational::new(0, 1),
            ),
            longitude_ref: LonRef::East,
            longitude: LatLng::new(
                URational::new(116, 1),
                URational::new(23, 1),
                URational::new(27, 1),
            ),
            altitude: Altitude::AboveSeaLevel(URational::new(0, 1)),
            speed: None,
        };
        assert_eq!(palace.to_iso6709(), "+39.91667+116.39083/");

        let liberty = GPSInfo {
            latitude_ref: LatRef::North,
            latitude: LatLng::new(
                URational::new(40, 1),
                URational::new(41, 1),
                URational::new(21, 1),
            ),
            longitude_ref: LonRef::West,
            longitude: LatLng::new(
                URational::new(74, 1),
                URational::new(2, 1),
                URational::new(40, 1),
            ),
            altitude: Altitude::AboveSeaLevel(URational::new(0, 1)),
            speed: None,
        };
        assert_eq!(liberty.to_iso6709(), "+40.68917-074.04444/");

        let above = GPSInfo {
            latitude_ref: LatRef::North,
            latitude: LatLng::new(
                URational::new(40, 1),
                URational::new(41, 1),
                URational::new(21, 1),
            ),
            longitude_ref: LonRef::West,
            longitude: LatLng::new(
                URational::new(74, 1),
                URational::new(2, 1),
                URational::new(40, 1),
            ),
            altitude: Altitude::AboveSeaLevel(URational::new(123, 1)),
            speed: None,
        };
        assert_eq!(above.to_iso6709(), "+40.68917-074.04444+123CRSWGS_84/");

        let below = GPSInfo {
            latitude_ref: LatRef::North,
            latitude: LatLng::new(
                URational::new(40, 1),
                URational::new(41, 1),
                URational::new(21, 1),
            ),
            longitude_ref: LonRef::West,
            longitude: LatLng::new(
                URational::new(74, 1),
                URational::new(2, 1),
                URational::new(40, 1),
            ),
            altitude: Altitude::BelowSeaLevel(URational::new(123, 1)),
            speed: None,
        };
        assert_eq!(below.to_iso6709(), "+40.68917-074.04444-123CRSWGS_84/");

        let below = GPSInfo {
            latitude_ref: LatRef::North,
            latitude: LatLng::new(
                URational::new(40, 1),
                URational::new(41, 1),
                URational::new(21, 1),
            ),
            longitude_ref: LonRef::West,
            longitude: LatLng::new(
                URational::new(74, 1),
                URational::new(2, 1),
                URational::new(40, 1),
            ),
            altitude: Altitude::BelowSeaLevel(URational::new(100, 3)),
            speed: None,
        };
        assert_eq!(
            below.to_iso6709(),
            "+40.68917-074.04444-33.333CRSWGS_84/"
        );
    }

    #[test]
    fn gps_iso6709_with_invalid_alt() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let iso: ISO6709Coord = iso6709parse::parse("+26.5322-078.1969+019.099/").unwrap();
        assert_eq!(iso.lat, 26.5322);
        assert_eq!(iso.lon, -78.1969);
        assert_eq!(iso.altitude, None);

        let iso: GPSInfo = "+26.5322-078.1969+019.099/".parse().unwrap();
        assert_eq!(iso.latitude_ref, LatRef::North);
        assert_eq!(
            iso.latitude,
            LatLng::new(
                URational::new(26, 1),
                URational::new(31, 1),
                URational::new(5592, 100),
            )
        );

        assert_eq!(iso.longitude_ref, LonRef::West);
        assert_eq!(
            iso.longitude,
            LatLng::new(
                URational::new(78, 1),
                URational::new(11, 1),
                URational::new(4884, 100),
            )
        );

        assert_eq!(iso.altitude, Altitude::Unknown);
    }

    #[test]
    fn latlng_to_decimal_degrees() {
        let p = LatLng::new(
            URational::new(40, 1),
            URational::new(41, 1),
            URational::new(21, 1),
        );
        let d = p.to_decimal_degrees().unwrap();
        assert!((d - 40.689_167).abs() < 1e-5);
    }

    #[test]
    fn latlng_to_decimal_degrees_zero_denominator() {
        let p = LatLng::new(
            URational::new(40, 0),
            URational::new(41, 1),
            URational::new(21, 1),
        );
        assert_eq!(p.to_decimal_degrees(), None);
    }

    #[test]
    fn latlng_try_from_decimal_degrees_ok() {
        let p = LatLng::try_from_decimal_degrees(43.5).unwrap();
        let back = p.to_decimal_degrees().unwrap();
        assert!((back - 43.5).abs() < 1e-3);
    }

    #[test]
    fn latlng_try_from_decimal_degrees_rejects_nan_inf_oob() {
        use crate::ConvertError;
        assert!(matches!(
            LatLng::try_from_decimal_degrees(f64::NAN),
            Err(ConvertError::InvalidDecimalDegrees(_))
        ));
        assert!(matches!(
            LatLng::try_from_decimal_degrees(f64::INFINITY),
            Err(ConvertError::InvalidDecimalDegrees(_))
        ));
        assert!(matches!(
            LatLng::try_from_decimal_degrees(181.0),
            Err(ConvertError::InvalidDecimalDegrees(_))
        ));
    }

    #[test]
    fn lat_lon_ref_round_trip() {
        for c in ['N', 'S', 'n', 's'] {
            assert!(LatRef::from_char(c).is_some());
        }
        for c in ['E', 'W', 'e', 'w'] {
            assert!(LonRef::from_char(c).is_some());
        }
        assert_eq!(LatRef::North.as_char(), 'N');
        assert_eq!(LonRef::West.as_char(), 'W');
        assert_eq!(LatRef::South.sign(), -1.0);
        assert_eq!(LonRef::East.sign(), 1.0);
        assert_eq!(LatRef::from_char('X'), None);
    }

    #[test]
    fn altitude_meters_signed() {
        let above = Altitude::AboveSeaLevel(URational::new(123, 1));
        let below = Altitude::BelowSeaLevel(URational::new(123, 1));
        assert_eq!(above.meters(), Some(123.0));
        assert_eq!(below.meters(), Some(-123.0));
        assert_eq!(Altitude::Unknown.meters(), None);
        assert_eq!(Altitude::AboveSeaLevel(URational::new(1, 0)).meters(), None);
    }

    #[test]
    fn speed_unit_round_trip() {
        assert_eq!(SpeedUnit::from_char('K'), Some(SpeedUnit::KmPerHour));
        assert_eq!(SpeedUnit::from_char('M'), Some(SpeedUnit::MilesPerHour));
        assert_eq!(SpeedUnit::from_char('N'), Some(SpeedUnit::Knots));
        assert_eq!(SpeedUnit::from_char('X'), None);
        assert_eq!(SpeedUnit::Knots.as_char(), 'N');
    }

    #[test]
    fn gps_info_decimal_accessors() {
        let liberty = GPSInfo {
            latitude_ref: LatRef::North,
            latitude: LatLng::new(URational::new(40, 1), URational::new(41, 1), URational::new(21, 1)),
            longitude_ref: LonRef::West,
            longitude: LatLng::new(URational::new(74, 1), URational::new(2, 1), URational::new(40, 1)),
            altitude: Altitude::AboveSeaLevel(URational::new(123, 1)),
            speed: None,
        };
        let lat = liberty.latitude_decimal().unwrap();
        let lon = liberty.longitude_decimal().unwrap();
        assert!((lat - 40.689_167).abs() < 1e-5);
        assert!((lon - (-74.044_444)).abs() < 1e-5);
        assert_eq!(liberty.altitude_meters(), Some(123.0));
    }

    #[test]
    fn gps_info_from_str_uses_convert_error() {
        use crate::ConvertError;
        let err = "garbage".parse::<GPSInfo>().unwrap_err();
        assert!(matches!(err, ConvertError::InvalidIso6709(_)));
    }
}

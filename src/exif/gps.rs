use super::value::URational;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct GPSInfo {
    // N, S
    pub latitude_ref: char,
    // degree, minute, second,
    pub latitude: LatLng,

    // E, W
    pub longitude_ref: char,
    // degree, minute, second,
    pub longitude: LatLng,

    pub altitude_ref: u8,
    pub altitude: URational,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct LatLng(pub URational, pub URational, pub URational);

impl Default for LatLng {
    fn default() -> Self {
        LatLng(
            URational::default(),
            URational::default(),
            URational::default(),
        )
    }
}

impl GPSInfo {
    /// Returns an ISO 6709 geographic point location string such as
    /// `+48.8577+002.295/`.
    ///
    /// ⚠️ Altitude information is ignored currently.
    pub fn to_iso6709(&self) -> String {
        let latitude = self.latitude.0.to_float()
            + self.latitude.1.to_float() / 60.0
            + self.latitude.2.to_float() / 3600.0;
        let longitude = self.longitude.0.to_float()
            + self.longitude.1.to_float() / 60.0
            + self.longitude.2.to_float() / 3600.0;
        format!(
            "{}{latitude:08.5}{}{longitude:09.5}/",
            if self.latitude_ref == 'N' { '+' } else { '-' },
            if self.longitude_ref == 'E' { '+' } else { '-' },
        )
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
    use super::*;

    #[test]
    fn gps_iso6709() {
        let palace = GPSInfo {
            latitude_ref: 'N',
            latitude: LatLng(URational(39, 1), URational(55, 1), URational(0, 1)),
            longitude_ref: 'E',
            longitude: LatLng(URational(116, 1), URational(23, 1), URational(27, 1)),
            altitude_ref: 0,
            altitude: URational(0, 1),
        };
        assert_eq!(palace.to_iso6709(), "+39.91667+116.39083/");

        let liberty = GPSInfo {
            latitude_ref: 'N',
            latitude: LatLng(URational(40, 1), URational(41, 1), URational(21, 1)),
            longitude_ref: 'W',
            longitude: LatLng(URational(74, 1), URational(2, 1), URational(40, 1)),
            altitude_ref: 0,
            altitude: URational(0, 1),
        };
        assert_eq!(liberty.to_iso6709(), "+40.68917-074.04444/");
    }
}

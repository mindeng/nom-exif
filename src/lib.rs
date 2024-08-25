mod bbox;
mod error;
mod exif;
mod file;
mod heif;
mod input;
mod jpeg;
mod mov;
mod slice;
mod values;

pub use heif::parse_heif_exif;
pub use jpeg::parse_jpeg_exif;
pub use mov::{parse_metadata, parse_mov_metadata};

pub use exif::{Exif, ExifTag};
pub use values::EntryValue;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod testkit;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use std::collections::HashMap;
    use std::sync::RwLock;
    use std::thread;

    #[test]
    fn parse_exif_in_thread() {
        let exifs: HashMap<String, Exif> = HashMap::new();
        let exifs = RwLock::new(exifs);

        thread::scope(|s| {
            s.spawn(|| {
                let path = "exif.jpg";
                let f = open_sample(path).unwrap();
                let exif = parse_jpeg_exif(f).unwrap().unwrap();

                let mut exifs = exifs.write().unwrap();
                exifs.insert(path.to_string(), exif);
            });

            s.spawn(|| {
                let path = "exif.heic";
                let f = open_sample(path).unwrap();
                let exif = parse_heif_exif(f).unwrap().unwrap();

                let mut exifs = exifs.write().unwrap();
                exifs.insert(path.to_string(), exif);
            });
        });

        let exifs = exifs.read().unwrap();
        assert_eq!(
            exifs["exif.jpg"]
                .get_value(&ExifTag::Model)
                .unwrap()
                .unwrap(),
            "vivo X90 Pro+".into()
        );
        assert_eq!(
            exifs["exif.heic"]
                .get_value(&ExifTag::Model)
                .unwrap()
                .unwrap(),
            "iPhone 12 Pro".into()
        );
    }
}

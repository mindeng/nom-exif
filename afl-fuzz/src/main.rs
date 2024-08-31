use std::io::Cursor;

use nom_exif::{
    parse_exif, parse_heif_exif, parse_jpeg_exif, parse_metadata, parse_mov_metadata, Exif,
};

fn main() {
    afl::fuzz!(|data: &[u8]| {
        let reader = Cursor::new(data);
        let iter = parse_exif(reader.clone(), None);
        if let Ok(iter) = iter {
            if let Some(iter) = iter {
                let _ = iter.parse_gps_info();
                let _: Exif = iter.into();
            }
        }
        let _ = parse_metadata(reader.clone());
        let _ = parse_heif_exif(reader.clone());
        let _ = parse_jpeg_exif(reader.clone());
        let _ = parse_mov_metadata(reader.clone());
    });
}

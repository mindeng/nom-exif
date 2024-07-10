use std::io::Cursor;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        let reader = Cursor::new(data);
        let _ = nom_exif::parse_metadata(reader.clone());
        let _ = nom_exif::parse_heif_exif(reader.clone());
        let _ = nom_exif::parse_jpeg_exif(reader.clone());
        let _ = nom_exif::parse_mov_metadata(reader.clone());
    });
}

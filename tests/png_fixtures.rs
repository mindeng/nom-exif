//! PNG fixture builders for integration tests. Programmatically
//! generates minimal valid PNG byte sequences with specified chunk
//! contents.
//!
//! CRCs are written as zero — nom-exif's PNG parser does not validate
//! CRCs (consistent with how JPEG markers / HEIC boxes are handled).

#![allow(dead_code)]

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Build a single PNG chunk: length:4 + type:4 + data + crc:4 (zeros).
pub fn build_chunk(ctype: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ctype);
    out.extend_from_slice(data);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

/// Minimal 1×1 grayscale IHDR.
pub fn ihdr_minimal() -> Vec<u8> {
    build_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0])
}

/// Empty IEND.
pub fn iend() -> Vec<u8> {
    build_chunk(b"IEND", &[])
}

/// Tiny IDAT — gives the PNG some image data so it's not "header only".
pub fn idat_tiny() -> Vec<u8> {
    build_chunk(
        b"IDAT",
        &[0x78, 0x9c, 0x62, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x01],
    )
}

/// Build an `eXIf` chunk wrapping the given TIFF bytes (no extra header).
pub fn exif_chunk(tiff: &[u8]) -> Vec<u8> {
    build_chunk(b"eXIf", tiff)
}

/// Build a `tEXt` chunk for the given (key, value).
pub fn text_chunk(key: &str, value: &str) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(key.as_bytes());
    data.push(0);
    data.extend_from_slice(value.as_bytes());
    build_chunk(b"tEXt", &data)
}

/// Compose a complete PNG buffer: signature + IHDR + chunks + IDAT + IEND.
/// The order is convention-following: ancillary chunks before IDAT.
pub fn build_png(ancillary: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(PNG_SIGNATURE);
    out.extend_from_slice(&ihdr_minimal());
    for c in ancillary {
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&idat_tiny());
    out.extend_from_slice(&iend());
    out
}

/// Extract the TIFF bytes from a JPEG APP1 segment in `testdata/exif.jpg`.
/// We piggy-back on the existing test fixture to get a real-world EXIF
/// blob without hand-crafting one.
pub fn tiff_from_jpeg_fixture() -> Vec<u8> {
    let raw = std::fs::read("testdata/exif.jpg").expect("testdata/exif.jpg missing");
    // Walk JPEG to find APP1 ("Exif\0\0" + TIFF).
    let mut i = 2; // skip SOI
    while i + 4 < raw.len() {
        if raw[i] != 0xFF {
            break;
        }
        let marker = raw[i + 1];
        let seg_len = u16::from_be_bytes([raw[i + 2], raw[i + 3]]) as usize;
        if marker == 0xE1 {
            // APP1: payload starts at i+4. Check "Exif\0\0" prefix.
            let payload = &raw[i + 4..i + 2 + seg_len];
            if payload.starts_with(b"Exif\x00\x00") {
                return payload[6..].to_vec();
            }
        }
        i += 2 + seg_len;
    }
    panic!("could not locate APP1/Exif segment in testdata/exif.jpg");
}

#[cfg(test)]
mod gen {
    use super::*;

    /// Run via `cargo test --test png_fixtures gen::write_fixtures` to
    /// (re)generate testdata/*.png from this builder. Idempotent —
    /// existing files are overwritten.
    #[test]
    #[ignore = "fixture generation is opt-in; run with --ignored"]
    fn write_fixtures() {
        // exif.png: eXIf chunk + Title + Software tEXt
        let tiff = tiff_from_jpeg_fixture();
        let png = build_png(&[
            text_chunk("Title", "PNG with EXIF"),
            text_chunk("Software", "nom-exif fixture builder"),
            exif_chunk(&tiff),
        ]);
        std::fs::write("testdata/exif.png", &png).unwrap();

        // text-only.png: tEXt only, no EXIF
        let png = build_png(&[
            text_chunk("Title", "Just text"),
            text_chunk("Author", "test"),
        ]);
        std::fs::write("testdata/text-only.png", &png).unwrap();
    }
}

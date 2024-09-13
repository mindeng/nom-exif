use std::io::{Read, Seek};

use nom::combinator::fail;
use nom::{number::complete::be_u32, IResult};

use crate::bbox::find_box;
use crate::exif::{input_to_exif, read_exif, Exif};
use crate::file::FileFormat;
use crate::{
    bbox::{BoxHolder, MetaBox, ParseBox},
    exif::check_exif_header,
};

/// Analyze the byte stream in the `reader` as a HEIF/HEIC file, attempting to
/// extract Exif data it may contain.
///
/// Please note that the parsing routine itself provides a buffer, so the
/// `reader` may not need to be wrapped with `BufRead`.
///
/// # Usage
///
/// ```rust
/// use nom_exif::*;
/// use nom_exif::ExifTag::*;
///
/// use std::fs::File;
/// use std::path::Path;
///
/// let f = File::open(Path::new("./testdata/exif.heic")).unwrap();
/// let exif = parse_heif_exif(f).unwrap().unwrap();
///
/// assert_eq!(exif.get(Make).unwrap().to_string(), "Apple");
/// ```
///
/// See also: [`parse_exif`](crate::parse_exif).
pub fn parse_heif_exif<R: Read + Seek>(reader: R) -> crate::Result<Option<Exif>> {
    read_exif(reader, Some(FileFormat::Heif))?
        .map(input_to_exif)
        .transpose()
}

/// Extract Exif TIFF data from the bytes of a HEIF/HEIC file.
pub(crate) fn extract_exif_data(input: &[u8]) -> IResult<&[u8], Option<&[u8]>> {
    let remain = input;
    let (remain, bbox) = BoxHolder::parse(remain)?;
    if bbox.box_type() != "ftyp" {
        return fail(input);
    }

    let (_, Some(bbox)) = find_box(remain, "meta")? else {
        return Ok((remain, None));
    };
    let (_, bbox) = MetaBox::parse_box(bbox.data)?;
    let (out_remain, data) = bbox.exif_data(input)?;

    if let Some(data) = data {
        let (remain, _) = be_u32(data)?;
        if check_exif_header(remain) {
            Ok((out_remain, Some(&remain[6..]))) // Safe-slice
        } else {
            Ok((out_remain, None))
        }
    } else {
        Ok((out_remain, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        file::{check_heif, FileFormat},
        testkit::*,
    };
    use test_case::test_case;

    #[test_case("exif.heic")]
    fn heif(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let reader = open_sample(path).unwrap();
        let exif = parse_heif_exif(reader).unwrap().unwrap();
        let mut expect = String::new();
        open_sample(&format!("{path}.sorted.txt"))
            .unwrap()
            .read_to_string(&mut expect)
            .unwrap();

        assert_eq!(sorted_exif_entries(&exif).join("\n"), expect.trim());
    }

    #[test_case("ramdisk.img")]
    fn invalid_heic(path: &str) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let reader = open_sample(path).unwrap();
        parse_heif_exif(reader).expect_err("should be ParseFailed error");
    }

    #[test_case("compatible-brands.heic", Some(FileFormat::Heif))]
    #[test_case("compatible-brands-fail.heic", None)]
    fn heic_compatible_brands(path: &str, ft: Option<FileFormat>) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let got = check_heif(&buf);
        if ft.is_some() {
            got.unwrap();
        } else {
            got.unwrap_err();
        }
    }

    #[test_case("exif-one-entry.heic", 0x24-10)]
    #[test_case("exif.heic", 0xa3a-10)]
    fn heic_exif_data(path: &str, exif_size: usize) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let (_, exif) = extract_exif_data(&buf[..]).unwrap();

        if exif_size == 0 {
            assert!(exif.is_none());
        } else {
            assert_eq!(exif.unwrap().len(), exif_size);
        }
    }
}

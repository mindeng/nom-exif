use nom::{
    number::{streaming, Endianness},
    IResult, Needed, Parser,
};

use crate::{
    error::{MalformedKind, ParsingError},
    exif::TiffHeader,
    values::{array_to_string, DataFormat},
    TagOrCode,
};

use super::{exif_exif::IFD_ENTRY_SIZE, exif_iter::SUBIFD_TAGS};

/// Only iterates headers, don't parse entries.
///
/// Currently only used to extract Exif data for *.tiff files.
///
/// NOTE: `parse_tag_entry_header` short-circuits on `tag == 0` as a guard
/// against zero-padded malformed IFDs. This is safe **today** because we
/// only observe `sub_ifd_offset`, and tag 0 is never in `SUBIFD_TAGS`. If
/// this struct is ever extended to emit entry values, gate that
/// short-circuit on "not inside the GPS sub-IFD" — tag 0 is the legitimate
/// GPSVersionID and dropping it loses every following GPS field. See
/// `IfdIter::is_gps_subifd` in `exif_iter.rs` and issue #50.
pub(crate) struct IfdHeaderTravel<'a> {
    // starts from file beginning
    data: &'a [u8],

    tag: TagOrCode,

    endian: Endianness,

    // ifd data offset
    offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct EntryInfo<'a> {
    pub tag: u16,
    #[allow(unused)]
    pub data: &'a [u8],
    #[allow(unused)]
    pub data_format: DataFormat,
    #[allow(unused)]
    pub data_offset: Option<u32>,
    pub sub_ifd_offset: Option<u32>,
}

impl<'a> IfdHeaderTravel<'a> {
    pub fn new(input: &'a [u8], offset: usize, tag: TagOrCode, endian: Endianness) -> Self {
        Self {
            data: input,
            tag,
            endian,
            offset,
        }
    }

    #[tracing::instrument(skip_all)]
    fn parse_tag_entry_header(
        &'a self,
        entry_data: &'a [u8],
    ) -> IResult<&'a [u8], Option<EntryInfo<'a>>> {
        let endian = self.endian;
        let (remain, (tag, data_format, components_num, value_or_offset)) = (
            streaming::u16::<_, nom::error::Error<_>>(endian),
            streaming::u16(endian),
            streaming::u32(endian),
            streaming::u32(endian),
        )
            .parse(entry_data)?;

        if tag == 0 {
            return Ok((remain, None));
        }

        let data_format: DataFormat = match data_format.try_into() {
            Ok(df) => df,
            // Ignore errors here
            Err(e) => {
                tracing::warn!(?e, "Ignored: IFD entry data format error");
                return Ok((&[][..], None));
            }
        };

        // get component_size according to data format
        let component_size = data_format.component_size();

        // get entry data
        let size = components_num as usize * component_size;
        let (data, data_offset) = if size > 4 {
            let start = self.get_data_pos(value_or_offset) as usize;
            let end = start + size;
            tracing::debug!(
                components_num,
                size,
                "tag {:04x} entry data start {:08x} end {:08x} my_offset: {:08x} data len {:08x}",
                tag,
                value_or_offset,
                start,
                end,
                self.data.len(),
            );
            if end > self.data.len() {
                return Err(nom::Err::Incomplete(Needed::new(end - self.data.len())));
            }
            (&self.data[start..end], Some(start as u32))
        } else {
            (entry_data, None)
        };

        let sub_ifd_offset = if SUBIFD_TAGS.contains(&tag) {
            let offset = self.get_data_pos(value_or_offset);
            if offset > 0 {
                Some(offset)
            } else {
                None
            }
        } else {
            None
        };

        let entry = EntryInfo {
            tag,
            data,
            data_format,
            data_offset,
            sub_ifd_offset,
        };
        Ok((&[][..], Some(entry)))
    }

    fn get_data_pos(&'a self, value_or_offset: u32) -> u32 {
        // value_or_offset.saturating_sub(self.offset)
        value_or_offset
    }

    #[tracing::instrument(skip(self))]
    fn parse_ifd_entry_header(&self, pos: u32) -> IResult<&[u8], Option<IfdHeaderTravel<'a>>> {
        let (_, entry_data) =
            nom::bytes::streaming::take(IFD_ENTRY_SIZE)(&self.data[pos as usize..])?;

        let (remain, entry) = self.parse_tag_entry_header(entry_data)?;

        if let Some(entry) = entry {
            // if !cb(&entry) {
            //     return Ok((&[][..], ()));
            // }

            if let Some(offset) = entry.sub_ifd_offset {
                let tag: TagOrCode = entry.tag.into();
                tracing::debug!(?offset, data_len = self.data.len(), "sub-ifd: {:?}", tag);

                // Full fill bytes until sub-ifd header
                let (_, _) =
                    nom::bytes::streaming::take(offset as usize - remain.len() + 2)(self.data)?;

                let sub_ifd = IfdHeaderTravel::new(self.data, offset as usize, tag, self.endian);
                return Ok((remain, Some(sub_ifd)));
            }
        }

        Ok((remain, None))
    }

    #[tracing::instrument(skip(self))]
    pub fn travel_ifd(&mut self, depth: usize) -> Result<(), ParsingError> {
        if depth >= 3 {
            let msg = "depth shouldn't be greater than 3";
            tracing::error!(msg);
            return Err(ParsingError::Failed {
                kind: MalformedKind::IfdEntry,
                message: msg.into(),
            });
        }

        if self.offset + 2 > self.data.len() {
            return Err(ParsingError::Failed {
                kind: MalformedKind::TiffHeader,
                message: format!("invalid ifd offset: {}", self.offset),
            });
        }

        let (_, entry_num) =
            TiffHeader::parse_ifd_entry_num(&self.data[self.offset..], self.endian).map_err(
                |e: nom::Err<nom::error::Error<&[u8]>>| ParsingError::Failed {
                    kind: MalformedKind::TiffHeader,
                    message: format!("parse ifd entry count failed: {e:?}"),
                },
            )?;
        let mut pos = self.offset + 2;

        let mut sub_ifds = Vec::new();

        // parse entries
        for _ in 0..entry_num {
            if pos >= self.data.len() {
                break;
            }
            let (_, sub_ifd) = self.parse_ifd_entry_header(pos as u32).map_err(
                |e: nom::Err<nom::error::Error<&[u8]>>| ParsingError::Failed {
                    kind: MalformedKind::IfdEntry,
                    message: format!("parse ifd entry header failed: {e:?}"),
                },
            )?;
            pos += IFD_ENTRY_SIZE;

            if let Some(ifd) = sub_ifd {
                tracing::debug!(
                    data = array_to_string("bytes", self.data),
                    tag = ifd.tag.to_string(),
                );
                sub_ifds.push(ifd);
            }
        }

        for mut ifd in sub_ifds {
            ifd.travel_ifd(depth + 1)?;
        }

        // Currently, we ignore ifd1 data in *.tif files
        Ok(())
    }
}

// fn keep_incomplete_err_only<T: Debug>(e: nom::Err<T>) -> nom::Err<String> {
//     match e {
//         nom::Err::Incomplete(n) => nom::Err::Incomplete(n),
//         nom::Err::Error(e) => nom::Err::Error(format!("parse ifd error: {:?}", e)),
//         nom::Err::Failure(_) => nom::Err::Failure("parse ifd failure".to_string()),
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::read_sample;
    use nom::number::Endianness;

    /// Build a single 12-byte little-endian IFD entry: tag(2) + format(2) + count(4) + value/offset(4).
    fn entry(tag: u16, format: u16, count: u32, value: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity(12);
        v.extend_from_slice(&tag.to_le_bytes());
        v.extend_from_slice(&format.to_le_bytes());
        v.extend_from_slice(&count.to_le_bytes());
        v.extend_from_slice(&value.to_le_bytes());
        v
    }

    /// Build a little-endian IFD: 2-byte entry_count + entries + 4-byte next-IFD offset (zero).
    fn ifd(entries: &[Vec<u8>]) -> Vec<u8> {
        let count = entries.len() as u16;
        let mut v = count.to_le_bytes().to_vec();
        for e in entries {
            v.extend_from_slice(e);
        }
        v.extend_from_slice(&[0u8; 4]);
        v
    }

    #[test]
    fn travel_short_circuits_on_tag_zero() {
        // tag = 0 must not be emitted as a sub-IFD (covers line 75).
        let data = ifd(&[entry(0, 1, 1, 0)]);
        let mut t = IfdHeaderTravel::new(&data, 0, 0u16.into(), Endianness::Little);
        assert!(t.travel_ifd(0).is_ok());
    }

    #[test]
    fn travel_rejects_invalid_data_format() {
        // data_format = 99 is out of range — covers the `Err(_)` arm (lines 81-83).
        let data = ifd(&[entry(0x010F /* Make */, 99, 1, 0)]);
        let mut t = IfdHeaderTravel::new(&data, 0, 0u16.into(), Endianness::Little);
        assert!(t.travel_ifd(0).is_ok());
    }

    #[test]
    fn travel_data_past_eof_errors() {
        // size > 4 with offset past EOF must error (covers lines 103-106 of the
        // size>4 branch in parse_tag_entry_header). We do not assert the error
        // kind — both Incomplete and Failed satisfy the contract.
        let data = ifd(&[entry(0x010F /* Make */, 2, 100, 0x0000_FF00)]);
        let mut t = IfdHeaderTravel::new(&data, 0, 0u16.into(), Endianness::Little);
        assert!(t.travel_ifd(0).is_err());
    }

    #[test]
    fn travel_invalid_offset_guard() {
        // offset + 2 > data.len() (covers line 176).
        let data = vec![0u8; 1];
        let mut t = IfdHeaderTravel::new(&data, 100, 0u16.into(), Endianness::Little);
        assert!(t.travel_ifd(0).is_err());
    }

    #[test]
    fn travel_depth_guard() {
        // depth >= 3 must error (covers lines 170-172).
        let data = ifd(&[]);
        let mut t = IfdHeaderTravel::new(&data, 0, 0u16.into(), Endianness::Little);
        assert!(t.travel_ifd(3).is_err());
    }

    #[test]
    fn travel_real_tiff_entry_loop() {
        // Real TIFF traversal — exercises parse_ifd_entry_num and the per-entry
        // 12-byte step loop on a full IFD0. testdata/tif.tif has no ExifIFD or
        // GPSInfo entries, so this test does NOT drive sub-IFD recursion; for
        // that, see travel_synthetic_subifd_recursion below.
        let buf = read_sample("tif.tif").unwrap();
        let endian = if &buf[0..2] == b"II" {
            Endianness::Little
        } else {
            Endianness::Big
        };
        let ifd_offset = match endian {
            Endianness::Little => u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            Endianness::Big => u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
            _ => unreachable!(),
        };
        let mut t = IfdHeaderTravel::new(&buf, ifd_offset as usize, 0u16.into(), endian);
        t.travel_ifd(0).unwrap();
    }

    #[test]
    fn travel_synthetic_subifd_recursion() {
        // Hand-built TIFF body: outer IFD at offset 0 with a single ExifOffset
        // (tag 0x8769) entry pointing to a child IFD with zero entries.
        // Exercises SUBIFD_TAGS branch (lines 113-118) and the recursion path
        // (lines 151-162, 205-206).
        const EXIF_OFFSET_TAG: u16 = 0x8769;
        // Outer IFD: 2-byte count + one 12-byte entry + 4-byte next-ifd = 18 bytes
        let sub_ifd_off: u32 = 18;
        let outer = ifd(&[entry(EXIF_OFFSET_TAG, 4 /* LONG */, 1, sub_ifd_off)]);
        // Sub-IFD: 0 entries + zero next-ifd pointer = 6 bytes
        let sub = ifd(&[]);
        let mut data = outer;
        data.extend_from_slice(&sub);
        let mut t = IfdHeaderTravel::new(&data, 0, 0u16.into(), Endianness::Little);
        t.travel_ifd(0).unwrap();
    }

    #[test]
    fn travel_subifd_zero_offset_is_skipped() {
        // ExifOffset tag with value 0 — covers the `else { None }` branch in the
        // `if offset > 0` check (line 118).
        const EXIF_OFFSET_TAG: u16 = 0x8769;
        let data = ifd(&[entry(EXIF_OFFSET_TAG, 4 /* LONG */, 1, 0)]);
        let mut t = IfdHeaderTravel::new(&data, 0, 0u16.into(), Endianness::Little);
        t.travel_ifd(0).unwrap();
    }
}

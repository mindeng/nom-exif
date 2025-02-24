use nom::{
    number::{streaming, Endianness},
    sequence::tuple,
    IResult, Needed,
};

use crate::{error::ParsingError, exif::TiffHeader, values::DataFormat, ExifTag};

use super::{exif_exif::IFD_ENTRY_SIZE, exif_iter::SUBIFD_TAGS};

/// Only iterates headers, don't parse entries.
///
/// Currently only used to extract Exif data for *.tiff files
pub(crate) struct IfdHeaderTravel<'a> {
    // starts from "ifd/sub-ifd entries" (two bytes of ifd/sub-ifd entry num)
    ifd_data: &'a [u8],

    // IFD data offset relative to the TIFF header.
    offset: u32,

    endian: Endianness,
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
    pub fn new(input: &'a [u8], offset: u32, endian: Endianness) -> Self {
        Self {
            ifd_data: input,
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
        let (remain, (tag, data_format, components_num, value_or_offset)) = tuple((
            streaming::u16::<_, nom::error::Error<_>>(endian),
            streaming::u16(endian),
            streaming::u32(endian),
            streaming::u32(endian),
        ))(entry_data)?;

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
                "tag {:04x} entry data @ offset {:08x} start {:08x} end {:08x} my_offset: {:08x} data len {:08x}",
                tag,
                value_or_offset,
                start,
                end,
                self.offset,
                self.ifd_data.len()
            );
            if end > self.ifd_data.len() {
                return Err(nom::Err::Incomplete(Needed::new(end - self.ifd_data.len())));
            }
            (&self.ifd_data[start..end], Some(start as u32))
        } else {
            (entry_data, None)
        };

        let sub_ifd_offset = if SUBIFD_TAGS.contains(&tag) {
            let offset = self.get_data_pos(value_or_offset);
            Some(offset)
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
        value_or_offset.saturating_sub(self.offset)
    }

    #[tracing::instrument(skip(self))]
    fn parse_ifd_entry_header(&self, pos: u32) -> IResult<&[u8], Option<IfdHeaderTravel<'a>>> {
        let (_, entry_data) =
            nom::bytes::streaming::take(IFD_ENTRY_SIZE)(&self.ifd_data[pos as usize..])?;

        let (remain, entry) = self.parse_tag_entry_header(entry_data)?;

        if let Some(entry) = entry {
            // if !cb(&entry) {
            //     return Ok((&[][..], ()));
            // }

            if let Some(offset) = entry.sub_ifd_offset {
                let tag: ExifTag = entry.tag.try_into().unwrap();
                tracing::debug!(
                    ?offset,
                    data_len = self.ifd_data.len(),
                    "sub-ifd: {:?}",
                    tag
                );

                // Full fill bytes until sub-ifd header
                let (_, _) =
                    nom::bytes::streaming::take(offset as usize - remain.len() + 2)(self.ifd_data)?;

                let sub_ifd =
                    IfdHeaderTravel::new(&self.ifd_data[offset as usize..], offset, self.endian);
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
            return Err(ParsingError::Failed(msg.into()));
        }

        tracing::debug!(ifd_data_len = self.ifd_data.len(), offset = self.offset);
        let (remain, entry_num) = TiffHeader::parse_ifd_entry_num(self.ifd_data, self.endian)?;
        let mut pos = self.ifd_data.len() - remain.len();

        let mut sub_ifds = Vec::new();

        // parse entries
        for _ in 0..entry_num {
            let (_, sub_ifd) = self.parse_ifd_entry_header(pos as u32)?;
            pos += IFD_ENTRY_SIZE;

            if let Some(ifd) = sub_ifd {
                if ifd.offset <= self.offset {
                    tracing::error!(
                        current_ifd_offset = self.offset,
                        subifd_offset = ifd.offset,
                        "bad new SUB-IFD in TIFF: offset is smaller than current IFD"
                    );
                } else {
                    sub_ifds.push(ifd);
                }
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

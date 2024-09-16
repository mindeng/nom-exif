use std::{fmt::Debug, sync::Arc};

use nom::{
    number::{complete, streaming, Endianness},
    sequence::tuple,
    IResult, Needed,
};
use thiserror::Error;

use crate::{
    error::ParsingError,
    partial_vec::{AssociatedInput, PartialVec},
    slice::SliceChecked,
    values::{DataFormat, EntryData, IRational, ParseEntryError, URational},
    EntryValue, ExifTag,
};

use super::{exif_exif::IFD_ENTRY_SIZE, tags::ExifTagCode, GPSInfo, TiffHeader};

/// An iterator version of [`Exif`](crate::Exif). Use [`ParsedExifEntry`] as
/// iterator items.
///
/// Clone an `ExifIter` is very cheap, the underlying data is shared
/// through Arc.
///
/// ⚠️ Currently `ExifIter::clone()` will reset the new cloned iterator's index.
/// Please try to avoid calling this method, because the current behavior of
/// this method is not very intuitive. If you wish to clone the iterator and
/// reset the iteration state, use [`ExifIter::clone_and_rewind`] explicitly.
///
/// For the sake of compatibility, the current behavior of this method is
/// temporarily retained, and may be modified in subsequent versions.
///
/// If you want to convert an `ExifIter` `into` an [`Exif`], you probably want
/// to call `clone_and_rewind` and use the new cloned one. Since the iterator
/// index may have been modified by `Iterator::next()` calls.
pub struct ExifIter {
    // Use Arc to make sure we won't clone the owned data.
    input: Arc<PartialVec>,
    tiff_header: TiffHeader,
    tz: Option<String>,
    ifd0: ImageFileDirectoryIter,

    // Iterating status
    ifds: Vec<ImageFileDirectoryIter>,
}

impl Debug for ExifIter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExifIter")
            .field("data len", &self.input.len())
            .field("tiff_header", &self.tiff_header)
            .field("ifd0", &self.ifd0)
            .field("state", &self.ifds.first().map(|x| (x.index, x.pos)))
            .field("ifds num", &self.ifds.len())
            .finish_non_exhaustive()
    }
}

impl Clone for ExifIter {
    /// ⚠️ Try to avoid calling this method. The semantics of this method are
    /// not clear at present. If you wish to clone the iterator and reset the
    /// iteration state, use [`ExifIter::clone_and_rewind`] explicitly.
    ///
    /// For the sake of compatibility, the current behavior of this method is
    /// temporarily retained, and may be modified in subsequent versions.
    ///
    /// Clone an `ExifIter` is very cheap, the underlying data is shared
    /// through Arc.
    ///
    /// If you want to convert an `ExifIter` `into` an [`Exif`], you'd better
    /// clone the `ExifIter` before converting. Since the iterator index may
    /// have been modified by `Iterator::next()` calls.
    ///
    /// `clone()` will reset the cloned iterator index to be the first one.
    fn clone(&self) -> Self {
        self.clone_and_rewind()
    }
}

impl ExifIter {
    pub(crate) fn new(
        input: impl Into<PartialVec>,
        tiff_header: TiffHeader,
        tz: Option<String>,
        ifd0: ImageFileDirectoryIter,
    ) -> ExifIter {
        let ifds = vec![ifd0.clone()];
        ExifIter {
            input: Arc::new(input.into()),
            tiff_header,
            tz,
            ifd0,
            ifds,
        }
    }

    /// Clone and rewind the iterator's index.
    ///
    /// Clone an `ExifIter` is very cheap, the underlying data is shared
    /// through Arc.
    pub fn clone_and_rewind(&self) -> Self {
        let ifd0 = self.ifd0.clone_and_rewind();
        let ifds = vec![ifd0.clone()];
        Self {
            input: self.input.clone(),
            tiff_header: self.tiff_header.clone(),
            tz: self.tz.clone(),
            ifd0,
            ifds,
        }
    }

    /// Try to find and parse gps information.
    ///
    /// Calling this method won't affect the iterator's state.
    ///
    /// Returns:
    ///
    /// - An `Ok<Some<GPSInfo>>` if gps info is found and parsed successfully.
    /// - An `Ok<None>` if gps info is not found.
    /// - An `Err` if gps info is found but parsing failed.
    #[tracing::instrument(skip_all)]
    pub fn parse_gps_info(&self) -> crate::Result<Option<GPSInfo>> {
        let mut iter = self.clone_and_rewind();
        let Some(gps) = iter.find(|x| {
            tracing::info!(?x, "find");
            x.tag.tag().is_some_and(|t| t == ExifTag::GPSInfo)
        }) else {
            tracing::warn!(ifd0 = ?iter.ifds.first(), "GPSInfo not found");
            return Ok(None);
        };

        let offset = match gps.get_result() {
            Ok(v) => {
                if let Some(offset) = v.as_u32() {
                    offset
                } else {
                    return Err(EntryError(ParseEntryError::InvalidData(
                        "invalid gps offset".into(),
                    ))
                    .into());
                }
            }
            Err(e) => return Err(e.clone().into()),
        };

        let mut gps_subifd = match ImageFileDirectoryIter::try_new(
            gps.ifd,
            iter.input.partial(&iter.input[offset as usize..]),
            offset,
            iter.tiff_header.endian,
            iter.tz.clone(),
        ) {
            Ok(ifd0) => ifd0,
            Err(e) => return Err(e),
        };
        Ok(gps_subifd.parse_gps_info())
    }

    #[tracing::instrument(skip_all)]
    pub fn consume_parse_gps_info(mut self) -> crate::Result<Option<GPSInfo>> {
        let Some(gps) = self.find(|x| {
            tracing::info!(?x, "find");
            x.tag.tag().is_some_and(|t| t == ExifTag::GPSInfo)
        }) else {
            tracing::warn!(ifd0 = ?self.ifds.first(), "GPSInfo not found");
            return Ok(None);
        };

        let offset = match gps.get_result() {
            Ok(v) => {
                if let Some(offset) = v.as_u32() {
                    offset
                } else {
                    return Err(EntryError(ParseEntryError::InvalidData(
                        "invalid gps offset".into(),
                    ))
                    .into());
                }
            }
            Err(e) => return Err(e.clone().into()),
        };

        let iter = self;
        let mut gps_subifd = match ImageFileDirectoryIter::try_new(
            gps.ifd,
            iter.input.partial(&iter.input[offset as usize..]),
            offset,
            iter.tiff_header.endian,
            iter.tz.clone(),
        ) {
            Ok(ifd0) => ifd0,
            Err(e) => return Err(e),
        };
        Ok(gps_subifd.parse_gps_info())
    }

    pub(crate) fn to_owned(&self) -> ExifIter {
        ExifIter::new(
            self.input.to_vec(),
            self.tiff_header.clone(),
            self.tz.clone(),
            self.ifd0.clone_and_rewind(),
        )
    }
}

#[derive(Debug, Clone, Error)]
#[error("ifd entry error: {0}")]
pub struct EntryError(ParseEntryError);

impl From<EntryError> for crate::Error {
    fn from(value: EntryError) -> Self {
        Self::ParseFailed(value.into())
    }
}

/// Represents a parsed IFD entry. Used as iterator items in [`ExifIter`].
#[derive(Clone)]
pub struct ParsedExifEntry {
    // 0: ifd0, 1: ifd1
    ifd: usize,
    tag: ExifTagCode,
    res: Option<Result<EntryValue, EntryError>>,
}

impl ParsedExifEntry {
    /// Get the IFD index value where this entry is located.
    /// - 0: ifd0 (main image)
    /// - 1: ifd1 (thumbnail)
    pub fn ifd_index(&self) -> usize {
        self.ifd
    }

    /// Get recognized Exif tag of this entry, maybe return `None` if the tag
    /// is unrecognized.
    ///
    /// If you have any custom defined tag which does not exist in [`ExifTag`],
    /// then you should use [`Self::tag_code`] to get the raw tag code.
    ///
    /// **Note**: You can always get the raw tag code via [`Self::tag_code`],
    /// no matter if it's recognized.
    pub fn tag(&self) -> Option<ExifTag> {
        match self.tag {
            ExifTagCode::Tag(t) => Some(t),
            ExifTagCode::Code(_) => None,
        }
    }

    /// Get the raw tag code of this entry.
    ///
    /// In case you have some custom defined tags which doesn't exist in
    /// [`ExifTag`], you can use this method to get the raw tag code of this
    /// entry.
    pub fn tag_code(&self) -> u16 {
        self.tag.code()
    }

    /// Returns true if there is an `EntryValue` in self.
    ///
    /// Both of the following situations may cause this method to return false:
    /// - An error occurred while parsing this entry
    /// - The value has been taken by calling [`Self::take_value`] or
    ///   [`Self::take_result`] methods.
    pub fn has_value(&self) -> bool {
        self.res.as_ref().map(|e| e.is_ok()).is_some_and(|b| b)
    }

    /// Get the parsed entry value of this entry.
    pub fn get_value(&self) -> Option<&EntryValue> {
        match self.res.as_ref() {
            Some(Ok(v)) => Some(v),
            Some(Err(_)) | None => None,
        }
    }

    /// Takes out the parsed entry value of this entry.
    ///
    /// If you need to convert this `ExifIter` to an [`crate::Exif`], please
    /// don't call this method! Otherwise the converted `Exif` is incomplete.
    ///
    /// **Note**: This method can only be called once! Once it has been called,
    /// calling it again always returns `None`. You may want to check it by
    /// calling [`Self::has_value`] before calling this method.
    pub fn take_value(&mut self) -> Option<EntryValue> {
        match self.res.take() {
            Some(v) => v.ok(),
            None => None,
        }
    }

    /// Get the parsed result of this entry.
    ///
    /// Returns:
    ///
    /// - If any error occurred while parsing this entry, an
    ///   Err(&[`EntryError`]) is returned.
    ///
    /// - Otherwise, an Ok(&[`EntryValue`]) is returned.
    pub fn get_result(&self) -> Result<&EntryValue, &EntryError> {
        match self.res {
            Some(ref v) => v.as_ref(),
            None => panic!("take result of entry twice"),
        }
    }

    /// Takes out the parsed result of this entry.
    ///
    /// If you need to convert this `ExifIter` to an [`crate::Exif`], please
    /// don't call this method! Otherwise the converted `Exif` is incomplete.
    ///
    /// Returns:
    ///
    /// - If any error occurred while parsing this entry, an
    ///   Err([`InvalidEntry`](crate::Error::InvalidEntry)) is returned.
    ///
    /// - Otherwise, an Ok([`EntryValue`]) is returned.
    ///
    /// **Note**: This method can ONLY be called once! If you call it twice, it
    /// will **panic** directly!
    pub fn take_result(&mut self) -> Result<EntryValue, EntryError> {
        match self.res.take() {
            Some(v) => v,
            None => panic!("take result of entry twice"),
        }
    }

    fn make_ok(ifd: usize, tag: ExifTagCode, v: EntryValue) -> Self {
        Self {
            ifd,
            tag,
            res: Some(Ok(v)),
        }
    }

    fn make_err(ifd: usize, tag: ExifTagCode, e: ParseEntryError) -> Self {
        Self {
            ifd,
            tag,
            res: Some(Err(EntryError(e))),
        }
    }
}

impl Debug for ParsedExifEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self.get_result() {
            Ok(v) => format!("{v}"),
            Err(e) => format!("{e:?}"),
        };
        f.debug_struct("IfdEntryResult")
            .field("ifd", &format!("ifd{}", self.ifd))
            .field("tag", &self.tag)
            .field("value", &value)
            .finish()
    }
}

const MAX_IFD_DEPTH: usize = 8;

impl Iterator for ExifIter {
    type Item = ParsedExifEntry;

    #[tracing::instrument(skip_all)]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.ifds.is_empty() {
                return None;
            }

            if self.ifds.len() > MAX_IFD_DEPTH {
                self.ifds.clear();
                tracing::error!(
                    ifds_depth = self.ifds.len(),
                    "ifd depth is too deep, just go back to ifd0"
                );
                self.ifds.push(self.ifd0.clone_with_state());
            }

            let mut ifd = self.ifds.pop()?;
            match ifd.next() {
                Some((tag_code, entry)) => {
                    // tracing::debug!(ifd = ifd.ifd_idx, ?tag_code, ?entry, "next tag entry");

                    match entry {
                        IfdEntry::IfdNew(new_ifd) => {
                            let is_subifd = if new_ifd.ifd_idx == ifd.ifd_idx {
                                // Push the current ifd before enter sub-ifd.
                                self.ifds.push(ifd);
                                true
                            } else {
                                // Otherwise this is a next ifd. It means that the
                                // current ifd has been parsed, so we don't need to
                                // push it.
                                false
                            };

                            let (ifd_idx, offset) = (new_ifd.ifd_idx, new_ifd.offset);
                            tracing::debug!(
                                ifd = new_ifd.ifd_idx,
                                offset = format!("0x{:08x}", new_ifd.offset),
                                entry_num = new_ifd.entry_num,
                                "new ifd"
                            );
                            self.ifds.push(new_ifd);

                            if is_subifd {
                                // Return sub-ifd as an entry
                                return Some(ParsedExifEntry::make_ok(
                                    ifd_idx,
                                    tag_code,
                                    EntryValue::U32(offset),
                                ));
                            }
                        }
                        IfdEntry::Entry(v) => {
                            let res = Some(ParsedExifEntry::make_ok(ifd.ifd_idx, tag_code, v));
                            self.ifds.push(ifd);
                            return res;
                        }
                        IfdEntry::Err(e) => {
                            tracing::warn!(?tag_code, ?e, "parse ifd entry error");
                            let res = Some(ParsedExifEntry::make_err(ifd.ifd_idx, tag_code, e));
                            self.ifds.push(ifd);
                            return res;
                        }
                    }
                }
                None => continue,
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct ImageFileDirectoryIter {
    ifd_idx: usize,

    // starts from "ifd/sub-ifd entries" (two bytes of ifd/sub-ifd entry num)
    input: AssociatedInput,

    // IFD data offset relative to the TIFF header.
    offset: u32,

    pub tz: Option<String>,
    endian: Endianness,
    entry_num: u16,

    // Iterating status
    index: u16,
    pos: usize,
}

impl Debug for ImageFileDirectoryIter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageFileDirectoryIter")
            .field("ifd_idx", &self.ifd_idx)
            .field("data len", &self.input.len())
            .field("offset", &self.offset)
            .field("tz", &self.tz)
            .field("endian", &self.endian)
            .field("entry_num", &self.entry_num)
            .field("index", &self.index)
            .field("pos", &self.pos)
            .finish_non_exhaustive()
    }
}

impl ImageFileDirectoryIter {
    pub fn rewind(&mut self) {
        self.index = 0;
        // Skip the first two bytes, which is the entry num
        self.pos = 2;
    }

    pub fn clone_and_rewind(&self) -> Self {
        let mut it = self.clone();
        it.rewind();
        it
    }

    #[tracing::instrument(skip(input))]
    pub fn try_new(
        ifd_idx: usize,
        input: AssociatedInput,
        offset: u32,
        endian: Endianness,
        tz: Option<String>,
    ) -> crate::Result<Self> {
        if input.len() < 2 {
            return Err(crate::Error::ParseFailed(
                "ifd data is too small to decode entry num".into(),
            ));
        }
        // should use the complete header data to parse ifd entry num
        let (_, entry_num) = TiffHeader::parse_ifd_entry_num(&input[..], endian)?;

        Ok(Self {
            ifd_idx,
            input,
            offset,
            entry_num,
            tz,
            endian,
            // Skip the first two bytes, which is the entry num
            pos: 2,
            index: 0,
        })
    }

    fn parse_tag_entry(&self, entry_data: &[u8]) -> Option<(u16, IfdEntry)> {
        let endian = self.endian;
        let (_, (tag, data_format, components_num, value_or_offset)) = tuple((
            complete::u16::<_, nom::error::Error<_>>(endian),
            complete::u16(endian),
            complete::u32(endian),
            complete::u32(endian),
        ))(entry_data)
        .ok()?;

        if tag == 0 {
            return None;
        }

        let df: DataFormat = match data_format.try_into() {
            Ok(df) => df,
            Err(e) => {
                let t: ExifTagCode = tag.into();
                tracing::warn!(tag = ?t, ?e, "invalid entry data format");
                return Some((tag, IfdEntry::Err(e)));
            }
        };
        let (tag, res) = self.parse_entry(tag, df, components_num, entry_data, value_or_offset);
        Some((tag, res))
    }

    fn get_data_pos(&self, value_or_offset: u32) -> u32 {
        value_or_offset.saturating_sub(self.offset)
    }

    fn parse_entry(
        &self,
        tag: u16,
        data_format: DataFormat,
        components_num: u32,
        entry_data: &[u8],
        value_or_offset: u32,
    ) -> (u16, IfdEntry) {
        // get component_size according to data format
        let component_size = data_format.component_size();

        // get entry data
        let size = components_num as usize * component_size;
        let data = if size <= 4 {
            &entry_data[8..8 + size] // Safe-slice
        } else {
            let start = self.get_data_pos(value_or_offset) as usize;
            let end = start + size;
            let Some(data) = self.input.slice_checked(start..end) else {
                tracing::error!(
                    "entry data overflow, self.offset: {:08x} tag: {:04x} start: {:08x} end: {:08x} ifd data len {:08x}",
                    self.offset,
                    tag,
                    start,
                    end,
                    self.input.len(),
                );
                return (tag, IfdEntry::Err(ParseEntryError::EntrySizeTooBig));
            };

            data
        };

        if SUBIFD_TAGS.contains(&tag) {
            if let Some(value) = self.new_ifd_iter(self.ifd_idx, value_or_offset, tag) {
                return value;
            }
        }

        let entry = EntryData {
            endian: self.endian,
            tag,
            data,
            data_format,
            components_num,
        };
        match EntryValue::parse(&entry, &self.tz) {
            Ok(v) => (tag, IfdEntry::Entry(v)),
            Err(e) => (tag, IfdEntry::Err(e)),
        }
    }

    fn new_ifd_iter(
        &self,
        ifd_idx: usize,
        value_or_offset: u32,
        tag: u16,
    ) -> Option<(u16, IfdEntry)> {
        let pos = self.get_data_pos(value_or_offset) as usize;
        if pos < self.input.len() {
            match ImageFileDirectoryIter::try_new(
                ifd_idx,
                self.input.partial(&self.input[pos..]),
                value_or_offset,
                self.endian,
                self.tz.clone(),
            ) {
                Ok(iter) => return Some((tag, IfdEntry::IfdNew(iter))),
                Err(e) => {
                    tracing::debug!(?tag, ?e, "parse sub ifd error");
                }
            }
            // return (
            //     tag,
            //     // IfdEntry::Ifd {
            //     //     idx: self.ifd_idx,
            //     //     offset: value_or_offset,
            //     // },
            //     IfdEntry::IfdNew(),
            // );
        }
        None
    }

    pub fn find_tz_offset(&self) -> Option<String> {
        let endian = self.endian;
        // find ExifOffset
        for i in 0..self.entry_num {
            let pos = self.pos + i as usize * IFD_ENTRY_SIZE;
            let (remain, tag) =
                complete::u16::<_, nom::error::Error<_>>(endian)(&self.input[pos..]).ok()?;
            if tag == ExifTag::ExifOffset.code() {
                let (_, (_, _, offset)) = tuple((
                    complete::u16::<_, nom::error::Error<_>>(endian),
                    complete::u32(endian),
                    complete::u32(endian),
                ))(remain)
                .ok()?;

                // find tz offset
                return self.find_tz_offset_in_exif_subifd(offset);
            }
        }
        None
    }

    fn find_tz_offset_in_exif_subifd(&self, offset: u32) -> Option<String> {
        let num_entries = self.entry_num;
        let pos = self.get_data_pos(offset + 2) as usize;
        for i in 0..num_entries {
            let pos = pos + i as usize * IFD_ENTRY_SIZE;
            let entry_data = self.input.slice_checked(pos..pos + IFD_ENTRY_SIZE)?;
            let (tag, res) = self.parse_tag_entry(entry_data)?;

            if TZ_OFFSET_TAGS.contains(&tag) {
                return match res {
                    IfdEntry::IfdNew(_) => {
                        tracing::warn!("got ifd when parsing tz offset");
                        continue;
                    }
                    IfdEntry::Entry(v) => match v {
                        EntryValue::Text(v) => return Some(v),
                        _ => {
                            tracing::warn!("tz offset is not a text");
                            continue;
                        }
                    },
                    IfdEntry::Err(_) => None,
                };
            }
        }
        None
    }

    // Assume the current ifd is GPSInfo subifd.
    pub fn parse_gps_info(&mut self) -> Option<GPSInfo> {
        let mut gps = GPSInfo::default();
        let mut has_data = false;
        for (tag, entry) in self {
            let Some(tag) = tag.tag() else {
                continue;
            };
            has_data = true;
            match tag {
                ExifTag::GPSLatitudeRef => {
                    if let Some(c) = entry.as_char() {
                        gps.latitude_ref = c;
                    }
                }
                ExifTag::GPSLongitudeRef => {
                    if let Some(c) = entry.as_char() {
                        gps.longitude_ref = c;
                    }
                }
                ExifTag::GPSAltitudeRef => {
                    if let Some(c) = entry.as_u8() {
                        gps.altitude_ref = c;
                    }
                }
                ExifTag::GPSLatitude => {
                    if let Some(v) = entry.as_urational_array() {
                        gps.latitude = v.iter().collect();
                    } else if let Some(v) = entry.as_irational_array() {
                        gps.latitude = v.iter().collect();
                    }
                }
                ExifTag::GPSLongitude => {
                    if let Some(v) = entry.as_urational_array() {
                        gps.longitude = v.iter().collect();
                    } else if let Some(v) = entry.as_irational_array() {
                        gps.longitude = v.iter().collect();
                    }
                }
                ExifTag::GPSAltitude => {
                    if let Some(v) = entry.as_urational() {
                        gps.altitude = *v;
                    } else if let Some(v) = entry.as_irational() {
                        gps.altitude = (*v).into();
                    }
                }
                ExifTag::GPSSpeedRef => {
                    if let Some(c) = entry.as_char() {
                        gps.speed_ref = Some(c);
                    }
                }
                ExifTag::GPSSpeed => {
                    if let Some(v) = entry.as_urational() {
                        gps.speed = Some(*v);
                    } else if let Some(v) = entry.as_irational() {
                        gps.speed = Some((*v).into());
                    }
                }
                _ => (),
            }
        }

        if has_data {
            Some(gps)
        } else {
            tracing::warn!("GPSInfo data not found");
            None
        }
    }

    fn clone_with_state(&self) -> ImageFileDirectoryIter {
        let mut it = self.clone();
        it.index = self.index;
        it.pos = self.pos;
        it
    }
}

#[derive(Debug)]
pub(crate) enum IfdEntry {
    IfdNew(ImageFileDirectoryIter), // ifd index
    Entry(EntryValue),
    Err(ParseEntryError),
}

impl IfdEntry {
    pub fn as_u8(&self) -> Option<u8> {
        if let IfdEntry::Entry(EntryValue::U8(v)) = self {
            Some(*v)
        } else {
            None
        }
    }

    pub fn as_char(&self) -> Option<char> {
        if let IfdEntry::Entry(EntryValue::Text(s)) = self {
            s.chars().next()
        } else {
            None
        }
    }

    fn as_irational(&self) -> Option<&IRational> {
        if let IfdEntry::Entry(EntryValue::IRational(v)) = self {
            Some(v)
        } else {
            None
        }
    }

    fn as_irational_array(&self) -> Option<&Vec<IRational>> {
        if let IfdEntry::Entry(EntryValue::IRationalArray(v)) = self {
            Some(v)
        } else {
            None
        }
    }

    fn as_urational(&self) -> Option<&URational> {
        if let IfdEntry::Entry(EntryValue::URational(v)) = self {
            Some(v)
        } else {
            None
        }
    }

    fn as_urational_array(&self) -> Option<&Vec<URational>> {
        if let IfdEntry::Entry(EntryValue::URationalArray(v)) = self {
            Some(v)
        } else {
            None
        }
    }
}

const SUBIFD_TAGS: &[u16] = &[ExifTag::ExifOffset.code(), ExifTag::GPSInfo.code()];
const TZ_OFFSET_TAGS: &[u16] = &[
    ExifTag::OffsetTimeOriginal.code(),
    ExifTag::OffsetTimeDigitized.code(),
    ExifTag::OffsetTime.code(),
];

impl Iterator for ImageFileDirectoryIter {
    type Item = (ExifTagCode, IfdEntry);

    #[tracing::instrument(skip(self))]
    fn next(&mut self) -> Option<Self::Item> {
        // tracing::debug!(
        //     ifd = self.ifd_idx,
        //     index = self.index,
        //     entry_num = self.entry_num,
        //     pos = format!("{:08x}", self.pos),
        //     "next IFD entry"
        // );
        if self.input.len() < self.pos + IFD_ENTRY_SIZE {
            return None;
        }

        let endian = self.endian;
        if self.index > self.entry_num {
            return None;
        }
        if self.index == self.entry_num {
            tracing::debug!(index = self.index, pos = self.pos, "next IFD");
            self.index += 1;
            // next IFD
            let (_, offset) =
                complete::u32::<_, nom::error::Error<_>>(endian)(&self.input[self.pos..]).ok()?;

            if offset == 0 {
                // IFD parsing completed
                return None;
            }

            return self
                .new_ifd_iter(self.ifd_idx + 1, offset, 0xffff_u16)
                .map(|x| (x.0.into(), x.1));
        }

        let entry_data = self
            .input
            .slice_checked(self.pos..self.pos + IFD_ENTRY_SIZE)?;
        self.index += 1;
        self.pos += IFD_ENTRY_SIZE;

        let (tag, res) = self.parse_tag_entry(entry_data)?;

        Some((tag.into(), res)) // Safe-slice
    }
}

// Only iterates headers, don't parse entries.
pub(crate) struct IFDHeaderIter<'a> {
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

impl<'a> IFDHeaderIter<'a> {
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
                tracing::error!(?e);
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

    fn parse_ifd_entry_header(&self, pos: u32) -> IResult<&[u8], Option<IFDHeaderIter<'a>>> {
        let (_, entry_data) =
            nom::bytes::streaming::take(IFD_ENTRY_SIZE)(&self.ifd_data[pos as usize..])?;

        let (remain, entry) = self.parse_tag_entry_header(entry_data)?;

        if let Some(entry) = entry {
            // if !cb(&entry) {
            //     return Ok((&[][..], ()));
            // }

            if let Some(offset) = entry.sub_ifd_offset {
                let tag: ExifTag = entry.tag.try_into().unwrap();
                println!("sub-ifd: {:?}", tag);
                let sub_ifd =
                    IFDHeaderIter::new(&self.ifd_data[offset as usize..], offset, self.endian);
                return Ok((remain, Some(sub_ifd)));
            }
        }

        Ok((remain, None))
    }

    #[tracing::instrument(skip(self))]
    pub fn parse_ifd_header(&mut self, depth: usize) -> Result<(), ParsingError> {
        if depth > 1 {
            let msg = "depth shouldn't be greater than 1";
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
                sub_ifds.push(ifd);
            }
        }

        for mut ifd in sub_ifds {
            ifd.parse_ifd_header(depth + 1)?;
        }

        // ignore ifd1
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

    use crate::exif::extract_exif_with_mime;
    use crate::exif::input_into_iter;
    use crate::file::MimeImage;
    use crate::slice::SubsliceRange;
    use crate::testkit::read_sample;
    use test_case::test_case;

    #[test_case("exif.jpg", "+08:00", MimeImage::Jpeg)]
    #[test_case("broken.jpg", "", MimeImage::Jpeg)]
    #[test_case("exif.heic", "+08:00", MimeImage::Heic)]
    #[test_case("tif.tif", "", MimeImage::Tiff)]
    fn exif_iter_tz(path: &str, tz: &str, img_type: MimeImage) {
        let buf = read_sample(path).unwrap();
        let data = extract_exif_with_mime(img_type, &buf, None).unwrap();
        let subslice_range = data.and_then(|x| buf.subslice_range(x)).unwrap();
        let iter = input_into_iter((buf, subslice_range), None).unwrap();
        let expect = if tz.is_empty() {
            None
        } else {
            Some(tz.to_string())
        };
        assert_eq!(iter.tz, expect);
    }
}

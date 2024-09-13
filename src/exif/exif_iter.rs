use std::{fmt::Debug, sync::Arc};

use nom::{number::complete, sequence::tuple};
use thiserror::Error;

use crate::{
    input::{AssociatedInput, Input},
    slice::SliceChecked,
    values::{DataFormat, EntryData, IRational, ParseEntryError, URational},
    EntryValue, ExifTag,
};

use super::{parser::IFD_ENTRY_SIZE, tags::ExifTagCode, GPSInfo, TiffHeader};

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
#[derive(Debug, Default)]
pub struct ExifIter<'a> {
    // Use Arc to make sure we won't clone the owned data.
    input: Arc<Input<'a>>,
    tiff_header: TiffHeader,
    tz: Option<String>,
    ifd0: Option<ImageFileDirectoryIter>,

    // Iterating status
    ifds: Vec<ImageFileDirectoryIter>,
}

impl Clone for ExifIter<'_> {
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

impl<'a> ExifIter<'a> {
    pub(crate) fn new(
        input: impl Into<Input<'a>>,
        tiff_header: TiffHeader,
        tz: Option<String>,
        ifd0: Option<ImageFileDirectoryIter>,
    ) -> ExifIter<'a> {
        let mut ifds = Vec::new();
        if let Some(ref ifd0) = ifd0 {
            ifds.push(ifd0.clone());
        }
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
        let mut ifds = Vec::new();
        if let Some(ref ifd0) = self.ifd0 {
            ifds.push(ifd0.clone_and_rewind());
        }
        Self {
            input: self.input.clone(),
            tiff_header: self.tiff_header.clone(),
            tz: self.tz.clone(),
            ifd0: self.ifd0.as_ref().map(|x| x.clone_and_rewind()),
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
    pub fn parse_gps_info(&self) -> crate::Result<Option<GPSInfo>> {
        let mut iter = self.shallow_clone();
        let Some(gps) = iter.find(|x| x.tag.tag().is_some_and(|t| t == ExifTag::GPSInfo)) else {
            return Ok(None);
        };

        let offset = match gps.get_result() {
            Ok(v) => v.as_u32().unwrap() as usize,
            Err(e) => return Err(e.clone().into()),
        };

        let data = &iter.input[..];
        let mut gps_subifd = match ImageFileDirectoryIter::try_new(
            gps.ifd,
            iter.input.make_associated(data),
            iter.tiff_header.clone(),
            offset,
            iter.tz.clone(),
        ) {
            Ok(ifd0) => ifd0,
            Err(e) => return Err(e),
        };
        Ok(gps_subifd.parse_gps_info())
    }

    // Make sure we won't clone the owned data.
    fn shallow_clone(&'a self) -> Self {
        ExifIter::new(
            &self.input[..],
            self.tiff_header.clone(),
            self.tz.clone(),
            self.ifd0.as_ref().map(|x| x.clone_and_rewind()),
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

impl<'a> Iterator for ExifIter<'a> {
    type Item = ParsedExifEntry;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.ifds.len() > MAX_IFD_DEPTH {
                self.ifds.clear();
                tracing::error!(
                    ifds_depth = self.ifds.len(),
                    "ifd depth is too deep, just go back to ifd0"
                );
                if let Some(ref ifd0) = self.ifd0 {
                    self.ifds.push(ifd0.clone_with_state());
                }
            }

            let mut ifd = self.ifds.pop()?;
            match ifd.next() {
                Some((tag_code, entry)) => match entry {
                    IfdEntry::Ifd { idx, offset } => {
                        let is_subifd = if idx == ifd.ifd_idx {
                            // Push the current ifd before enter sub-ifd.
                            self.ifds.push(ifd);
                            true
                        } else {
                            // Otherwise this is a next ifd. It means that the
                            // current ifd has been parsed, so we don't need to
                            // push it.
                            false
                        };

                        if let Ok(ifd) = ImageFileDirectoryIter::try_new(
                            idx,
                            self.input.make_associated(&self.input[..]),
                            self.tiff_header.clone(),
                            offset,
                            self.tz.clone(),
                        ) {
                            self.ifds.push(ifd);
                        }

                        if is_subifd {
                            // Return sub-ifd as an entry
                            return Some(ParsedExifEntry::make_ok(
                                idx,
                                tag_code,
                                EntryValue::U32(offset as u32),
                            ));
                        }
                    }
                    IfdEntry::Entry(v) => {
                        let res = Some(ParsedExifEntry::make_ok(ifd.ifd_idx, tag_code, v));
                        self.ifds.push(ifd);
                        return res;
                    }
                    IfdEntry::Err(e) => {
                        let res = Some(ParsedExifEntry::make_err(ifd.ifd_idx, tag_code, e));
                        self.ifds.push(ifd);
                        return res;
                    }
                },
                None => continue,
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ImageFileDirectoryIter {
    pub ifd_idx: usize,
    pub input: AssociatedInput,
    pub tiff_header: TiffHeader,
    pub pos: usize,
    pub entry_num: u16,
    pub tz: Option<String>,

    // Iterating status
    pub index: u16,
}

impl ImageFileDirectoryIter {
    pub fn rewind(&mut self) {
        self.index = 0;
    }

    pub fn clone_and_rewind(&self) -> Self {
        let mut it = self.clone();
        it.rewind();
        it
    }

    pub fn try_new(
        ifd_idx: usize,
        input: AssociatedInput,
        tiff_header: TiffHeader,
        pos: usize,
        tz: Option<String>,
    ) -> crate::Result<Self> {
        // should use the complete header data to parse ifd entry num
        let (_, entry_num) = TiffHeader::parse_ifd_entry_num(&input, pos, tiff_header.endian)?;

        Ok(Self {
            ifd_idx,
            input,
            tiff_header,
            pos: pos + 2, // Skip ifd entry num field
            entry_num,
            tz,
            index: 0,
        })
    }

    fn parse_tag_entry(&self, entry_data: &[u8]) -> Option<(u16, IfdEntry)> {
        let endian = self.tiff_header.endian;
        let (_, (tag, data_format, components_num, value_or_offset)) = tuple((
            complete::u16::<_, nom::error::Error<_>>(endian),
            complete::u16(endian),
            complete::u32(endian),
            complete::u32(endian),
        ))(entry_data)
        .ok()?;

        let df: DataFormat = match data_format.try_into() {
            Ok(df) => df,
            Err(e) => return Some((tag, IfdEntry::Err(e))),
        };
        let (tag, res) = self.parse_entry(tag, df, components_num, entry_data, value_or_offset);
        Some((tag, res))
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
            let start = value_or_offset as usize;
            let end = start + size;
            let Some(data) = self.input.slice_checked(start..end) else {
                return (tag, IfdEntry::Err(ParseEntryError::EntrySizeTooBig));
            };

            // Is `start` should be greater than or equal to `pos + ENTRY_SIZE` ?

            data
        };

        if SUBIFD_TAGS.contains(&tag) {
            if (value_or_offset as usize) < self.input.len() {
                return (
                    tag,
                    IfdEntry::Ifd {
                        idx: self.ifd_idx,
                        offset: value_or_offset as usize,
                    },
                );
            } else {
                return (tag, IfdEntry::Err(ParseEntryError::EntrySizeTooBig));
            }
        }

        let entry = EntryData {
            endian: self.tiff_header.endian,
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

    pub fn find_tz_offset(&self) -> Option<String> {
        let endian = self.tiff_header.endian;
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
                return self.find_tz_offset_in_exif_subifd(offset as usize);
            }
        }
        None
    }

    fn find_tz_offset_in_exif_subifd(&self, offset: usize) -> Option<String> {
        let num_entries = self.entry_num;
        let pos = offset + 2;
        for i in 0..num_entries {
            let pos = pos + i as usize * IFD_ENTRY_SIZE;
            let entry_data = self.input.slice_checked(pos..pos + IFD_ENTRY_SIZE)?;
            let (tag, res) = self.parse_tag_entry(entry_data)?;
            if TZ_OFFSET_TAGS.contains(&tag) {
                return match res {
                    IfdEntry::Ifd { idx: _, offset: _ } => unreachable!(),
                    IfdEntry::Entry(v) => match v {
                        EntryValue::Text(v) => Some(v),
                        _ => unreachable!(),
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
            None
        }
    }

    fn clone_with_state(&self) -> ImageFileDirectoryIter {
        let mut it = self.clone();
        it.index = self.index;
        it
    }
}

#[derive(Debug)]
pub(crate) enum IfdEntry {
    Ifd { idx: usize, offset: usize }, // ifd index
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

    fn next(&mut self) -> Option<Self::Item> {
        let endian = self.tiff_header.endian;
        if self.index >= self.entry_num {
            // next IFD
            let (_, offset) =
                complete::u32::<_, nom::error::Error<_>>(endian)(&self.input[self.pos..]).ok()?;
            let offset = offset as usize;

            if offset == 0 {
                // IFD parsing completed
                return None;
            } else if offset >= self.input.len() {
                // Ignore this error
                return None;
            } else {
                return Some((
                    ExifTagCode::Code(0),
                    IfdEntry::Ifd {
                        idx: self.ifd_idx + 1,
                        offset,
                    },
                ));
            }
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

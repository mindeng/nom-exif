use std::{collections::HashSet, fmt::Debug};

use bytes::Bytes;
use nom::{number::complete, Parser};

use crate::{
    error::EntryError,
    slice::SliceChecked,
    values::{DataFormat, EntryData, IRational, URational},
    EntryValue, ExifTag,
};

use super::{exif_exif::IFD_ENTRY_SIZE, GPSInfo, LatLng, TiffHeader};
use crate::TagOrCode;

/// Index of an IFD (Image File Directory) within an EXIF blob.
///
/// `0` = main image (`IfdIndex::MAIN`), `1` = thumbnail (`IfdIndex::THUMBNAIL`),
/// `>=2` = sub-IFDs in the order encountered. Use the constants for the common
/// cases and [`IfdIndex::new`] for raw indexing.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IfdIndex(usize);

impl IfdIndex {
    /// Index of the main image IFD (always `0`).
    pub const MAIN: Self = IfdIndex(0);

    /// Index of the thumbnail IFD (`1` when present).
    pub const THUMBNAIL: Self = IfdIndex(1);

    /// Construct from a raw index. `0`/`1` correspond to [`Self::MAIN`] /
    /// [`Self::THUMBNAIL`]; values `>= 2` are sub-IFDs.
    pub const fn new(index: usize) -> Self {
        IfdIndex(index)
    }

    /// Underlying raw index.
    pub const fn get(self) -> usize {
        self.0
    }
}

impl std::fmt::Display for IfdIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ifd{}", self.0)
    }
}

/// Eager view into a single Exif entry. Yielded by [`crate::Exif::iter`] and
/// designed to be cheap to copy: the `value` is a borrow into the parent
/// [`crate::Exif`].
///
/// # Why pub fields instead of getters?
///
/// `ifd`, `tag`, and `value` are independent — there is no cross-field
/// invariant to enforce. The Rust idiom for plain data carriers (cf.
/// [`std::ops::Range`]) is `pub` fields. The lazy yield type
/// [`crate::ExifIterEntry`] uses *private* fields because it carries a
/// `value xor error` invariant.
#[derive(Clone, Copy, Debug)]
pub struct ExifEntry<'a> {
    pub ifd: IfdIndex,
    pub tag: TagOrCode,
    pub value: &'a crate::EntryValue,
}

/// Represents an additional TIFF data block to be processed after the primary block.
/// Used for CR3 files with multiple CMT boxes (CMT1, CMT2, CMT3).
#[derive(Clone)]
pub(crate) struct TiffDataBlock {
    /// Block identifier (e.g., "CMT1", "CMT2", "CMT3")
    #[allow(dead_code)]
    pub block_id: String,
    /// Pre-sliced bytes view for this block's data
    pub data: Bytes,
    /// TIFF header information (optional, if known)
    pub header: Option<TiffHeader>,
}

/// Parses header from input data, and returns an [`ExifIter`].
///
/// All entries are lazy-parsed. That is, only when you iterate over
/// [`ExifIter`] will the IFD entries be parsed one by one.
///
/// The one exception is the time zone entries. The method will try to find
/// and parse the time zone data first, so we can correctly parse all time
/// information in subsequent iterates.
#[tracing::instrument]
pub(crate) fn input_into_iter(
    input: impl Into<bytes::Bytes> + Debug,
    state: Option<TiffHeader>,
) -> crate::Result<ExifIter> {
    let input: bytes::Bytes = input.into();
    let header = match state {
        // header has been parsed, and header has been skipped, input data
        // is the IFD data
        Some(header) => header,
        _ => {
            // header has not been parsed, input data includes IFD header
            let (_, header) = TiffHeader::parse(&input[..])?;

            tracing::debug!(
                ?header,
                data_len = format!("{:#x}", input.len()),
                "TIFF header parsed"
            );
            header
        }
    };

    let start = header.ifd0_offset as usize;
    if start > input.len() {
        return Err(crate::Error::UnexpectedEof {
            context: "exif iter init",
        });
    }
    tracing::debug!(?header, offset = start);

    let mut ifd0 = IfdIter::try_new(0, input.clone(), header.to_owned(), start, None)?;

    let tz = ifd0.find_tz_offset();
    ifd0.tz = tz.clone();
    let iter: ExifIter = ExifIter::new(input, header, tz, ifd0);

    tracing::debug!(?iter, "got IFD0");

    Ok(iter)
}

/// An iterator version of [`Exif`](crate::Exif). Use [`ParsedExifEntry`] as
/// iterator items.
///
/// Clone an `ExifIter` is very cheap; the underlying data is shared
/// via `bytes::Bytes` reference counting.
///
/// The new cloned `ExifIter`'s iteration index will be reset to the first one.
///
/// If you want to convert an `ExifIter` `into` an [`Exif`](crate::Exif), you probably want
/// to clone the `ExifIter` and use the new cloned one to do the converting.
/// Since the original's iteration index may have been modified by
/// `Iterator::next()` calls.
pub struct ExifIter {
    input: Bytes,
    tiff_header: TiffHeader,
    tz: Option<String>,
    ifd0: IfdIter,

    // Iterating status
    ifds: Vec<IfdIter>,
    visited_offsets: HashSet<usize>,

    // Multi-block support for CR3 files with multiple CMT boxes
    /// Additional TIFF data blocks to process after the primary block
    additional_blocks: Vec<TiffDataBlock>,
    /// Current block index: 0 = primary block, 1+ = additional blocks
    current_block_index: usize,
    /// Tags encountered so far for duplicate filtering (ifd_index, tag_code)
    encountered_tags: HashSet<(usize, u16)>,
}

impl Debug for ExifIter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExifIter")
            .field("data len", &self.input.len())
            .field("tiff_header", &self.tiff_header)
            .field("ifd0", &self.ifd0)
            .field("state", &self.ifds.first().map(|x| (x.index, x.pos)))
            .field("ifds num", &self.ifds.len())
            .field("additional_blocks", &self.additional_blocks.len())
            .field("current_block_index", &self.current_block_index)
            .finish_non_exhaustive()
    }
}

impl Clone for ExifIter {
    fn clone(&self) -> Self {
        self.clone_and_rewind()
    }
}

impl ExifIter {
    pub(crate) fn new(
        input: bytes::Bytes,
        tiff_header: TiffHeader,
        tz: Option<String>,
        ifd0: IfdIter,
    ) -> ExifIter {
        let ifds = vec![ifd0.clone()];
        ExifIter {
            input,
            tiff_header,
            tz,
            ifd0,
            ifds,
            visited_offsets: HashSet::new(),
            additional_blocks: Vec::new(),
            current_block_index: 0,
            encountered_tags: HashSet::new(),
        }
    }

    /// Clone and rewind the iterator's index.
    ///
    /// Clone an `ExifIter` is very cheap; the underlying data is shared
    /// via `bytes::Bytes` reference counting.
    pub fn clone_and_rewind(&self) -> Self {
        let ifd0 = self.ifd0.clone_and_rewind();
        let ifds = vec![ifd0.clone()];
        Self {
            input: self.input.clone(),
            tiff_header: self.tiff_header.clone(),
            tz: self.tz.clone(),
            ifd0,
            ifds,
            visited_offsets: HashSet::new(),
            additional_blocks: self.additional_blocks.clone(),
            current_block_index: 0,
            encountered_tags: HashSet::new(),
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
                    return Err(EntryError::InvalidValue("invalid gps offset").into());
                }
            }
            Err(e) => return Err(e.clone().into()),
        };
        if offset as usize >= iter.input.len() {
            return Err(crate::Error::Malformed {
                kind: crate::error::MalformedKind::IfdEntry,
                message: "GPSInfo offset out of range".into(),
            });
        }

        let mut gps_subifd = match IfdIter::try_new(
            gps.ifd,
            iter.input.clone(),
            iter.tiff_header,
            offset as usize,
            iter.tz.clone(),
        ) {
            Ok(ifd0) => ifd0.tag_code(ExifTag::GPSInfo.code()),
            Err(e) => return Err(e),
        };
        Ok(gps_subifd.parse_gps_info())
    }

    /// Add an additional TIFF data block to be iterated after the current block.
    /// Used internally for CR3 files with multiple CMT boxes.
    ///
    /// # Arguments
    /// * `block_id` - Identifier for this TIFF block (e.g., "CMT2", "CMT3")
    /// * `data` - Pre-sliced `Bytes` view containing this block's TIFF data
    /// * `header` - Optional TIFF header if already parsed
    pub(crate) fn add_tiff_block(
        &mut self,
        block_id: String,
        data: bytes::Bytes,
        header: Option<TiffHeader>,
    ) {
        self.additional_blocks.push(TiffDataBlock {
            block_id,
            data,
            header,
        });
    }
}

/// Represents a parsed IFD entry. Used as iterator items in [`ExifIter`].
#[derive(Clone)]
pub struct ParsedExifEntry {
    // 0: ifd0, 1: ifd1
    ifd: usize,
    tag: TagOrCode,
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
            TagOrCode::Tag(t) => Some(t),
            TagOrCode::Unknown(_) => None,
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
    ///   `Err(&EntryError)` is returned.
    ///
    /// - Otherwise, an `Ok(&EntryValue)` is returned.
    #[allow(rustdoc::private_intra_doc_links)]
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
    ///   `Err(EntryError)` is returned.
    ///
    /// - Otherwise, an `Ok(EntryValue)` is returned.
    ///
    /// **Note**: This method can ONLY be called once! If you call it twice, it
    /// will **panic** directly!
    pub fn take_result(&mut self) -> Result<EntryValue, EntryError> {
        match self.res.take() {
            Some(v) => v,
            None => panic!("take result of entry twice"),
        }
    }

    fn make_ok(ifd: usize, tag: TagOrCode, v: EntryValue) -> Self {
        Self {
            ifd,
            tag,
            res: Some(Ok(v)),
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

impl ExifIter {
    /// Attempt to load and start iterating the next additional TIFF block.
    /// Returns true if a new block was successfully loaded, false if no more blocks.
    fn load_next_block(&mut self) -> bool {
        // Move to the next additional block
        let block_index = self.current_block_index;
        if block_index >= self.additional_blocks.len() {
            return false;
        }

        let block = &self.additional_blocks[block_index];
        tracing::debug!(
            block_id = block.block_id,
            block_index,
            "Loading additional TIFF block"
        );

        // Get the data for this block from the shared input
        let block_data = block.data.clone();
        let header = block.header.clone();

        // Try to create an ExifIter for this block
        match input_into_iter(block_data, header) {
            Ok(iter) => {
                // Update our state with the new block's data
                self.ifd0 = iter.ifd0;
                self.ifds = vec![self.ifd0.clone()];
                self.visited_offsets.clear();
                self.current_block_index += 1;

                tracing::debug!(block_index, "Successfully loaded additional TIFF block");
                true
            }
            Err(e) => {
                tracing::warn!(
                    block_index,
                    error = %e,
                    "Failed to load additional TIFF block, skipping"
                );
                // Move to next block and try again
                self.current_block_index += 1;
                self.load_next_block()
            }
        }
    }

    /// Check if a tag should be included based on duplicate filtering.
    /// Returns true if the tag should be included, false if it's a duplicate.
    fn should_include_tag(&mut self, ifd_index: usize, tag_code: u16) -> bool {
        let tag_key = (ifd_index, tag_code);
        if self.encountered_tags.contains(&tag_key) {
            tracing::debug!(ifd_index, tag_code, "Skipping duplicate tag");
            false
        } else {
            self.encountered_tags.insert(tag_key);
            true
        }
    }
}

impl Iterator for ExifIter {
    type Item = ParsedExifEntry;

    #[tracing::instrument(skip_all)]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.ifds.is_empty() {
                // Current block exhausted, try to load next additional block
                if !self.load_next_block() {
                    tracing::debug!(?self, "all IFDs and blocks have been parsed");
                    return None;
                }
                // Continue with the newly loaded block
                continue;
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
            let cur_ifd_idx = ifd.ifd_idx;
            match ifd.next() {
                Some((tag_code, entry)) => {
                    tracing::debug!(ifd = ifd.ifd_idx, ?tag_code, "next tag entry");

                    match entry {
                        IfdEntry::IfdNew(new_ifd) => {
                            if new_ifd.offset > 0 {
                                if self.visited_offsets.contains(&new_ifd.offset) {
                                    // Ignore repeated ifd parsing to avoid dead looping
                                    continue;
                                }
                                self.visited_offsets.insert(new_ifd.offset);
                            }

                            let is_subifd = if new_ifd.ifd_idx == ifd.ifd_idx {
                                // Push the current ifd before enter sub-ifd.
                                self.ifds.push(ifd);
                                tracing::debug!(?tag_code, ?new_ifd, "got new SUB-IFD");
                                true
                            } else {
                                // Otherwise this is a next ifd. It means that the
                                // current ifd has been parsed, so we don't need to
                                // push it.
                                tracing::debug!("IFD{} parsing completed", cur_ifd_idx);
                                tracing::debug!(?new_ifd, "got new IFD");
                                false
                            };

                            let (ifd_idx, offset) = (new_ifd.ifd_idx, new_ifd.offset);
                            self.ifds.push(new_ifd);

                            if is_subifd {
                                // Check for duplicates before returning sub-ifd entry
                                let tc = tag_code.unwrap();
                                if !self.should_include_tag(ifd_idx, tc.code()) {
                                    continue;
                                }
                                // Return sub-ifd as an entry
                                return Some(ParsedExifEntry::make_ok(
                                    ifd_idx,
                                    tc,
                                    EntryValue::U32(offset as u32),
                                ));
                            }
                        }
                        IfdEntry::Entry(v) => {
                            let tc = tag_code.unwrap();
                            // Check for duplicates before returning entry
                            if !self.should_include_tag(ifd.ifd_idx, tc.code()) {
                                self.ifds.push(ifd);
                                continue;
                            }
                            let res = Some(ParsedExifEntry::make_ok(ifd.ifd_idx, tc, v));
                            self.ifds.push(ifd);
                            return res;
                        }
                        IfdEntry::Err(e) => {
                            tracing::warn!(?tag_code, ?e, "parse ifd entry error");
                            self.ifds.push(ifd);
                            continue;
                        }
                    }
                }
                None => continue,
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct IfdIter {
    ifd_idx: usize,
    tag_code: Option<TagOrCode>,

    // starts from TIFF header
    input: Bytes,

    // ifd data offset
    offset: usize,

    header: TiffHeader,
    entry_num: u16,

    pub tz: Option<String>,

    // Iterating status
    index: u16,
    pos: usize,
}

impl Debug for IfdIter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IfdIter")
            .field("ifd_idx", &self.ifd_idx)
            .field("tag", &self.tag_code)
            .field("data len", &self.input.len())
            .field("tz", &self.tz)
            .field("header", &self.header)
            .field("entry_num", &self.entry_num)
            .field("index", &self.index)
            .field("pos", &self.pos)
            .finish()
    }
}

impl IfdIter {
    pub fn rewind(&mut self) {
        self.index = 0;
        // Skip the first two bytes, which is the entry num
        self.pos = self.offset + 2;
    }

    pub fn clone_and_rewind(&self) -> Self {
        let mut it = self.clone();
        it.rewind();
        it
    }

    pub fn tag_code_maybe(mut self, code: Option<u16>) -> Self {
        self.tag_code = code.map(|x| x.into());
        self
    }

    pub fn tag_code(mut self, code: u16) -> Self {
        self.tag_code = Some(code.into());
        self
    }

    #[allow(unused)]
    pub fn tag(mut self, tag: TagOrCode) -> Self {
        self.tag_code = Some(tag);
        self
    }

    #[tracing::instrument(skip(input))]
    pub fn try_new(
        ifd_idx: usize,
        input: Bytes,
        header: TiffHeader,
        offset: usize,
        tz: Option<String>,
    ) -> crate::Result<Self> {
        if input.len() < 2 {
            return Err(crate::Error::Malformed {
                kind: crate::error::MalformedKind::TiffHeader,
                message: "ifd data too small to decode entry num".into(),
            });
        }
        // should use the complete header data to parse ifd entry num
        assert!(offset <= input.len());
        let ifd_data = input.slice(offset..);
        let (_, entry_num) = TiffHeader::parse_ifd_entry_num(&ifd_data, header.endian)?;

        Ok(Self {
            ifd_idx,
            tag_code: None,
            input,
            offset,
            header,
            entry_num,
            tz,
            // Skip the first two bytes, which is the entry num
            pos: offset + 2,
            index: 0,
        })
    }

    fn parse_tag_entry(&self, entry_data: &[u8]) -> Option<(u16, IfdEntry)> {
        let endian = self.header.endian;
        let (_, (tag, data_format, components_num, value_or_offset)) = (
            complete::u16::<_, nom::error::Error<_>>(endian),
            complete::u16(endian),
            complete::u32(endian),
            complete::u32(endian),
        ).parse(entry_data)
        .ok()?;

        if tag == 0 {
            return None;
        }

        let df: DataFormat = match DataFormat::try_from(data_format) {
            Ok(df) => df,
            Err(bad) => {
                let t: TagOrCode = tag.into();
                tracing::warn!(tag = ?t, format = bad, "invalid entry data format");
                return Some((
                    tag,
                    IfdEntry::Err(EntryError::InvalidShape {
                        format: bad,
                        count: components_num,
                    }),
                ));
            }
        };
        let (tag, res) = self.parse_entry(tag, df, components_num, entry_data, value_or_offset);
        Some((tag, res))
    }

    fn get_data_pos(&self, value_or_offset: u32) -> usize {
        // value_or_offset.saturating_sub(self.offset)
        value_or_offset as usize
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
            let start = self.get_data_pos(value_or_offset);
            let end = start + size;
            let Some(data) = self.input.slice_checked(start..end) else {
                tracing::warn!(
                    "entry data overflow, tag: {:04x} start: {:08x} end: {:08x} ifd data len {:08x}",
                    tag,
                    start,
                    end,
                    self.input.len(),
                );
                return (
                    tag,
                    IfdEntry::Err(EntryError::Truncated {
                        needed: size,
                        available: self.input.len().saturating_sub(start),
                    }),
                );
            };

            data
        };

        if SUBIFD_TAGS.contains(&tag) {
            if let Some(value) = self.new_ifd_iter(self.ifd_idx, value_or_offset, Some(tag)) {
                return (tag, value);
            }
        }

        let entry = EntryData {
            endian: self.header.endian,
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
        tag: Option<u16>,
    ) -> Option<IfdEntry> {
        let offset = self.get_data_pos(value_or_offset);
        if offset < self.input.len() {
            match IfdIter::try_new(
                ifd_idx,
                self.input.clone(),
                self.header.to_owned(),
                offset,
                self.tz.clone(),
            ) {
                Ok(iter) => return Some(IfdEntry::IfdNew(iter.tag_code_maybe(tag))),
                Err(e) => {
                    tracing::warn!(?tag, ?e, "Create next/sub IFD failed");
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

    pub fn find_exif_iter(&self) -> Option<IfdIter> {
        let endian = self.header.endian;
        // find ExifOffset
        for i in 0..self.entry_num {
            let pos = self.pos + i as usize * IFD_ENTRY_SIZE;
            let (_, tag) =
                complete::u16::<_, nom::error::Error<_>>(endian)(&self.input[pos..]).ok()?;
            if tag == ExifTag::ExifOffset.code() {
                let entry_data = self.input.slice_checked(pos..pos + IFD_ENTRY_SIZE)?;
                let (_, entry) = self.parse_tag_entry(entry_data)?;
                match entry {
                    IfdEntry::IfdNew(iter) => return Some(iter),
                    IfdEntry::Entry(_) | IfdEntry::Err(_) => return None,
                }
            }
        }
        None
    }

    pub fn find_tz_offset(&self) -> Option<String> {
        let iter = self.find_exif_iter()?;
        let mut offset = None;
        for entry in iter {
            let Some(tag) = entry.0 else {
                continue;
            };
            if tag.code() == ExifTag::OffsetTimeOriginal.code()
                || tag.code() == ExifTag::OffsetTimeDigitized.code()
            {
                return entry.1.as_str().map(|x| x.to_owned());
            } else if tag.code() == ExifTag::OffsetTime.code() {
                offset = entry.1.as_str().map(|x| x.to_owned());
            }
        }

        offset
    }

    // Assume the current ifd is GPSInfo subifd.
    pub fn parse_gps_info(&mut self) -> Option<GPSInfo> {
        use crate::exif::gps::{Altitude, LatRef, LonRef, Speed, SpeedUnit};

        let mut latitude_ref = None;
        let mut latitude = None;
        let mut longitude_ref = None;
        let mut longitude = None;
        let mut altitude_ref = None;
        let mut altitude_value = None;
        let mut speed_unit = None;
        let mut speed_value = None;
        let mut has_data = false;

        for (tag, entry) in self {
            let Some(tag) = tag.and_then(|x| x.tag()) else { continue };
            has_data = true;
            match tag {
                ExifTag::GPSLatitudeRef => {
                    latitude_ref = entry.as_char().and_then(LatRef::from_char);
                }
                ExifTag::GPSLongitudeRef => {
                    longitude_ref = entry.as_char().and_then(LonRef::from_char);
                }
                ExifTag::GPSAltitudeRef => {
                    altitude_ref = entry.as_u8();
                }
                ExifTag::GPSLatitude => {
                    if let Some(v) = entry.as_urational_slice() {
                        latitude = LatLng::try_from(v).ok();
                    } else if let Some(v) = entry.as_irational_slice() {
                        latitude = LatLng::try_from(v).ok();
                    }
                }
                ExifTag::GPSLongitude => {
                    if let Some(v) = entry.as_urational_slice() {
                        longitude = LatLng::try_from(v).ok();
                    } else if let Some(v) = entry.as_irational_slice() {
                        longitude = LatLng::try_from(v).ok();
                    }
                }
                ExifTag::GPSAltitude => {
                    if let Some(v) = entry.as_urational() {
                        altitude_value = Some(*v);
                    } else if let Some(v) = entry.as_irational() {
                        if let Ok(u) = URational::try_from(*v) {
                            altitude_value = Some(u);
                        }
                    }
                }
                ExifTag::GPSSpeedRef => {
                    speed_unit = entry.as_char().and_then(SpeedUnit::from_char);
                }
                ExifTag::GPSSpeed => {
                    if let Some(v) = entry.as_urational() {
                        speed_value = Some(*v);
                    } else if let Some(v) = entry.as_irational() {
                        if let Ok(u) = URational::try_from(*v) {
                            speed_value = Some(u);
                        }
                    }
                }
                _ => (),
            }
        }

        if !has_data {
            tracing::warn!("GPSInfo data not found");
            return None;
        }

        let altitude = match (altitude_ref, altitude_value) {
            (Some(0), Some(v)) => Altitude::AboveSeaLevel(v),
            (Some(1), Some(v)) => Altitude::BelowSeaLevel(v),
            _ => Altitude::Unknown,
        };

        let speed = match (speed_unit, speed_value) {
            (Some(unit), Some(value)) => Some(Speed { unit, value }),
            _ => None,
        };

        Some(GPSInfo {
            latitude_ref: latitude_ref.unwrap_or(LatRef::North),
            latitude: latitude.unwrap_or_default(),
            longitude_ref: longitude_ref.unwrap_or(LonRef::East),
            longitude: longitude.unwrap_or_default(),
            altitude,
            speed,
        })
    }

    fn clone_with_state(&self) -> IfdIter {
        let mut it = self.clone();
        it.index = self.index;
        it.pos = self.pos;
        it
    }
}

#[derive(Debug)]
pub(crate) enum IfdEntry {
    IfdNew(IfdIter), // ifd index
    Entry(EntryValue),
    Err(EntryError),
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

    fn as_irational_slice(&self) -> Option<&Vec<IRational>> {
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

    fn as_urational_slice(&self) -> Option<&Vec<URational>> {
        if let IfdEntry::Entry(EntryValue::URationalArray(v)) = self {
            Some(v)
        } else {
            None
        }
    }

    fn as_str(&self) -> Option<&str> {
        if let IfdEntry::Entry(e) = self {
            e.as_str()
        } else {
            None
        }
    }
}

pub(crate) const SUBIFD_TAGS: &[u16] = &[ExifTag::ExifOffset.code(), ExifTag::GPSInfo.code()];

impl Iterator for IfdIter {
    type Item = (Option<TagOrCode>, IfdEntry);

    #[tracing::instrument(skip(self))]
    fn next(&mut self) -> Option<Self::Item> {
        tracing::debug!(
            ifd = self.ifd_idx,
            index = self.index,
            entry_num = self.entry_num,
            offset = format!("{:08x}", self.offset),
            pos = format!("{:08x}", self.pos),
            "next IFD entry"
        );
        if self.input.len() < self.pos + IFD_ENTRY_SIZE {
            return None;
        }

        let endian = self.header.endian;
        if self.index > self.entry_num {
            return None;
        }
        if self.index == self.entry_num {
            tracing::debug!(
                self.ifd_idx,
                self.index,
                pos = self.pos,
                "try to get next ifd"
            );
            self.index += 1;

            // next IFD offset
            let (_, offset) =
                complete::u32::<_, nom::error::Error<_>>(endian)(&self.input[self.pos..]).ok()?;

            if offset == 0 {
                // IFD parsing completed
                tracing::debug!(?self, "IFD parsing completed");
                return None;
            }

            return self
                .new_ifd_iter(self.ifd_idx + 1, offset, None)
                .map(|x| (None, x));
        }

        let entry_data = self
            .input
            .slice_checked(self.pos..self.pos + IFD_ENTRY_SIZE)?;
        self.index += 1;
        self.pos += IFD_ENTRY_SIZE;

        let (tag, res) = self.parse_tag_entry(entry_data)?;

        Some((Some(tag.into()), res)) // Safe-slice
    }
}

#[cfg(test)]
mod tests {

    use crate::exif::extract_exif_with_mime;
    use crate::exif::input_into_iter;
    use crate::file::MediaMimeImage;
    use crate::slice::SubsliceRange;
    use crate::testkit::read_sample;
    use crate::Exif;
    use test_case::test_case;

    #[test_case("exif.jpg", "+08:00", "2023-07-09T20:36:33+08:00", MediaMimeImage::Jpeg)]
    #[test_case("exif-no-tz.jpg", "", "2023-07-09 20:36:33", MediaMimeImage::Jpeg)]
    #[test_case("broken.jpg", "-", "2014-09-21 15:51:22", MediaMimeImage::Jpeg)]
    #[test_case("exif.heic", "+08:00", "2022-07-22T21:26:32+08:00", MediaMimeImage::Heic)]
    #[test_case("tif.tif", "-", "-", MediaMimeImage::Tiff)]
    #[test_case(
        "fujifilm_x_t1_01.raf.meta",
        "-",
        "2014-01-30 12:49:13",
        MediaMimeImage::Raf
    )]
    fn exif_iter_tz(path: &str, tz: &str, time: &str, img_type: MediaMimeImage) {
        let buf = read_sample(path).unwrap();
        let (data, _) = extract_exif_with_mime(img_type, &buf, None).unwrap();
        let range = data.and_then(|x| buf.subslice_in_range(x)).unwrap();
        let iter = input_into_iter(bytes::Bytes::from(buf).slice(range), None).unwrap();
        let expect = if tz == "-" {
            None
        } else {
            Some(tz.to_string())
        };
        assert_eq!(iter.tz, expect);
        let exif: Exif = iter.into();
        let value = exif.get(crate::ExifTag::DateTimeOriginal);
        if time == "-" {
            assert!(value.is_none());
        } else {
            let value = value.unwrap();
            assert_eq!(value.to_string(), time);
        }
    }

    #[test]
    fn ifd_index_constants() {
        use crate::IfdIndex;
        assert_eq!(IfdIndex::MAIN.get(), 0);
        assert_eq!(IfdIndex::THUMBNAIL.get(), 1);
    }

    #[test]
    fn ifd_index_roundtrip_via_new_and_get() {
        use crate::IfdIndex;
        for raw in [0, 1, 2, 3, 7, 99] {
            assert_eq!(IfdIndex::new(raw).get(), raw);
        }
    }

    #[test]
    fn ifd_index_equality_and_hash() {
        use crate::IfdIndex;
        use std::collections::HashSet;
        let mut set: HashSet<IfdIndex> = HashSet::new();
        set.insert(IfdIndex::MAIN);
        set.insert(IfdIndex::new(0)); // duplicate
        set.insert(IfdIndex::THUMBNAIL);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn ifd_index_display_format() {
        use crate::IfdIndex;
        assert_eq!(format!("{}", IfdIndex::MAIN), "ifd0");
        assert_eq!(format!("{}", IfdIndex::new(7)), "ifd7");
    }

    #[test]
    fn tag_or_code_for_known_tag_resolves_to_tag_variant() {
        use crate::{ExifTag, TagOrCode};
        let t: TagOrCode = ExifTag::Make.code().into();
        assert_eq!(t, TagOrCode::Tag(ExifTag::Make));
        assert_eq!(t.code(), ExifTag::Make.code());
    }

    #[test]
    fn tag_or_code_for_unknown_tag_resolves_to_unknown_variant() {
        use crate::TagOrCode;
        let t: TagOrCode = 0xffff_u16.into();
        assert_eq!(t, TagOrCode::Unknown(0xffff));
        assert_eq!(t.code(), 0xffff);
    }

    #[test]
    fn exif_entry_pub_fields_construct_and_destructure() {
        use crate::{EntryValue, ExifEntry, ExifTag, IfdIndex, TagOrCode};
        let val = EntryValue::Text("vivo X90 Pro+".into());
        let e = ExifEntry {
            ifd: IfdIndex::MAIN,
            tag: TagOrCode::Tag(ExifTag::Model),
            value: &val,
        };
        // Pub fields: just match.
        let ExifEntry { ifd, tag, value } = e;
        assert_eq!(ifd, IfdIndex::MAIN);
        assert_eq!(tag.code(), ExifTag::Model.code());
        assert!(matches!(value, EntryValue::Text(_)));
        // Copy works because EntryValue is borrowed.
        let _e2 = e;
        let _e3 = e;
    }
}

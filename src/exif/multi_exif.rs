use std::collections::HashSet;

use crate::{partial_vec::PartialVec, values::EntryValue, ExifTag};

use super::{
    exif_iter::{input_into_iter, ExifIter, ParsedExifEntry},
    TiffHeader,
};

/// Strategy for handling duplicate tags across multiple TIFF blocks
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum DuplicateStrategy {
    /// Ignore duplicate tags (skip subsequent occurrences)
    IgnoreDuplicates,
    /// Allow duplicate tags (emit all occurrences)
    AllowDuplicates,
}

/// A TIFF data source for lazy loading
struct TiffDataSource {
    /// Block identifier
    block_id: String,
    /// Data loader function (lazy loading)
    data_loader: Box<dyn Fn() -> crate::Result<PartialVec> + Send + Sync>,
    /// TIFF header information (optional, if known)
    header: Option<TiffHeader>,
    /// Whether loading has been attempted
    load_attempted: bool,
}

/// An iterator for multiple TIFF/Exif data blocks with lazy loading support.
///
/// This is designed for files like Canon CR3 that contain multiple TIFF data
/// blocks (e.g., in CMT1/CMT2 boxes) that need to be processed together.
pub struct MultiExifIter {
    /// TIFF data sources (lazy loading)
    tiff_sources: Vec<TiffDataSource>,
    /// Current TIFF block index being iterated
    current_block_index: usize,
    /// Currently loaded ExifIter (created only when needed)
    current_iter: Option<ExifIter>,
    /// Tag handling strategy for duplicates
    duplicate_strategy: DuplicateStrategy,
    /// Set of encountered tags for duplicate detection (ifd_index, tag_code)
    encountered_tags: HashSet<(usize, u16)>,
}

/// A parsed EXIF entry from multiple TIFF blocks
pub struct MultiExifParsedEntry {
    /// The original parsed entry
    inner: ParsedExifEntry,
    /// Source TIFF block identifier
    source_block_id: String,
    /// Source TIFF block index
    source_block_index: usize,
}

#[allow(dead_code)]
impl MultiExifParsedEntry {
    /// Get the source TIFF block identifier
    pub fn source_block_id(&self) -> &str {
        &self.source_block_id
    }

    /// Get the source TIFF block index
    pub fn source_block_index(&self) -> usize {
        self.source_block_index
    }

    /// Get the IFD index value where this entry is located.
    /// - 0: ifd0 (main image)
    /// - 1: ifd1 (thumbnail)
    pub fn ifd_index(&self) -> usize {
        self.inner.ifd_index()
    }

    /// Get recognized Exif tag of this entry, maybe return `None` if the tag
    /// is unrecognized.
    pub fn tag(&self) -> Option<ExifTag> {
        self.inner.tag()
    }

    /// Get the raw tag code of this entry.
    pub fn tag_code(&self) -> u16 {
        self.inner.tag_code()
    }

    /// Returns true if there is an `EntryValue` in self.
    pub fn has_value(&self) -> bool {
        self.inner.has_value()
    }

    /// Get the parsed entry value of this entry.
    pub fn get_value(&self) -> Option<&EntryValue> {
        self.inner.get_value()
    }

    /// Takes out the parsed entry value of this entry.
    ///
    /// **Note**: This method can only be called once! Once it has been called,
    /// calling it again always returns `None`.
    pub fn take_value(&mut self) -> Option<EntryValue> {
        self.inner.take_value()
    }

    /// Get the parsed result of this entry.
    pub fn get_result(&self) -> Result<&EntryValue, &super::exif_iter::EntryError> {
        self.inner.get_result()
    }

    /// Takes out the parsed result of this entry.
    ///
    /// **Note**: This method can ONLY be called once! If you call it twice, it
    /// will **panic** directly!
    pub fn take_result(&mut self) -> Result<EntryValue, super::exif_iter::EntryError> {
        self.inner.take_result()
    }
}

impl std::fmt::Debug for MultiExifParsedEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiParsedExifEntry")
            .field("source_block_id", &self.source_block_id)
            .field("source_block_index", &self.source_block_index)
            .field("inner", &self.inner)
            .finish()
    }
}

#[allow(dead_code)]
impl MultiExifIter {
    /// Create a new multi-TIFF iterator with the specified duplicate handling strategy
    pub fn new(strategy: DuplicateStrategy) -> Self {
        Self {
            tiff_sources: Vec::new(),
            current_block_index: 0,
            current_iter: None,
            duplicate_strategy: strategy,
            encountered_tags: HashSet::new(),
        }
    }

    /// Add a TIFF data source with lazy loading
    ///
    /// # Arguments
    /// * `block_id` - Identifier for this TIFF block (e.g., "CMT1", "CMT2")
    /// * `loader` - Function that returns TIFF data when called
    /// * `header` - Optional TIFF header if already parsed
    pub fn add_tiff_source<F>(&mut self, block_id: String, loader: F, header: Option<TiffHeader>)
    where
        F: Fn() -> crate::Result<PartialVec> + Send + Sync + 'static,
    {
        self.tiff_sources.push(TiffDataSource {
            block_id,
            data_loader: Box::new(loader),
            header,
            load_attempted: false,
        });
    }

    /// Add already available TIFF data (immediately usable)
    ///
    /// # Arguments
    /// * `block_id` - Identifier for this TIFF block
    /// * `data` - TIFF data
    /// * `header` - Optional TIFF header if already parsed
    pub fn add_tiff_data(
        &mut self,
        block_id: String,
        data: PartialVec,
        header: Option<TiffHeader>,
    ) {
        self.add_tiff_source(block_id, move || Ok(data.clone()), header);
    }

    /// Get the number of TIFF blocks
    pub fn block_count(&self) -> usize {
        self.tiff_sources.len()
    }

    /// Get current block information (block_id, block_index)
    pub fn current_block_info(&self) -> Option<(&str, usize)> {
        if self.current_block_index < self.tiff_sources.len() {
            Some((
                &self.tiff_sources[self.current_block_index].block_id,
                self.current_block_index,
            ))
        } else {
            None
        }
    }

    /// Reset the iterator to the beginning
    pub fn rewind(&mut self) {
        self.current_block_index = 0;
        self.current_iter = None;
        self.encountered_tags.clear();

        // Reset load_attempted flags
        for source in &mut self.tiff_sources {
            source.load_attempted = false;
        }
    }

    /// Load the next TIFF block and create an ExifIter for it
    fn load_next_block(&mut self) -> crate::Result<ExifIter> {
        if self.current_block_index >= self.tiff_sources.len() {
            return Err("No more TIFF blocks to load".into());
        }

        let source = &mut self.tiff_sources[self.current_block_index];
        if source.load_attempted {
            return Err("Block already failed to load".into());
        }

        source.load_attempted = true;
        let data = (source.data_loader)()?;
        tracing::debug!(
            block_id = source.block_id,
            block_index = self.current_block_index,
            data_len = data.len(),
            "Loading TIFF block"
        );

        match input_into_iter(data, source.header.clone()) {
            Ok(iter) => {
                tracing::debug!(
                    "Successfully created ExifIter for block {}",
                    source.block_id
                );
                Ok(iter)
            }
            Err(e) => {
                tracing::warn!(
                    block_id = source.block_id,
                    error = %e,
                    "Failed to create ExifIter for block"
                );
                Err(e)
            }
        }
    }
}

impl Clone for MultiExifIter {
    fn clone(&self) -> Self {
        // Clone the iterator and reset to beginning
        Self {
            tiff_sources: self
                .tiff_sources
                .iter()
                .map(|source| {
                    TiffDataSource {
                        block_id: source.block_id.clone(),
                        // Note: We can't clone the Box<dyn Fn>, so we create a new one that always fails
                        // This is a limitation - cloned iterators won't work with lazy loading
                        data_loader: Box::new(|| Err("Cannot clone lazy loader".into())),
                        header: source.header.clone(),
                        load_attempted: false,
                    }
                })
                .collect(),
            current_block_index: 0,
            current_iter: None,
            duplicate_strategy: self.duplicate_strategy,
            encountered_tags: HashSet::new(),
        }
    }
}

impl Iterator for MultiExifIter {
    type Item = MultiExifParsedEntry;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // If current block iterator exists, try to get next entry
            if let Some(ref mut current_iter) = self.current_iter {
                for entry in current_iter.by_ref() {
                    // Check duplicate strategy
                    let tag_key = (entry.ifd_index(), entry.tag_code());
                    let should_include = match self.duplicate_strategy {
                        DuplicateStrategy::IgnoreDuplicates => {
                            if self.encountered_tags.contains(&tag_key) {
                                false // Skip duplicate tag
                            } else {
                                self.encountered_tags.insert(tag_key);
                                true
                            }
                        }
                        DuplicateStrategy::AllowDuplicates => {
                            // Always allow, just record the tag
                            self.encountered_tags.insert(tag_key);
                            true
                        }
                    };

                    if should_include {
                        return Some(MultiExifParsedEntry {
                            inner: entry,
                            source_block_id: self.tiff_sources[self.current_block_index]
                                .block_id
                                .clone(),
                            source_block_index: self.current_block_index,
                        });
                    }
                    // If tag should be skipped, continue to next entry
                }

                // Current iterator is exhausted, move to next block
                self.current_iter = None;
                self.current_block_index += 1;
            }

            // No current iterator, need to load the next block
            if self.current_block_index >= self.tiff_sources.len() {
                return None; // All blocks have been iterated
            }

            // Lazy load the current TIFF block
            match self.load_next_block() {
                Ok(iter) => {
                    tracing::debug!(
                        block_index = self.current_block_index,
                        block_id = self.tiff_sources[self.current_block_index].block_id,
                        "Successfully loaded TIFF block"
                    );
                    self.current_iter = Some(iter);
                    // Continue the loop to get entries from the new block
                }
                Err(e) => {
                    tracing::warn!(
                        block_index = self.current_block_index,
                        block_id = self.tiff_sources[self.current_block_index].block_id,
                        error = %e,
                        "Failed to load TIFF block, skipping"
                    );
                    // Move to next block
                    self.current_block_index += 1;
                }
            }
        }
    }
}

impl std::fmt::Debug for MultiExifIter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiExifIter")
            .field("block_count", &self.tiff_sources.len())
            .field("current_block_index", &self.current_block_index)
            .field("duplicate_strategy", &self.duplicate_strategy)
            .field("encountered_tags_count", &self.encountered_tags.len())
            .field("has_current_iter", &self.current_iter.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::partial_vec::PartialVec;

    #[test]
    fn test_multi_exif_iter_creation() {
        let iter = MultiExifIter::new(DuplicateStrategy::IgnoreDuplicates);
        assert_eq!(iter.block_count(), 0);
        assert!(iter.current_block_info().is_none());
    }

    #[test]
    fn test_add_tiff_data() {
        let mut iter = MultiExifIter::new(DuplicateStrategy::IgnoreDuplicates);
        let data = PartialVec::from(vec![0u8; 100]);

        iter.add_tiff_data("test_block".to_string(), data, None);
        assert_eq!(iter.block_count(), 1);
    }

    #[test]
    fn test_add_tiff_source() {
        let mut iter = MultiExifIter::new(DuplicateStrategy::IgnoreDuplicates);

        iter.add_tiff_source(
            "test_block".to_string(),
            || Ok(PartialVec::from(vec![0u8; 100])),
            None,
        );

        assert_eq!(iter.block_count(), 1);
    }

    #[test]
    fn test_multi_tiff_with_real_data() {
        use super::super::extract_exif_with_mime;
        use crate::file::MimeImage;
        use crate::slice::SubsliceRange;
        use crate::testkit::read_sample;

        let mut iter = MultiExifIter::new(DuplicateStrategy::IgnoreDuplicates);

        // Use real TIFF data from test files
        let tiff_buf = read_sample("tif.tif").unwrap();
        let (tiff_data, _) = extract_exif_with_mime(MimeImage::Tiff, &tiff_buf, None).unwrap();

        if let Some(exif_range) = tiff_data.and_then(|x| tiff_buf.subslice_in_range(x)) {
            let exif_data = &tiff_buf[exif_range];

            // Add the same TIFF data twice as different blocks
            iter.add_tiff_data(
                "CMT1".to_string(),
                PartialVec::from(exif_data.to_vec()),
                None,
            );
            iter.add_tiff_data(
                "CMT2".to_string(),
                PartialVec::from(exif_data.to_vec()),
                None,
            );

            assert_eq!(iter.block_count(), 2);

            let mut entries = Vec::new();
            for entry in &mut iter {
                println!(
                    "Real data test - Got entry: block={}, tag={:04x}",
                    entry.source_block_id(),
                    entry.tag_code()
                );
                assert_eq!(entry.source_block_id, "CMT1");
                entries.push(entry);
            }

            println!("Real data test - Total entries: {}", entries.len());
            // With IgnoreDuplicates strategy, we should get entries only from the first block
            assert!(!entries.is_empty(), "Should have at least some entries");
        } else {
            panic!("Failed to extract TIFF data from test file");
        }
    }

    #[test]
    fn test_duplicate_strategies() {
        let ignore_iter = MultiExifIter::new(DuplicateStrategy::IgnoreDuplicates);
        let allow_iter = MultiExifIter::new(DuplicateStrategy::AllowDuplicates);

        assert!(matches!(
            ignore_iter.duplicate_strategy,
            DuplicateStrategy::IgnoreDuplicates
        ));
        assert!(matches!(
            allow_iter.duplicate_strategy,
            DuplicateStrategy::AllowDuplicates
        ));
    }

    #[test]
    fn test_multi_tiff_with_allow_duplicates_strategy() {
        use super::super::extract_exif_with_mime;
        use crate::file::MimeImage;
        use crate::slice::SubsliceRange;
        use crate::testkit::read_sample;

        let mut iter = MultiExifIter::new(DuplicateStrategy::AllowDuplicates);

        // Use real TIFF data from test files
        let tiff_buf = read_sample("tif.tif").unwrap();
        let (tiff_data, _) = extract_exif_with_mime(MimeImage::Tiff, &tiff_buf, None).unwrap();

        if let Some(exif_range) = tiff_data.and_then(|x| tiff_buf.subslice_in_range(x)) {
            let exif_data = &tiff_buf[exif_range];

            // Add the same TIFF data twice as different blocks
            iter.add_tiff_data(
                "CMT1".to_string(),
                PartialVec::from(exif_data.to_vec()),
                None,
            );
            iter.add_tiff_data(
                "CMT2".to_string(),
                PartialVec::from(exif_data.to_vec()),
                None,
            );

            assert_eq!(iter.block_count(), 2);

            let mut entries = HashMap::new();
            for entry in &mut iter {
                println!(
                    "Overwrite test - Got entry: block={}, tag={:04x}",
                    entry.source_block_id(),
                    entry.tag_code()
                );
                entries.insert((entry.ifd_index(), entry.tag_code()), entry);
            }

            println!("Allow duplicates test - Total entries: {}", entries.len());
            // With AllowDuplicates strategy, we should get entries from both blocks
            assert!(!entries.is_empty(), "Should have at least some entries");

            let block_ids: std::collections::HashSet<_> =
                entries.iter().map(|e| e.1.source_block_id()).collect();
            assert!(
                !block_ids.is_empty(),
                "Should have entries from at least one block"
            );
            for id in block_ids {
                assert_eq!(id, "CMT2");
            }
        } else {
            panic!("Failed to extract TIFF data from test file");
        }
    }

    #[test]
    fn test_lazy_loading_with_error() {
        use crate::slice::SubsliceRange;

        let mut iter = MultiExifIter::new(DuplicateStrategy::IgnoreDuplicates);

        // Add a source that will fail to load
        iter.add_tiff_source(
            "failing_block".to_string(),
            || Err("Simulated loading error".into()),
            None,
        );

        // Add a successful source using real TIFF data
        let tiff_buf = crate::testkit::read_sample("tif.tif").unwrap();
        let (tiff_data, _) =
            super::super::extract_exif_with_mime(crate::file::MimeImage::Tiff, &tiff_buf, None)
                .unwrap();

        if let Some(exif_range) = tiff_data.and_then(|x| tiff_buf.subslice_in_range(x)) {
            let exif_data: &[u8] = &tiff_buf[exif_range];
            iter.add_tiff_data(
                "good_block".to_string(),
                PartialVec::from(exif_data.to_vec()),
                None,
            );

            let mut entries = Vec::new();
            for entry in &mut iter {
                println!(
                    "Error test - Got entry: block={}, tag={:04x}",
                    entry.source_block_id(),
                    entry.tag_code()
                );
                entries.push(entry);
            }

            // Should only get entries from the successful block
            assert!(
                !entries.is_empty(),
                "Should have at least some entries from the good block"
            );
            for entry in &entries {
                assert_eq!(entry.source_block_id(), "good_block");
            }
        }
    }
}

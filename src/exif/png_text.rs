//! PNG `tEXt` chunks as Latin-1-decoded key/value pairs.
//!
//! See [`PngTextChunks`] for accessors. Used as the payload of
//! [`crate::ImageFormatMetadata::Png`].

/// PNG `tEXt` chunks, decoded as Latin-1 `(key, value)` pairs in file
/// order.
///
/// Duplicate keys are preserved (PNG spec permits multiple `tEXt`
/// chunks with the same keyword). Encoding is strict Latin-1 per spec
/// — no UTF-8 sniffing.
///
/// **Note**: when a PNG carries EXIF inside a `Raw profile type exif` /
/// `Raw profile type APP1` text chunk (legacy ImageMagick / Photoshop
/// pattern), the EXIF entries are merged into the `Exif` (under
/// `ImageMetadata.exif`) transparently; the original text chunk is
/// also visible here.
///
/// Forward-compatible: future iTXt / zTXt support can extend
/// `PngTextChunks` non-breakingly.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PngTextChunks {
    pub(crate) entries: Vec<(String, String)>,
}

impl PngTextChunks {
    /// First value whose key matches exactly, or `None`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// All values whose key matches exactly, in file order.
    pub fn get_all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.entries
            .iter()
            .filter(move |(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// All `(key, value)` pairs in file order, including duplicates.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Number of `(key, value)` pairs (counts duplicates).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if no `tEXt` entries are present.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PngTextChunks {
        PngTextChunks {
            entries: vec![
                ("Title".into(), "Hello".into()),
                ("Author".into(), "Alice".into()),
                ("Comment".into(), "first comment".into()),
                ("Comment".into(), "second comment".into()),
            ],
        }
    }

    #[test]
    fn get_returns_first_match() {
        let t = fixture();
        assert_eq!(t.get("Title"), Some("Hello"));
        assert_eq!(t.get("Comment"), Some("first comment"));
        assert_eq!(t.get("nonexistent"), None);
    }

    #[test]
    fn get_all_returns_all_in_order() {
        let t = fixture();
        let comments: Vec<&str> = t.get_all("Comment").collect();
        assert_eq!(comments, vec!["first comment", "second comment"]);
        let titles: Vec<&str> = t.get_all("Title").collect();
        assert_eq!(titles, vec!["Hello"]);
        let nothing: Vec<&str> = t.get_all("nonexistent").collect();
        assert!(nothing.is_empty());
    }

    #[test]
    fn iter_in_file_order_with_duplicates() {
        let t = fixture();
        let pairs: Vec<(&str, &str)> = t.iter().collect();
        assert_eq!(pairs.len(), 4);
        assert_eq!(pairs[2], ("Comment", "first comment"));
        assert_eq!(pairs[3], ("Comment", "second comment"));
    }

    #[test]
    fn len_and_is_empty() {
        let t = fixture();
        assert_eq!(t.len(), 4);
        assert!(!t.is_empty());

        let empty = PngTextChunks::default();
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
    }
}

# PNG P3 — Public types: `ImageMetadata`, `ImageFormatMetadata`, `PngTextChunks`, `ExifRepr`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose the public types that `parse_image_metadata` will return: `ImageMetadata<E: ExifRepr = Exif>`, `ImageFormatMetadata` (`#[non_exhaustive]`), `PngTextChunks`, and the sealed `ExifRepr` trait. PNG dispatch in `MediaParser` is still stubbed at this phase — types ship with no caller. P4 wires them up.

**Architecture:** `PngTextChunks` is an opaque wrapper around `Vec<(String, String)>` exposing `get` / `get_all` / `iter` / `len` / `is_empty`. `ImageFormatMetadata` is a `#[non_exhaustive]` enum with one variant `Png(PngTextChunks)`. `ImageMetadata<E: ExifRepr = Exif>` is a struct holding `Option<E>` and `Option<ImageFormatMetadata>`. `ExifRepr` is a sealed trait implemented by `Exif` and `ExifIter`. `From<ImageMetadata<ExifIter>> for ImageMetadata<Exif>` enables eager conversion.

**Tech Stack:** Generic types with default type parameter, sealed trait pattern.

---

## File Structure

| File | Change |
|---|---|
| `src/exif/png_text.rs` | NEW — `PngTextChunks` type + accessors. |
| `src/image_metadata.rs` | NEW — `ImageMetadata<E>`, `ImageFormatMetadata`, `ExifRepr` sealed trait, `From<ImageMetadata<ExifIter>>` impl. |
| `src/exif.rs` | Add `pub mod png_text;` declaration to expose the inner module. |
| `src/lib.rs` | Add `mod image_metadata;` declaration; add re-exports of public types. |

---

## Task 3.1: Create `PngTextChunks` type

**Files:**
- Create: `src/exif/png_text.rs`
- Modify: `src/exif.rs` (add module declaration)

- [ ] **Step 1: Create the module**

Create `src/exif/png_text.rs`:

```rust
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
```

- [ ] **Step 2: Add module declaration in `src/exif.rs`**

Edit `src/exif.rs` — find the `mod` declarations near the top (around lines 21-25) and add:

```rust
mod exif_exif;
mod exif_iter;
pub mod gps;
pub mod png_text;  // NEW
mod tags;
mod travel;
```

Note: `pub mod` because `PngTextChunks` is publicly re-exported via `lib.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test --all-features png_text`
Expected: 4 new tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/exif/png_text.rs src/exif.rs
git commit -m "$(cat <<'EOF'
feat: add PngTextChunks public type

Opaque wrapper around Vec<(String, String)> exposing get / get_all /
iter / len / is_empty. Latin-1 decoded key/value pairs in PNG file
order, duplicates preserved.

Will be the payload of ImageFormatMetadata::Png in the next commit.
EOF
)"
```

---

## Task 3.2: Create `ImageMetadata<E>`, `ImageFormatMetadata`, `ExifRepr`

**Files:**
- Create: `src/image_metadata.rs`
- Modify: `src/lib.rs` (add module declaration)

- [ ] **Step 1: Create the module**

Create `src/image_metadata.rs`:

```rust
//! Structured image-metadata view returned by [`crate::MediaParser::parse_image_metadata`].
//!
//! See [`ImageMetadata`].

use crate::exif::png_text::PngTextChunks;

mod sealed {
    pub trait Sealed {}
}

/// Marker trait for the two valid "EXIF representations" held by
/// [`ImageMetadata`]: [`Exif`](crate::Exif) (eager) and [`ExifIter`](crate::ExifIter)
/// (lazy). Sealed — users cannot add their own implementations.
pub trait ExifRepr: sealed::Sealed {}

impl sealed::Sealed for crate::Exif {}
impl ExifRepr for crate::Exif {}

impl sealed::Sealed for crate::ExifIter {}
impl ExifRepr for crate::ExifIter {}

/// Structured image-metadata view: EXIF (if any) plus format-specific
/// metadata (if any).
///
/// Default `E = Exif` — eager EXIF representation. The
/// [`MediaParser::parse_image_metadata`](crate::MediaParser::parse_image_metadata)
/// method returns `ImageMetadata<ExifIter>` (lazy); convert to the
/// default eager form via `.into()` when desired.
///
/// **Forward-compat note**: this struct is shaped to be reused
/// unchanged by a future v4 redesign of the [`Metadata`](crate::Metadata)
/// enum (`Metadata::Image(ImageMetadata)`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageMetadata<E: ExifRepr = crate::Exif> {
    /// EXIF tags found in the source image, if any. For PNG, this
    /// includes legacy `Raw profile type {exif,APP1}` hex-encoded
    /// EXIF transparently merged.
    pub exif: Option<E>,

    /// Format-specific metadata that does not fit the EXIF/IFD
    /// abstraction (e.g. PNG `tEXt` chunks).
    pub format: Option<ImageFormatMetadata>,
}

impl From<ImageMetadata<crate::ExifIter>> for ImageMetadata<crate::Exif> {
    fn from(m: ImageMetadata<crate::ExifIter>) -> Self {
        ImageMetadata {
            exif: m.exif.map(Into::into),
            format: m.format,
        }
    }
}

/// Format-specific image metadata. One variant per format that has
/// metadata not expressible as EXIF tags.
///
/// Marked `#[non_exhaustive]` so future formats can be added
/// (`Gif(...)`, `Webp(...)` etc.) without breaking exhaustive `match`
/// statements in user code.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ImageFormatMetadata {
    /// PNG `tEXt` chunks. Latin-1 key/value pairs in file order.
    Png(PngTextChunks),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let m: ImageMetadata = ImageMetadata::default();
        assert!(m.exif.is_none());
        assert!(m.format.is_none());
    }

    #[test]
    fn generic_explicit_lazy_form() {
        // ImageMetadata<ExifIter> compiles and is constructible.
        let m: ImageMetadata<crate::ExifIter> = ImageMetadata {
            exif: None,
            format: None,
        };
        assert!(m.exif.is_none());
    }

    #[test]
    fn from_lazy_to_eager_compiles() {
        // We can't easily construct an ExifIter here; just verify the
        // type-level conversion exists by going through Default.
        let lazy: ImageMetadata<crate::ExifIter> = ImageMetadata::default();
        let _eager: ImageMetadata<crate::Exif> = lazy.into();
    }

    #[test]
    fn format_metadata_png_variant() {
        let png_text = PngTextChunks::default();
        let fm = ImageFormatMetadata::Png(png_text);
        match fm {
            ImageFormatMetadata::Png(t) => assert!(t.is_empty()),
        }
    }
}
```

- [ ] **Step 2: Add module declaration**

Edit `src/lib.rs` — find the `mod` declarations and add:

```rust
mod file;
mod heif;
mod image_metadata;  // NEW
mod jpeg;
```

(Sorted alphabetically into the existing list.)

- [ ] **Step 3: Add re-exports**

Edit `src/lib.rs` — find the `pub use` lines (around line 134-146) and add:

```rust
pub use exif::png_text::PngTextChunks;
pub use image_metadata::{ExifRepr, ImageFormatMetadata, ImageMetadata};
```

(Place them with the other `pub use` lines.)

- [ ] **Step 4: Run tests**

Run: `cargo test --all-features image_metadata`
Expected: 4 new tests pass.

- [ ] **Step 5: Verify the public types compile under serde feature**

Run: `cargo build --all-features --features serde`
Expected: no errors. (`PngTextChunks` and `ImageMetadata` derive `Serialize`/`Deserialize` under feature; need to verify no conflicts.)

If `cargo build --all-features` already includes serde (it should), the previous step covers it.

- [ ] **Step 6: Commit**

```bash
git add src/image_metadata.rs src/lib.rs
git commit -m "$(cat <<'EOF'
feat: add ImageMetadata<E>, ImageFormatMetadata, ExifRepr

Public types for the new parse_image_metadata entry point:
- ImageMetadata<E: ExifRepr = Exif> — { exif, format } struct
- ImageFormatMetadata — #[non_exhaustive] enum with Png variant
- ExifRepr — sealed marker trait, impl'd by Exif and ExifIter
- From<ImageMetadata<ExifIter>> for ImageMetadata<Exif> — eager
  conversion

PNG dispatch in MediaParser is still stubbed; the type can be
constructed but the parse_image_metadata method that returns it
lands in phase 4.
EOF
)"
```

---

## Task 3.3: Verify type composition with a doctest example

**Files:**
- Modify: `src/image_metadata.rs` (add module-level docstring with examples)

- [ ] **Step 1: Add a runnable doctest at the top of the file**

Edit `src/image_metadata.rs` — replace the module-level comment with a richer one including a doctest:

```rust
//! Structured image-metadata view returned by
//! [`crate::MediaParser::parse_image_metadata`].
//!
//! See [`ImageMetadata`].
//!
//! # Example: lazy and eager forms compose
//!
//! ```rust
//! use nom_exif::{ImageMetadata, ImageFormatMetadata, ExifIter, Exif};
//!
//! // Default form (eager — type parameter defaults to Exif).
//! let _eager_default: ImageMetadata = ImageMetadata::default();
//!
//! // Explicit lazy form.
//! let lazy: ImageMetadata<ExifIter> = ImageMetadata {
//!     exif: None,
//!     format: None,
//! };
//!
//! // Lazy → eager conversion via From.
//! let _eager: ImageMetadata<Exif> = lazy.into();
//! ```
```

(Keep the existing internal sealed module + impls below.)

- [ ] **Step 2: Run doctest**

Run: `cargo test --all-features --doc image_metadata`
Expected: doctest compiles and passes.

- [ ] **Step 3: Commit**

```bash
git add src/image_metadata.rs
git commit -m "$(cat <<'EOF'
docs: add doctest demonstrating ImageMetadata generic forms

Shows the default (eager) form, explicit lazy form, and From
conversion between them. Verifies type-level composition compiles
cleanly without users needing to spell out the sealed trait.
EOF
)"
```

---

## Task 3.4: Final verification of phase 3

- [ ] **Step 1: Full test suite green**

Run: `cargo test --all-features`
Expected: green; new png_text and image_metadata tests pass.

- [ ] **Step 2: Doctests pass**

Run: `cargo test --all-features --doc`
Expected: all doctests (including the new ones) pass.

- [ ] **Step 3: Format clean**

Run: `cargo fmt --check`
Expected: clean.

- [ ] **Step 4: Public types appear in `cargo doc`**

Run: `cargo doc --no-deps --all-features 2>&1 | grep -i warning | head`
Expected: no new warnings. Visit `target/doc/nom_exif/index.html` (optional) to confirm `ImageMetadata`, `ImageFormatMetadata`, `PngTextChunks`, `ExifRepr` are documented.

- [ ] **Step 5: Self-check exit criterion**

> `PngTextChunks` exposes get / get_all / iter / len / is_empty. ✓
> `ImageMetadata<E: ExifRepr = Exif>` with both fields. ✓
> `ImageFormatMetadata` (`#[non_exhaustive]`) with `Png` variant. ✓
> `ExifRepr` sealed trait. ✓
> `From<ImageMetadata<ExifIter>> for ImageMetadata<Exif>` impl. ✓
> Exports added to `lib.rs`. ✓
> Unit tests cover accessors + generic instantiation + From conversion. ✓
> PNG dispatch still stubbed. ✓
> `cargo test --all-features` green. ✓

Phase 3 complete. Proceed to P4.

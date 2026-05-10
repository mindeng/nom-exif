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
#[derive(Debug, Clone)]
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

impl<E: ExifRepr> Default for ImageMetadata<E> {
    fn default() -> Self {
        ImageMetadata {
            exif: None,
            format: None,
        }
    }
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

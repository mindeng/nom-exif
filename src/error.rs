use std::fmt::{Debug, Display};
use thiserror::Error;

/// Top-level error returned by `read_exif`, `MediaParser::parse_*`,
/// `MediaSource::open`, and any other public function that touches a file.
///
/// `#[non_exhaustive]` — downstream code MUST use a `_ =>` fallback in `match`
/// to remain compatible with future variants.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unsupported media format")]
    UnsupportedFormat,

    #[error("no exif data found in this file")]
    ExifNotFound,

    #[error("no track info found in this file")]
    TrackNotFound,

    /// Data was recognized as the target format but its inner structure is broken.
    #[error("malformed {kind}: {message}")]
    Malformed {
        kind: MalformedKind,
        message: String,
    },

    /// Parsing needed more bytes but the stream ended.
    #[error("unexpected end of input while parsing {context}")]
    UnexpectedEof { context: &'static str },
}

#[derive(Debug, Error)]
pub(crate) enum ParsedError {
    #[error("no enough bytes")]
    NoEnoughBytes,

    #[error("io error: {0}")]
    IOError(std::io::Error),

    #[error("{0}")]
    Failed(String),
}

/// Due to the fact that metadata in MOV files is typically located at the end
/// of the file, conventional parsing methods would require reading a
/// significant amount of unnecessary data during the parsing process. This
/// would impact the performance of the parsing program and consume more memory.
///
/// To address this issue, we have defined an `Error::Skip` enumeration type to
/// inform the caller that certain bytes in the parsing process are not required
/// and can be skipped directly. The specific method of skipping can be
/// determined by the caller based on the situation. For example:
///
/// - For files, you can quickly skip using a `Seek` operation.
///
/// - For network byte streams, you may need to skip these bytes through read
///   operations, or preferably, by designing an appropriate network protocol for
///   skipping.
///
/// # [`ParsingError::ClearAndSkip`]
///
/// Please note that when the caller receives an `Error::Skip(n)` error, it
/// should be understood as follows:
///
/// - The parsing program has already consumed all available data and needs to
///   skip n bytes further.
///
/// - After skipping n bytes, it should continue to read subsequent data to fill
///   the buffer and use it as input for the parsing function.
///
/// - The next time the parsing function is called (usually within a loop), the
///   previously consumed data (including the skipped bytes) should be ignored,
///   and only the newly read data should be passed in.
///
/// # [`ParsingError::Need`]
///
/// Additionally, to simplify error handling, we have integrated
/// `nom::Err::Incomplete` error into `Error::Need`. This allows us to use the
/// same error type to notify the caller that we require more bytes to continue
/// parsing.
#[derive(Debug, Error)]
pub(crate) enum ParsingError {
    #[error("need more bytes: {0}")]
    Need(usize),

    #[error("clear and skip bytes: {0:?}")]
    ClearAndSkip(usize),

    #[error("{0}")]
    Failed(String),
}

#[derive(Debug, Error)]
pub(crate) struct ParsingErrorState {
    pub err: ParsingError,
    pub state: Option<ParsingState>,
}

impl ParsingErrorState {
    pub fn new(err: ParsingError, state: Option<ParsingState>) -> Self {
        Self { err, state }
    }
}

impl Display for ParsingErrorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(
            &format!(
                "ParsingError(err: {}, state: {})",
                self.err,
                self.state
                    .as_ref()
                    .map(|x| x.to_string())
                    .unwrap_or("None".to_string())
            ),
            f,
        )
    }
}

impl From<&str> for ParsingError {
    fn from(value: &str) -> Self {
        Self::Failed(value.to_string())
    }
}

impl From<std::io::Error> for ParsedError {
    fn from(value: std::io::Error) -> Self {
        Self::IOError(value)
    }
}

impl From<ParsedError> for crate::Error {
    fn from(value: ParsedError) -> Self {
        match value {
            ParsedError::NoEnoughBytes => Self::UnexpectedEof {
                context: "media stream",
            },
            ParsedError::IOError(e) => Self::Io(e),
            // Best-effort default: P3 will plumb the actual MalformedKind
            // through ParsedError so this fallback can go away.
            ParsedError::Failed(e) => Self::Malformed {
                kind: MalformedKind::IsoBmffBox,
                message: e,
            },
        }
    }
}

use crate::parser::ParsingState;

impl<T: Debug> From<nom::Err<nom::error::Error<T>>> for crate::Error {
    fn from(e: nom::Err<nom::error::Error<T>>) -> Self {
        convert_parse_error(e, "")
    }
}

pub(crate) fn convert_parse_error<T: Debug>(
    e: nom::Err<nom::error::Error<T>>,
    message: &str,
) -> Error {
    let s = match e {
        nom::Err::Incomplete(_) => format!("{e}; {message}"),
        nom::Err::Error(e) => format!("{}; {message}", e.code.description()),
        nom::Err::Failure(e) => format!("{}; {message}", e.code.description()),
    };
    Error::Malformed {
        kind: MalformedKind::TiffHeader,
        message: s,
    }
}

impl From<nom::Err<nom::error::Error<&[u8]>>> for ParsingError {
    fn from(e: nom::Err<nom::error::Error<&[u8]>>) -> Self {
        match e {
            nom::Err::Incomplete(needed) => match needed {
                nom::Needed::Unknown => ParsingError::Need(1),
                nom::Needed::Size(n) => ParsingError::Need(n.get()),
            },
            nom::Err::Failure(e) | nom::Err::Error(e) => {
                ParsingError::Failed(e.code.description().to_string())
            }
        }
    }
}

pub(crate) fn nom_error_to_parsing_error_with_state(
    e: nom::Err<nom::error::Error<&[u8]>>,
    state: Option<ParsingState>,
) -> ParsingErrorState {
    match e {
        nom::Err::Incomplete(needed) => match needed {
            nom::Needed::Unknown => ParsingErrorState::new(ParsingError::Need(1), state),
            nom::Needed::Size(n) => ParsingErrorState::new(ParsingError::Need(n.get()), state),
        },
        nom::Err::Failure(e) | nom::Err::Error(e) => ParsingErrorState::new(
            ParsingError::Failed(e.code.description().to_string()),
            state,
        ),
    }
}

/// Categorizes the *structural unit* that produced a `Error::Malformed`.
///
/// Variants describe the kind of bytes that failed to parse (a JPEG segment,
/// a TIFF header, an IFD entry, an ISO BMFF box, an EBML element), not the
/// outer file format. Format-specific context — e.g. "cr3:", "heif idat:" —
/// is conveyed in the accompanying `message` string.
///
/// This intentionally avoids a parallel format-level taxonomy (`Heif`,
/// `Cr3Container`, `Raf`, …): those families are all built on top of one of
/// the structural units listed here, so adding a row per format would create
/// non-orthogonal categories that overlap with the structural ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MalformedKind {
    JpegSegment,
    TiffHeader,
    IfdEntry,
    IsoBmffBox,
    EbmlElement,
}

impl std::fmt::Display for MalformedKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::JpegSegment => "jpeg segment",
            Self::TiffHeader => "tiff header",
            Self::IfdEntry => "ifd entry",
            Self::IsoBmffBox => "iso-bmff box",
            Self::EbmlElement => "ebml element",
        };
        f.write_str(s)
    }
}

/// Errors from conversions that are *orthogonal* to file parsing: parsing a tag
/// name from a string, narrowing an `IRational` into a `URational`, building a
/// `LatLng` from decimal degrees, parsing an ISO 6709 coordinate string.
///
/// Deliberately a peer type of `Error` — there is **no** `From<ConvertError>
/// for Error`. Downstream code that needs to combine file-level errors and
/// conversion errors should define its own wrapper enum (the standard
/// `thiserror` `#[from]` pattern). See spec §3.2.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum ConvertError {
    #[error("unknown ExifTag name: {0}")]
    UnknownTagName(String),

    #[error("invalid ISO 6709 coordinate: {0}")]
    InvalidIso6709(String),

    #[error("rational has negative value")]
    NegativeRational,

    #[error("decimal degrees out of range or non-finite: {0}")]
    InvalidDecimalDegrees(f64),
}

/// Errors that occur while decoding a single IFD entry.
///
/// Constructed internally during EXIF parsing; surfaces to downstream code
/// as the `Err` arm of [`crate::ExifIterEntry::result`],
/// or — when converted via `From<EntryError> for Error` — as
/// [`Error::Malformed`] with [`MalformedKind::IfdEntry`].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum EntryError {
    #[error("entry truncated: needed {needed} bytes, only {available} available")]
    Truncated { needed: usize, available: usize },

    #[error("invalid entry shape: format={format}, count={count}")]
    InvalidShape { format: u16, count: u32 },

    #[error("invalid value: {0}")]
    InvalidValue(&'static str),
}

impl From<EntryError> for Error {
    fn from(e: EntryError) -> Self {
        Error::Malformed {
            kind: MalformedKind::IfdEntry,
            message: e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_kind_is_copy_and_eq() {
        let a = MalformedKind::JpegSegment;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn malformed_kind_covers_all_structural_units() {
        for k in [
            MalformedKind::JpegSegment,
            MalformedKind::TiffHeader,
            MalformedKind::IfdEntry,
            MalformedKind::IsoBmffBox,
            MalformedKind::EbmlElement,
        ] {
            let _ = format!("{k:?}");
        }
    }

    #[test]
    fn convert_error_displays_each_variant() {
        let cases: &[(ConvertError, &str)] = &[
            (
                ConvertError::UnknownTagName("Foo".into()),
                "unknown ExifTag name: Foo",
            ),
            (
                ConvertError::InvalidIso6709("garbage".into()),
                "invalid ISO 6709 coordinate: garbage",
            ),
            (
                ConvertError::NegativeRational,
                "rational has negative value",
            ),
            (
                ConvertError::InvalidDecimalDegrees(f64::NAN),
                "decimal degrees out of range or non-finite: NaN",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), *expected);
        }
    }

    #[test]
    fn convert_error_does_not_convert_to_error() {
        // Compile-time intent: ConvertError must NOT be convertible into Error.
        // This is asserted documentally — there is no `impl From<ConvertError> for Error`.
        // We just verify both types compile here.
        let _ = ConvertError::NegativeRational;
        let _ = Error::UnsupportedFormat;
    }

    #[test]
    fn error_io_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "x");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn error_unsupported_format_displays() {
        assert_eq!(
            Error::UnsupportedFormat.to_string(),
            "unsupported media format"
        );
    }

    #[test]
    fn error_exif_not_found_displays() {
        assert_eq!(
            Error::ExifNotFound.to_string(),
            "no exif data found in this file"
        );
    }

    #[test]
    fn error_track_not_found_displays() {
        assert_eq!(
            Error::TrackNotFound.to_string(),
            "no track info found in this file"
        );
    }

    #[test]
    fn error_malformed_displays() {
        let e = Error::Malformed {
            kind: MalformedKind::JpegSegment,
            message: "bad SOI".into(),
        };
        assert_eq!(e.to_string(), "malformed jpeg segment: bad SOI");
    }

    #[test]
    fn error_unexpected_eof_displays() {
        let e = Error::UnexpectedEof {
            context: "tiff header",
        };
        assert_eq!(
            e.to_string(),
            "unexpected end of input while parsing tiff header"
        );
    }

    #[test]
    fn entry_error_truncated_displays() {
        let e = EntryError::Truncated {
            needed: 8,
            available: 4,
        };
        assert_eq!(
            e.to_string(),
            "entry truncated: needed 8 bytes, only 4 available"
        );
    }

    #[test]
    fn entry_error_invalid_shape_displays() {
        let e = EntryError::InvalidShape {
            format: 7,
            count: 1,
        };
        assert_eq!(e.to_string(), "invalid entry shape: format=7, count=1");
    }

    #[test]
    fn entry_error_invalid_value_displays() {
        let e = EntryError::InvalidValue("not utf-8");
        assert_eq!(e.to_string(), "invalid value: not utf-8");
    }

    #[test]
    fn entry_error_into_error_routes_to_malformed_ifd_entry() {
        let e = EntryError::Truncated {
            needed: 8,
            available: 4,
        };
        let err: Error = e.into();
        match err {
            Error::Malformed { kind, message } => {
                assert_eq!(kind, MalformedKind::IfdEntry);
                assert!(message.contains("entry truncated"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}

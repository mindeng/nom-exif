extern crate alloc;

use alloc::string::FromUtf8Error;
use std::io;

use thiserror::Error;

type FallbackError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Error)]
pub enum Error {
    /// `NotFound` has been deprecated, please don't check this error in your
    /// code (use "_" to ommit it if you are using match statement).
    ///
    /// The parser won't return this error anymore. It will be deleted in next
    /// major version.
    #[deprecated(since = "1.5.0", note = "won't return this error anymore")]
    #[error("exif/metadata not found")]
    NotFound,

    #[error("parse failed; {0}")]
    ParseFailed(FallbackError),

    #[error("invalid entry; {0}")]
    InvalidEntry(FallbackError),

    #[error("parsed entry result has been taken")]
    EntryHasBeenTaken,
}

impl From<io::Error> for Error {
    #[inline]
    fn from(value: io::Error) -> Self {
        Self::ParseFailed(value.into())
    }
}

impl From<String> for Error {
    #[inline]
    fn from(src: String) -> Self {
        Self::ParseFailed(src.into())
    }
}

impl From<&str> for Error {
    #[inline]
    fn from(src: &str) -> Self {
        src.to_string().into()
    }
}

impl From<FromUtf8Error> for Error {
    #[inline]
    fn from(value: FromUtf8Error) -> Self {
        Self::ParseFailed(value.into())
    }
}

impl From<nom::Err<nom::error::Error<&[u8]>>> for crate::Error {
    #[inline]
    fn from(e: nom::Err<nom::error::Error<&[u8]>>) -> Self {
        convert_parse_error(e, "")
    }
}

pub(crate) fn convert_parse_error(e: nom::Err<nom::error::Error<&[u8]>>, message: &str) -> Error {
    let s = match e {
        nom::Err::Incomplete(_) => format!("{e}; {message}"),
        nom::Err::Failure(e) | nom::Err::Error(e) => format!("{}; {message}", e.code.description()),
    };

    s.into()
}

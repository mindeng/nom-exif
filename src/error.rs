use std::{io, string::FromUtf8Error};
use thiserror::Error;

type FallbackError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("exif/metadata not found")]
    NotFound,

    #[error("parse failed; error: {0}")]
    ParseFailed(FallbackError),
}

pub use Error::*;

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        ParseFailed(value.into())
    }
}

impl From<String> for Error {
    fn from(src: String) -> Error {
        ParseFailed(src.into())
    }
}

impl From<&str> for Error {
    fn from(src: &str) -> Error {
        src.to_string().into()
    }
}

impl From<FromUtf8Error> for Error {
    fn from(value: FromUtf8Error) -> Self {
        ParseFailed(value.into())
    }
}

impl From<nom::Err<nom::error::Error<&[u8]>>> for crate::Error {
    fn from(e: nom::Err<nom::error::Error<&[u8]>>) -> Self {
        convert_parse_error(e, "")
    }
}

pub fn convert_parse_error(e: nom::Err<nom::error::Error<&[u8]>>, message: &str) -> Error {
    let s = match e {
        nom::Err::Incomplete(_) => format!("{e}; {message}"),
        nom::Err::Error(e) => format!("{}; {message}", e.code.description()),
        nom::Err::Failure(e) => format!("{}; {message}", e.code.description()),
    };

    s.into()
}

use std::{
    fmt::{Debug, Display},
    io::{self},
    string::FromUtf8Error,
};
use thiserror::Error;

type FallbackError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("parse failed: {0}")]
    ParseFailed(FallbackError),

    #[error("io error: {0}")]
    IOError(std::io::Error),

    /// If you encounter this error, please consider filing a bug on github
    #[error("unrecognized file format")]
    UnrecognizedFileFormat,
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
/// # [`ParsingError::Skip`]
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
            ParsedError::NoEnoughBytes => Self::ParseFailed(value.into()),
            ParsedError::IOError(e) => Self::IOError(e),
            ParsedError::Failed(e) => Self::ParseFailed(e.into()),
        }
    }
}

use Error::*;

use crate::parser::ParsingState;

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

    s.into()
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

// impl From<nom::Err<nom::error::Error<&[u8]>>> for ParsingErrorState {
//     fn from(e: nom::Err<nom::error::Error<&[u8]>>) -> Self {
//         match e {
//             nom::Err::Incomplete(needed) => match needed {
//                 nom::Needed::Unknown => ParsingErrorState::new(ParsingError::Need(1), None),
//                 nom::Needed::Size(n) => ParsingErrorState::new(ParsingError::Need(n.get()), None),
//             },
//             nom::Err::Failure(e) | nom::Err::Error(e) => {
//                 ParsingErrorState::new(ParsingError::Failed(e.code.description().to_string()), None)
//             }
//         }
//     }
// }

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

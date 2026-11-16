//! What can go wrong between the person's choice and the picture that comes back. Every variant
//! names a cause the person or the agent can act on, and none carries a key or a path.

use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    /// The catalogue has no entry with this id.
    UnknownProvider(String),
    /// The frame spec has no frame with this id.
    UnknownFrame(String),
    /// The entry has no address and the person gave none.
    NeedsEndpoint,
    /// The entry names no model and the person gave none.
    NeedsModel,
    /// The person gave something the provider does not take (a model, a seed).
    NotUsed(&'static str),
    /// The address cannot be used: the reason is a fixed phrase.
    InvalidEndpoint(&'static str),
    /// The entry needs the person's own key and none is stored.
    MissingKey,
    /// Keys are stored for this provider, but none for this address.
    KeyForAnotherAddress,
    /// A key that cannot travel in a request header.
    InvalidKey(&'static str),
    /// A picture that cannot be used: the reason is a fixed phrase.
    Picture(&'static str),
    /// The picture is not a multiple of the size the entry needs.
    SourceSize {
        width: u32,
        height: u32,
        multiple: u32,
    },
    /// The service refused the key.
    Unauthorized,
    /// The service asked to slow down.
    RateLimited,
    /// The service answered with an error. `reason` is the short text it gave, when it gave one.
    Provider { status: u16, reason: Option<String> },
    /// The service could not be reached, or curl failed before an answer.
    Unreachable,
    /// The service answered with something this adapter does not understand.
    BadResponse(&'static str),
    /// The job did not finish in time.
    TimedOut,
    /// The server ran the job and reported an error.
    JobFailed,
    /// A request value that cannot be written into the request.
    InvalidRequest(&'static str),
    /// A catalogue or spec that does not read (a build defect, caught by the crate's tests).
    Data(&'static str),
    /// A file could not be read or written: its kind only, since the system's message can name a
    /// path.
    Io(io::ErrorKind),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProvider(id) => write!(formatter, "No image provider has the id {id}"),
            Self::UnknownFrame(id) => write!(formatter, "No frame has the id {id}"),
            Self::NeedsEndpoint => {
                formatter.write_str("This provider needs an address (--endpoint)")
            }
            Self::NeedsModel => formatter.write_str("This provider needs a model name (--model)"),
            Self::NotUsed(what) => write!(formatter, "This provider takes no {what}"),
            Self::InvalidEndpoint(reason) => {
                write!(formatter, "The address cannot be used: {reason}")
            }
            Self::MissingKey => formatter.write_str(
                "This provider needs your own key: store it with `soyeht avatar key set`",
            ),
            Self::KeyForAnotherAddress => formatter.write_str(
                "A key is stored for this provider, but not for this address: \
store it with `soyeht avatar key set PROVIDER --endpoint ADDRESS`",
            ),
            Self::InvalidKey(reason) => write!(formatter, "The key cannot be used: {reason}"),
            Self::Picture(reason) => write!(formatter, "The picture cannot be used: {reason}"),
            Self::SourceSize {
                width,
                height,
                multiple,
            } => write!(
                formatter,
                "The picture is {width}x{height}; this provider needs both sides to be a multiple of {multiple}"
            ),
            Self::Unauthorized => formatter.write_str("The service refused the key"),
            Self::RateLimited => {
                formatter.write_str("The service asked to slow down; try again later")
            }
            Self::Provider {
                status,
                reason: Some(reason),
            } => write!(formatter, "The service answered {status}: {reason}"),
            Self::Provider {
                status,
                reason: None,
            } => write!(formatter, "The service answered {status}"),
            Self::Unreachable => formatter.write_str("The service could not be reached"),
            Self::BadResponse(reason) => {
                write!(
                    formatter,
                    "The service answered something unexpected: {reason}"
                )
            }
            Self::TimedOut => formatter.write_str("The picture was not ready in time"),
            Self::JobFailed => {
                formatter.write_str("The server reported an error while it made the picture")
            }
            Self::InvalidRequest(reason) => {
                write!(formatter, "The request cannot be sent: {reason}")
            }
            Self::Data(reason) => write!(formatter, "The built-in data is broken: {reason}"),
            Self::Io(kind) => write!(formatter, "A file could not be read or written ({kind})"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

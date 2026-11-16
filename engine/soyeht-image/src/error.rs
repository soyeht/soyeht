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
    /// The address cannot be used: the reason is a fixed phrase.
    InvalidEndpoint(&'static str),
    /// The entry needs the person's own key and none is stored.
    MissingKey,
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
    /// A file could not be read or written.
    Disk(io::Error),
    /// `curl` could not be run within its deadline.
    Command(soyeht_process::Error),
}

impl Error {
    /// The sentence for the person or the agent.
    fn sentence(&self) -> String {
        match self {
            Self::UnknownProvider(id) => format!("No image provider has the id {id}"),
            Self::UnknownFrame(id) => format!("No frame has the id {id}"),
            Self::NeedsEndpoint => String::from("This provider needs an address (--endpoint)"),
            Self::NeedsModel => String::from("This provider needs a model name (--model)"),
            Self::InvalidEndpoint(reason) => {
                format!("The address cannot be used: {reason}")
            }
            Self::MissingKey => String::from(
                "This provider needs your own key: store it with `soyeht avatar key set`",
            ),
            Self::InvalidKey(reason) => format!("The key cannot be used: {reason}"),
            Self::Picture(reason) => format!("The picture cannot be used: {reason}"),
            Self::SourceSize {
                width,
                height,
                multiple,
            } => format!(
                "The picture is {width}x{height}; this provider needs both sides to be a multiple of {multiple}"
            ),
            Self::Unauthorized => String::from("The service refused the key"),
            Self::RateLimited => String::from("The service asked to slow down; try again later"),
            Self::Provider {
                status,
                reason: Some(reason),
            } => {
                format!("The service answered {status}: {reason}")
            }
            Self::Provider {
                status,
                reason: None,
            } => format!("The service answered {status}"),
            Self::Unreachable => String::from("The service could not be reached"),
            Self::BadResponse(reason) => {
                format!("The service answered something unexpected: {reason}")
            }
            Self::TimedOut => String::from("The picture was not ready in time"),
            Self::JobFailed => {
                String::from("The server reported an error while it made the picture")
            }
            Self::InvalidRequest(reason) => {
                format!("The request cannot be sent: {reason}")
            }
            Self::Data(reason) => format!("The built-in data is broken: {reason}"),
            Self::Disk(error) => error.to_string(),
            Self::Command(error) => error.to_string(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.sentence().as_str())
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Disk(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

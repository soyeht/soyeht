//! The tokio flavour of the local endpoint; see the parent module for what it is on each system.

#[cfg(any(windows, test))]
mod tcp;
#[cfg(unix)]
mod unix;

#[cfg(windows)]
pub use tcp::{Listener, OwnedReadHalf, OwnedWriteHalf, Stream, pair, same_user};
#[cfg(unix)]
pub use unix::{Listener, OwnedReadHalf, OwnedWriteHalf, Stream, pair, same_user};

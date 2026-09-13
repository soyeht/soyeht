//! Durable language-server lifecycle and protocol routing.

mod runtime;
mod state;

pub use state::{Completed, DiagnosticUpdate, Server, Service};

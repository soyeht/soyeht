//! Durable language-server lifecycle and protocol routing.

mod api;
mod runtime;
mod state;

pub use state::{Completed, DiagnosticUpdate, Server, Service};

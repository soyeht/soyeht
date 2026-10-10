//! Durable language-server lifecycle and protocol routing.

mod api;
mod format;
mod runtime;
mod state;
mod uri;

pub use state::{Completed, DiagnosticUpdate, Server, Service};

//! Durable language-server lifecycle and protocol routing.

mod api;
mod format;
mod runtime;
mod state;
mod trust;
mod uri;
pub(crate) mod watching;

pub use state::{Completed, DiagnosticUpdate, Server, Service};

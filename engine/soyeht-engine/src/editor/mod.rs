//! File ownership, viewport projection, and filesystem isolation.

mod api;
mod persistence;
mod search;
mod service;

pub use service::{FileSystemPort, Service, SystemFiles};

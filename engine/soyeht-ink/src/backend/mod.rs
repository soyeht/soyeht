//! The recognizer of each system. Each module has the same public shape: `system()`.

#[cfg(target_os = "macos")]
mod vision;
#[cfg(target_os = "macos")]
pub(crate) use vision::system;

#[cfg(not(target_os = "macos"))]
mod absent;
#[cfg(not(target_os = "macos"))]
pub(crate) use absent::system;

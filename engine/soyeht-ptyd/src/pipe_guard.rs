//! What keeps a pipe process (an agent adapter or a language server) from outliving the
//! supervisor, and what ends everything it started.
//!
//! On Unix a guardian process leads the process group and watches a control socket. On Windows
//! the process runs in a job that the system ends when the supervisor's last handle to it
//! closes.

use soyeht_platform::fs::{OpenOptionsPrivateExt, set_private_dir};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

#[cfg(unix)]
#[path = "pipe_guard/unix.rs"]
mod system;
#[cfg(windows)]
#[path = "pipe_guard/windows.rs"]
mod system;

#[cfg(unix)]
pub use system::run;
pub(crate) use system::{Guard, spawn};

/// The private file a pipe process's standard error goes to: it can name paths and tokens, so
/// it is owner-only and never shared with the engine's log.
fn open_diagnostic_log(directory: &Path, id: &str) -> std::io::Result<File> {
    let logs = directory.join("harness-logs");
    std::fs::create_dir_all(&logs)?;
    set_private_dir(&logs)?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .owner_only()
        .open_no_follow(&logs.join(format!("{id}.stderr.log")))
}

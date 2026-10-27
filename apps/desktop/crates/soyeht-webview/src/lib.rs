//! The web view of the operating system, behind one small interface (ADR 0110).
//!
//! A page is drawn off screen by the system's own engine (WebKitGTK on Linux) and handed over as
//! pixels, so the window that shows it is free to put anything over it and stays one window on
//! Wayland and on X11. The person's pointer and keys reach the page as the engine's own input
//! events, which the page sees as a person's. What the agent reads is read in an isolated script
//! world of the engine, which the page cannot see, and the page is captured by the engine itself.
//!
//! The library is found when the first view opens, not when the program is built: a machine
//! without WebKitGTK builds and runs this client, and a web pane there says what is missing.
//!
//! This crate depends on no other crate of Soyeht. It starts no automation interface of the
//! engine, opens no debugging port, and does not change what the page can tell about the browser
//! (`tests::nothing_here_names_an_automation_or_debugging_interface`).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod error;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod frame;
mod keys;
#[cfg(target_os = "linux")]
mod linux;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod policy;
mod types;

pub use error::{Error, Result};
pub use keys::{Keysym, keysym_for_key};
pub use types::{
    Button, Frame, KeyInput, Modifiers, Navigation, PageState, Pointer, ScrollInput, ViewOptions,
    ViewSink, ViewUpdate,
};

#[cfg(target_os = "linux")]
pub use linux::{Backend, WebView};

/// Whether this build has a system web view to load. Where it is false every call fails with
/// [`Error::Unsupported`], and the window says the web pane is not available.
pub const fn supported() -> bool {
    cfg!(target_os = "linux")
}

#[cfg(not(target_os = "linux"))]
mod unsupported;
#[cfg(not(target_os = "linux"))]
pub use unsupported::{Backend, WebView};

#[cfg(test)]
mod tests {
    /// The agent's read must not look like automation to a page or to a site (ADR 0110): no
    /// automation protocol, no debugging port, no inspector server. The names below may appear
    /// in no source file of this crate except this one.
    #[test]
    fn nothing_here_names_an_automation_or_debugging_interface() {
        let forbidden = [
            ["remote", "-debugging"].concat(),
            ["WEBKIT", "_INSPECTOR"].concat(),
            ["web", "driver"].concat(),
            ["set_automation", "_allowed"].concat(),
            ["selen", "ium"].concat(),
            ["play", "wright"].concat(),
            ["pupp", "eteer"].concat(),
            ["enable_developer", "_extras(true"].concat(),
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![root];
        while let Some(directory) = stack.pop() {
            for entry in std::fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.file_name().is_some_and(|name| name == "lib.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap().to_lowercase();
                for name in &forbidden {
                    assert!(
                        !text.contains(&name.to_lowercase()),
                        "{} names {name}",
                        path.display()
                    );
                }
            }
        }
    }
}

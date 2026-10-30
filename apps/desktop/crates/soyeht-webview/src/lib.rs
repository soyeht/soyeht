//! The web view of the operating system, behind one small interface (ADR 0110, ADR 0124).
//!
//! On Linux a page is drawn off screen by the system's own engine (WebKitGTK) and handed over as
//! pixels, so the window that shows it is free to put anything over it and stays one window on
//! Wayland and on X11; the person's pointer and keys reach the page as the engine's own input
//! events, which the page sees as a person's. On Windows the page is the engine's own (Microsoft
//! Edge WebView2) in a window of its own, placed over the pane ([`WebView::place`]), which takes
//! the person's input itself ([`embedded`]). Either way what the agent reads is read in an
//! isolated script world of the engine, which the page cannot see, and the page is captured by
//! the engine itself.
//!
//! The system's web view is found when the first view opens, not when the program is built: a
//! machine without it builds and runs this client, and a web pane there says what is missing.
//!
//! This crate depends on no other crate of Soyeht. It starts no automation interface of the
//! engine, opens no debugging port, refuses to run where the environment or a policy of the
//! machine would open one, and does not change what the page can tell about the browser
//! (`tests::nothing_here_names_an_automation_or_debugging_interface`).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod error;
#[cfg(any(target_os = "linux", windows, test))]
mod frame;
mod keys;
#[cfg(target_os = "linux")]
mod linux;
#[cfg_attr(not(windows), allow(dead_code))]
mod placement;
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
mod policy;
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
mod shared;
mod types;
#[cfg(windows)]
mod webview2;
#[cfg_attr(not(windows), allow(dead_code))]
mod world;

pub use error::{Error, Result};
pub use keys::{Keysym, keysym_for_key};
pub use types::{
    Button, Frame, KeyInput, Modifiers, Navigation, PageState, Placement, Pointer, Rect,
    ScrollInput, ViewOptions, ViewSink, ViewUpdate, WindowHandle,
};

#[cfg(target_os = "linux")]
pub use linux::{Backend, WebView};
#[cfg(windows)]
pub use webview2::{Backend, WebView};

/// Whether this build has a system web view to load. Where it is false every call fails with
/// [`Error::Unsupported`], and the window says the web pane is not available.
pub const fn supported() -> bool {
    cfg!(any(target_os = "linux", windows))
}

/// Whether a page is a window of the system's own, placed over its pane (`WebView::place`), which
/// takes the person's pointer and keys itself and is never handed over as a picture. Where it is
/// false the page is drawn off screen and the window paints its pictures and forwards the input.
pub const fn embedded() -> bool {
    cfg!(windows)
}

#[cfg(not(any(target_os = "linux", windows)))]
mod unsupported;
#[cfg(not(any(target_os = "linux", windows)))]
pub use unsupported::{Backend, WebView};

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// Every source file of this crate but this one, with its text.
    fn sources() -> Vec<(PathBuf, String)> {
        let mut stack = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        let mut found = Vec::new();
        while let Some(directory) = stack.pop() {
            for entry in std::fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.file_name().is_some_and(|name| name != "lib.rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    found.push((path, text));
                }
            }
        }
        found
    }

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
        for (path, text) in sources() {
            let text = text.to_lowercase();
            for name in &forbidden {
                assert!(
                    !text.contains(&name.to_lowercase()),
                    "{} names {name}",
                    path.display()
                );
            }
        }
    }

    /// The reader of a system whose web view has no script world of its own makes four calls of
    /// the engine's protocol and no other, and switches no domain of it on (ADR 0124): the calls a
    /// page can detect are the ones that enable a domain. A name is never built from text: a call
    /// is made from `world::Method`, and the web view's call is made in one place.
    #[test]
    fn the_engines_protocol_is_used_for_four_calls_and_switches_nothing_on() {
        let domains = [
            "Page",
            "Runtime",
            "Network",
            "Input",
            "Emulation",
            "DOM",
            "Target",
            "Browser",
            "Debugger",
            "Log",
            "Fetch",
            "Security",
            "Console",
            "Storage",
        ];
        let mut found = std::collections::BTreeSet::new();
        let mut call_sites = 0;
        for (path, text) in sources() {
            call_sites += text
                .matches(concat!("CallDevTools", "ProtocolMethod("))
                .count();
            for domain in domains {
                // A name written out, or built by `format!` from a domain and a name.
                for opening in [format!("\"{domain}."), format!("\"{{{domain}")] {
                    for piece in text.split(opening.as_str()).skip(1) {
                        let call: String = piece
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric())
                            .collect();
                        found.insert(format!("{domain}.{call}"));
                    }
                }
            }
            assert!(
                !text.contains(concat!("format!(\"", "{method}")),
                "{} builds a method name",
                path.display()
            );
        }
        let allowed: std::collections::BTreeSet<String> = [
            "Page.getFrameTree",
            "Page.createIsolatedWorld",
            "Runtime.evaluate",
            "Page.reload",
        ]
        .map(str::to_owned)
        .into();
        assert_eq!(found, allowed);
        assert_eq!(
            call_sites, 1,
            "the web view's protocol call is made in one place"
        );
    }
}

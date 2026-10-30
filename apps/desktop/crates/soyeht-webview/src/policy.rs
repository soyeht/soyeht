//! Where a page may go. The contract lets a web pane open `http` and `https` addresses; a page
//! also needs `about:blank` and `blob:` for the frames it makes itself. Everything else (`file:`,
//! `data:`, the schemes of other programs) is refused: a page does not get to open them.
//!
//! A frame inside a page is allowed more than the page is, as in a browser (which refuses a
//! `data:` navigation of the top level and not of a frame): many pages embed their own
//! `about:srcdoc` and `data:` frames, and what such a frame loads is as sandboxed as any frame.
//! The back ends apply [`allows`] where they can tell the top level apart (WebView2 can) and
//! [`allows_frame`] to a frame; where they cannot (WebKitGTK reports both the same way) they use
//! [`allows_frame`] at the decision and check the top level once it has loaded (ADR 0124).

/// Whether a navigation to `uri` may happen.
pub(crate) fn allows(uri: &str) -> bool {
    let Some((scheme, rest)) = uri.split_once(':') else {
        return false;
    };
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => rest.starts_with("//") && rest.len() > 2,
        "about" => rest == "blank",
        "blob" => !rest.is_empty(),
        _ => false,
    }
}

/// Whether a frame of a page may load `uri`: what the top level may, and `data:` and
/// `about:srcdoc`, which the frame's own page wrote.
pub(crate) fn allows_frame(uri: &str) -> bool {
    allows(uri)
        || uri.split_once(':').is_some_and(|(scheme, rest)| {
            (scheme.eq_ignore_ascii_case("data") && !rest.is_empty())
                || (scheme.eq_ignore_ascii_case("about") && rest == "srcdoc")
        })
}

/// Whether the address is one a person or an agent may ask for: `http` or `https`, with a host.
pub(crate) fn allows_request(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && allows(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_may_open_web_addresses_and_their_own_blank_frames_only() {
        for open in [
            "http://localhost:5173/",
            "https://example.com/a?b=c",
            "HTTPS://EXAMPLE.COM/",
            "about:blank",
            "blob:https://example.com/7a1c",
        ] {
            assert!(allows(open), "{open}");
        }
        for refused in [
            "file:///etc/passwd",
            "data:text/html,<p>x</p>",
            "javascript:alert(1)",
            "about:config",
            "mailto:a@b.c",
            "x-steam://run/1",
            "https://",
            "example.com",
            "",
        ] {
            assert!(!allows(refused), "{refused}");
        }
    }

    #[test]
    fn a_frame_may_also_hold_data_and_srcdoc_but_the_top_level_may_not() {
        for frame_only in [
            "data:text/html,<p>x</p>",
            "DATA:text/plain;base64,eA==",
            "about:srcdoc",
        ] {
            assert!(allows_frame(frame_only), "{frame_only}");
            assert!(!allows(frame_only), "{frame_only}");
        }
        for same in [
            "https://example.com/",
            "about:blank",
            "blob:https://example.com/7a1c",
        ] {
            assert!(allows_frame(same) && allows(same), "{same}");
        }
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "about:config",
            "mailto:a@b.c",
            "data:",
            "",
        ] {
            assert!(!allows_frame(refused), "{refused}");
        }
    }

    #[test]
    fn a_request_to_open_a_page_is_http_or_https_only() {
        assert!(allows_request("https://example.com/"));
        assert!(allows_request("http://127.0.0.1:8080/x"));
        assert!(!allows_request("about:blank"));
        assert!(!allows_request("blob:https://example.com/7a1c"));
    }
}

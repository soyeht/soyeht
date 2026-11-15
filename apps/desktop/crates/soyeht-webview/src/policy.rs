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
//!
//! A view that shows a project file as a page (contract 11, section 5b) is given a [`PageLimit`]:
//! it goes only to the page's own origin, and its frames may also hold what the page itself made
//! (`about:blank`, `about:srcdoc`, `data:` and `blob:`). The rule that two addresses have one origin
//! is not written here: the window hands over `soyeht_protocol::web::same_origin`, so that this
//! crate stays free of the protocol and there is still one rule. A limit with no page address yet
//! (the engine has not recorded the pane's address) allows nothing, not even the frames a page makes:
//! the page has not loaded, so there is no origin to hold it to and nothing may load but the page.

use std::sync::Arc;

/// Where one view may go: the origin of a page the engine serves. Made by the window, which names
/// the page's address and the rule that tells two addresses of one origin from two of another.
#[derive(Clone)]
pub struct PageLimit {
    page: Arc<str>,
    same_origin: fn(&str, &str) -> bool,
}

impl std::fmt::Debug for PageLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageLimit")
            .field("page", &self.page)
            .finish_non_exhaustive()
    }
}

impl PageLimit {
    /// A limit to the origin of `page`; `same_origin(page, target)` says whether `target` has it.
    /// An empty `page` is a page with no address yet: the limit then allows nothing, so that a view
    /// is held before its first load as it is after it.
    pub fn new(page: &str, same_origin: fn(&str, &str) -> bool) -> Self {
        Self {
            page: page.into(),
            same_origin,
        }
    }

    /// Whether the top level of the view may go to `uri`: its own origin and no other.
    pub fn allows(&self, uri: &str) -> bool {
        self.has_address() && allows(uri) && (self.same_origin)(&self.page, uri)
    }

    /// Whether a frame of the page may load `uri`: the page's origin, and what the page itself
    /// makes, which has no address of its own to go to.
    pub fn allows_frame(&self, uri: &str) -> bool {
        self.allows(uri) || (self.has_address() && made_by_the_page(uri))
    }

    /// Whether the page has an address: before it has, nothing it could make exists either.
    fn has_address(&self) -> bool {
        !self.page.is_empty()
    }
}

/// An address a page makes for itself: a blank or `srcdoc` frame, a `data:` or a `blob:`.
fn made_by_the_page(uri: &str) -> bool {
    uri.split_once(':').is_some_and(|(scheme, rest)| {
        let scheme = scheme.to_ascii_lowercase();
        match scheme.as_str() {
            "about" => rest == "blank" || rest == "srcdoc",
            "data" | "blob" => !rest.is_empty(),
            _ => false,
        }
    })
}

/// [`allows_frame`], or the limit's rule for a frame when the view has one.
pub(crate) fn frame_may_load(limit: Option<&PageLimit>, uri: &str) -> bool {
    match limit {
        Some(limit) => limit.allows_frame(uri),
        None => allows_frame(uri),
    }
}

/// [`allows`], or the limit's rule for the top level when the view has one.
#[cfg(any(windows, test))]
pub(crate) fn top_level_may_load(limit: Option<&PageLimit>, uri: &str) -> bool {
    match limit {
        Some(limit) => limit.allows(uri),
        None => allows(uri),
    }
}

/// [`allows_request`], and the limit's rule when the view has one: a page that asks for a window
/// is not given another origin by it.
pub(crate) fn request_may_open(limit: Option<&PageLimit>, uri: &str) -> bool {
    allows_request(uri) && limit.is_none_or(|limit| limit.allows(uri))
}

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

/// What to do with the page a web view has committed at its top level, for a back end that could
/// not tell the top level from a frame when it decided the navigation.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TopLevel {
    /// The page is one the top level may hold.
    Keep,
    /// The page is not, and there is a page before it: go back to that one, which leaves no entry
    /// of the refused page behind the person's Back button.
    GoBack,
    /// The page is not, and it is the first: replace it with `about:blank`.
    Blank,
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn top_level(limit: Option<&PageLimit>, uri: &str, can_go_back: bool) -> TopLevel {
    // `about:blank` is where a refused first page lands, so a limited view keeps it too.
    let allowed = match limit {
        Some(limit) => limit.allows(uri) || uri.eq_ignore_ascii_case("about:blank"),
        None => allows(uri),
    };
    if uri.is_empty() || allowed {
        TopLevel::Keep
    } else if can_go_back {
        TopLevel::GoBack
    } else {
        TopLevel::Blank
    }
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

    /// What a frame may not load, whatever else it may: a script, a file, a program of the system.
    #[test]
    fn a_frame_still_may_not_run_a_script_read_a_file_or_start_a_program() {
        for refused in [
            "javascript:alert(1)",
            "JavaScript:void(0)",
            "file:///etc/passwd",
            "file://host/share",
            "mailto:a@b.c",
            "tel:123",
            "ms-settings:network",
            "x-steam://run/1",
            "slack://open",
            "vbscript:msgbox(1)",
            "about:config",
            "about:",
            "view-source:https://example.com/",
        ] {
            assert!(!allows_frame(refused), "{refused}");
            assert!(!allows(refused), "{refused}");
            assert!(!allows_request(refused), "{refused}");
        }
    }

    /// The top level, and a request to open a page, are as strict as before the frame rule.
    #[test]
    fn the_top_level_and_a_request_still_refuse_data_and_srcdoc() {
        for frame_only in ["data:text/html,<p>x</p>", "data:,x", "about:srcdoc"] {
            assert!(!allows(frame_only), "{frame_only}");
            assert!(!allows_request(frame_only), "{frame_only}");
        }
        assert!(allows_request("https://example.com/") && allows("about:blank"));
    }

    #[test]
    fn a_top_level_page_that_is_refused_goes_back_when_it_can_and_goes_blank_when_it_cannot() {
        assert_eq!(
            top_level(None, "https://example.com/", true),
            TopLevel::Keep
        );
        assert_eq!(top_level(None, "about:blank", false), TopLevel::Keep);
        assert_eq!(top_level(None, "", true), TopLevel::Keep);
        assert_eq!(top_level(None, "data:text/html,x", true), TopLevel::GoBack);
        assert_eq!(top_level(None, "about:srcdoc", true), TopLevel::GoBack);
        assert_eq!(top_level(None, "data:text/html,x", false), TopLevel::Blank);
        // Going back is not a loop: the page it lands on is one the top level may hold.
        assert_eq!(
            top_level(None, "http://127.0.0.1:8765/", false),
            TopLevel::Keep
        );
    }

    /// A stand-in for `soyeht_protocol::web::same_origin`, which this crate does not know: the
    /// real rule runs through the vector in the window's tests (`soyeht-desktop`).
    fn same_host(page: &str, target: &str) -> bool {
        let host = |address: &str| {
            address
                .strip_prefix("http://")
                .map(|rest| rest.split('/').next().unwrap_or_default().to_owned())
        };
        host(page).is_some() && host(page) == host(target)
    }

    fn limit() -> PageLimit {
        PageLimit::new("http://127.0.0.1:5000/token/index.html", same_host)
    }

    #[test]
    fn a_limited_view_goes_to_its_own_origin_and_a_frame_also_to_what_the_page_made() {
        let limit = limit();
        for same in [
            "http://127.0.0.1:5000/token/other.html",
            "http://127.0.0.1:5000/",
        ] {
            assert!(limit.allows(same) && limit.allows_frame(same), "{same}");
        }
        for elsewhere in [
            "http://127.0.0.1:5001/token/index.html",
            "https://example.com/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "about:config",
            "",
        ] {
            assert!(!limit.allows(elsewhere), "{elsewhere}");
            assert!(!limit.allows_frame(elsewhere), "{elsewhere}");
        }
        for made in [
            "about:blank",
            "ABOUT:srcdoc",
            "data:text/html,<p>x</p>",
            "blob:http://127.0.0.1:5000/7a1c",
        ] {
            assert!(limit.allows_frame(made), "{made}");
            assert!(!limit.allows(made), "{made}");
        }
        for empty in ["data:", "blob:", "about:", "about:config"] {
            assert!(!limit.allows_frame(empty), "{empty}");
        }
    }

    /// A page the engine has not given an address yet: nothing may load, at the top level or in a
    /// frame, whatever the address is and whatever a page would make of it.
    #[test]
    fn a_limit_with_no_page_address_allows_nothing() {
        let none_yet = PageLimit::new("", same_host);
        for uri in [
            "http://127.0.0.1:5000/token/index.html",
            "http://127.0.0.1:5000/",
            "https://example.com/",
            "about:blank",
            "about:srcdoc",
            "data:text/html,<p>x</p>",
            "blob:http://127.0.0.1:5000/7a1c",
            "file:///etc/passwd",
            "",
        ] {
            assert!(!none_yet.allows(uri), "{uri}");
            assert!(!none_yet.allows_frame(uri), "{uri}");
            assert!(!frame_may_load(Some(&none_yet), uri), "{uri}");
            assert!(!top_level_may_load(Some(&none_yet), uri), "{uri}");
            assert!(!request_may_open(Some(&none_yet), uri), "{uri}");
        }
        // The first blank page is kept, and a page that is anywhere else is taken off.
        assert_eq!(
            top_level(Some(&none_yet), "about:blank", false),
            TopLevel::Keep
        );
        assert_eq!(
            top_level(Some(&none_yet), "https://example.com/", false),
            TopLevel::Blank
        );
        assert_eq!(
            top_level(Some(&none_yet), "https://example.com/", true),
            TopLevel::GoBack
        );
    }

    #[test]
    fn a_view_with_no_limit_goes_where_it_went_before() {
        assert!(frame_may_load(None, "https://example.com/"));
        assert!(top_level_may_load(None, "https://example.com/"));
        assert!(request_may_open(None, "https://example.com/"));
        let limit = limit();
        assert!(!frame_may_load(Some(&limit), "https://example.com/"));
        assert!(!top_level_may_load(Some(&limit), "about:blank"));
        assert!(frame_may_load(Some(&limit), "about:blank"));
        assert!(request_may_open(Some(&limit), "http://127.0.0.1:5000/a"));
        assert!(!request_may_open(Some(&limit), "https://example.com/"));
        assert!(!request_may_open(Some(&limit), "about:blank"));
    }

    #[test]
    fn a_limited_top_level_that_left_its_origin_goes_back_or_blank_and_blank_stays() {
        let limit = limit();
        let at = |uri, can_go_back| top_level(Some(&limit), uri, can_go_back);
        assert_eq!(at("http://127.0.0.1:5000/token/a", false), TopLevel::Keep);
        assert_eq!(at("about:blank", false), TopLevel::Keep);
        assert_eq!(at("", true), TopLevel::Keep);
        assert_eq!(at("https://example.com/", true), TopLevel::GoBack);
        assert_eq!(
            at("blob:http://127.0.0.1:5000/7a1c", true),
            TopLevel::GoBack
        );
        assert_eq!(at("data:text/html,x", false), TopLevel::Blank);
    }

    #[test]
    fn a_request_to_open_a_page_is_http_or_https_only() {
        assert!(allows_request("https://example.com/"));
        assert!(allows_request("http://127.0.0.1:8080/x"));
        assert!(!allows_request("about:blank"));
        assert!(!allows_request("blob:https://example.com/7a1c"));
    }
}

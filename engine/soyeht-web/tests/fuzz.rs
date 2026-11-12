//! Fuzz target for the URL policy of web panes: whatever text a person or an agent gives, the
//! pane navigates to an http or https address with no credentials, or the text is refused.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_web::NormalizedUrl;

#[path = "../../soyeht-protocol/tests/fuzz_support/mod.rs"]
mod fuzz_support;
use fuzz_support::{Corpus, fuzz_text};

const CASES: u64 = 40_000;

/// Texts that must never become a navigation, in the spellings a browser reads as the scheme.
const FORBIDDEN: &[&str] = &[
    "file:///etc/passwd",
    "FILE:///etc/passwd",
    "file:/etc/passwd",
    "file://localhost/etc/passwd",
    "javascript:alert(1)",
    "JaVaScRiPt:alert(1)",
    " javascript:alert(1)",
    "\tjavascript:alert(1)",
    "java\nscript:alert(1)",
    "java\tscript:alert(1)",
    "\u{0}javascript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "vbscript:msgbox(1)",
    "blob:https://example.com/1",
    "about:blank",
    "chrome://settings",
    "view-source:https://example.com",
    "ftp://example.com/",
    "ws://example.com/",
    "http://user:pass@example.com/",
    "https://user@example.com/",
    "http://:pass@example.com/",
    "https://",
    "",
    "   ",
    "http://exa mple.com",
    "http://example.com/\u{0}",
    "javascript://%0aalert(1)",
];

#[test]
fn schemes_that_run_code_or_read_files_and_embedded_credentials_are_refused() {
    let accepted: Vec<_> = FORBIDDEN
        .iter()
        .filter(|text| NormalizedUrl::parse(text).is_ok())
        .collect();
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
}

#[test]
fn an_accepted_address_is_http_or_https_without_credentials_and_is_stable() {
    let corpus = Corpus {
        seeds: &[
            b"https://example.com/a?b=c#d",
            b"http://localhost:5173",
            b"localhost:5173",
            b"example.com",
            b"[::1]:8080",
            b"192.168.0.1:80/path",
            b"HTTP://EXAMPLE.COM",
        ],
        tokens: &[
            b"javascript:",
            b"file://",
            b"data:",
            b"@",
            b":",
            b"//",
            b"\\",
            b"%00",
            b"%0a",
            b"\n",
            b"\t",
            b" ",
            b"[",
            b"]",
            b"http://",
            b"https://",
            b"user:pw@",
            b"..",
            b"\xe2\x80\xae",
            b"\xef\xbc\x8f",
            b"0x7f.1",
            b"2130706433",
        ],
        max_len: 120,
    };
    fuzz_text("web-url-policy", CASES, &corpus, |text| {
        let Ok(url) = NormalizedUrl::parse(text) else {
            return;
        };
        let value = url.as_str();
        assert!(
            value.starts_with("http://") || value.starts_with("https://"),
            "{text:?} became {value:?}"
        );
        assert!(!value.chars().any(|c| c.is_control() || c.is_whitespace()));
        assert!(!url.host().is_empty());
        // The text between the scheme and the first slash holds no credentials.
        let authority = value
            .split_once("://")
            .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
            .unwrap_or_default();
        assert!(!authority.contains('@'), "{text:?} became {value:?}");
        // Reading what the policy wrote gives the same address: no second parse can change it.
        let again = NormalizedUrl::parse(value).unwrap();
        assert_eq!(again, url, "{text:?} is not stable");
    });
}

//! The rules for showing a project file as a page (contract 11, section 5b): which request names a
//! file, what answers carry, and what the address of the page is. Pure: the engine does the IO.

use crate::{Error, Result};
use url::Url;

/// What every answer says about the page: an opaque origin (no cookies, no storage, no other page
/// of the loopback interface), scripts and styles of its own, and nothing fetched from anywhere
/// but the server that gave it.
pub const CONTENT_SECURITY_POLICY: &str = "sandbox allow-scripts; default-src 'self' data: blob:; \
script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; \
form-action 'none'; base-uri 'self'; frame-ancestors 'none'";

/// A request's path, split into the parts of a file below the page's directory. Empty when the
/// path is not one of the page: it does not start with the token, names a directory, climbs, or
/// reaches a hidden part.
pub fn requested_parts(token: &str, target: &str) -> Option<Vec<String>> {
    let path = target.split(['?', '#']).next()?;
    let rest = path
        .strip_prefix('/')?
        .strip_prefix(token)?
        .strip_prefix('/')?;
    let parts = rest
        .split('/')
        .map(decoded)
        .collect::<Option<Vec<String>>>()?;
    parts.iter().all(|part| servable(part)).then_some(parts)
}

/// The address of `parts` below the page: `http://127.0.0.1:<port>/<token>/<parts>`.
pub fn page_url(port: u16, token: &str, parts: &[String]) -> Result<String> {
    let mut url =
        Url::parse(&format!("http://127.0.0.1:{port}/")).map_err(|_| Error::InvalidUrl)?;
    url.path_segments_mut()
        .map_err(|()| Error::InvalidUrl)?
        .push(token)
        .extend(parts);
    Ok(url.into())
}

/// Whether a request's `Host` is the loopback address on the page's port, in any spelling a web
/// view of this machine would use. A page that reached the port by a name of its own (a rebinding
/// of an address it controls) is not.
pub fn host_is_page(host: &str, port: u16) -> bool {
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
    .iter()
    .any(|own| own.eq_ignore_ascii_case(host))
}

/// The media type a file of this name is answered as; the answer also says `nosniff`.
pub fn content_type(name: &str) -> &'static str {
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "txt" | "md" | "csv" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

/// Whether a part of a path may name something below the page: not empty (a directory), not a
/// climb, not hidden, and without a character that means something to a file system.
fn servable(part: &str) -> bool {
    !part.is_empty()
        && !part.starts_with('.')
        && !part
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}

/// A path part with its percent escapes resolved; `None` when an escape is not valid or the
/// result is not text.
fn decoded(part: &str) -> Option<String> {
    let bytes = part.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'%' {
            let pair = part.get(at + 1..at + 3)?;
            out.push(u8::from_str_radix(pair, 16).ok()?);
            at += 3;
        } else {
            out.push(byte);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn parts(target: &str) -> Option<Vec<String>> {
        requested_parts(TOKEN, target)
    }

    #[test]
    fn a_request_below_the_token_names_the_parts_of_a_file() {
        assert_eq!(
            parts(&format!("/{TOKEN}/cards.html")),
            Some(vec!["cards.html".into()])
        );
        assert_eq!(
            parts(&format!("/{TOKEN}/deck/a%20b.json?x=1#top")),
            Some(vec!["deck".into(), "a b.json".into()])
        );
    }

    #[test]
    fn every_other_request_is_not_the_page() {
        for target in [
            "/".to_owned(),
            format!("/{TOKEN}"),
            format!("/{TOKEN}/"),
            "/wrong/cards.html".to_owned(),
            format!("/{TOKEN}x/cards.html"),
            format!("/{TOKEN}/../secret"),
            format!("/{TOKEN}/%2e%2e/secret"),
            format!("/{TOKEN}/a/./b"),
            format!("/{TOKEN}/.env"),
            format!("/{TOKEN}/.git/config"),
            format!("/{TOKEN}/a%2fb"),
            format!("/{TOKEN}/a%5cb"),
            format!("/{TOKEN}/a%00b"),
            format!("/{TOKEN}/c:evil"),
            format!("/{TOKEN}/a//b"),
            format!("/{TOKEN}/%zz"),
            format!("/{TOKEN}/%ff"),
            format!("/{TOKEN}/%"),
        ] {
            assert_eq!(parts(&target), None, "{target}");
        }
    }

    #[test]
    fn the_address_of_a_page_escapes_what_a_name_may_hold() {
        let url = page_url(5000, TOKEN, &["deck one".into(), "a#b.html".into()]).unwrap();
        assert_eq!(
            url,
            format!("http://127.0.0.1:5000/{TOKEN}/deck%20one/a%23b.html")
        );
        assert_eq!(
            parts(&format!("/{TOKEN}/deck%20one/a%23b.html")),
            Some(vec!["deck one".into(), "a#b.html".into()])
        );
    }

    #[test]
    fn only_the_loopback_address_on_the_port_is_the_page_host() {
        for host in [
            "127.0.0.1:5000",
            "localhost:5000",
            "LOCALHOST:5000",
            "[::1]:5000",
        ] {
            assert!(host_is_page(host, 5000), "{host}");
        }
        for host in [
            "127.0.0.1:5001",
            "evil.example:5000",
            "127.0.0.1",
            "",
            "a.localhost:5000",
        ] {
            assert!(!host_is_page(host, 5000), "{host}");
        }
    }

    #[test]
    fn a_file_is_answered_by_its_extension_and_never_as_script_by_guess() {
        assert_eq!(content_type("a.HTML"), "text/html; charset=utf-8");
        assert_eq!(content_type("a.mjs"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("a.unknown"), "application/octet-stream");
        assert_eq!(content_type("noextension"), "application/octet-stream");
    }

    #[test]
    fn the_policy_gives_the_page_an_opaque_origin_and_no_other_network() {
        assert!(CONTENT_SECURITY_POLICY.starts_with("sandbox allow-scripts;"));
        assert!(!CONTENT_SECURITY_POLICY.contains("allow-same-origin"));
        assert!(CONTENT_SECURITY_POLICY.contains("connect-src 'self'"));
    }
}

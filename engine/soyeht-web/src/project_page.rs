//! The rules for showing a project file as a page (contract 11, section 5b): which request names a
//! file, what answers carry, and what the address of the page is. Pure: the engine does the IO.

use crate::{Error, Result};
use url::Url;

/// What every answer says about the page: an opaque origin (no cookies, no storage, no other page
/// of the loopback interface), scripts and styles of its own, and nothing loaded or fetched from
/// anywhere but the server that gave it. `origin` is that server's address, `http://127.0.0.1:<port>`.
/// It is written out and not `'self'`: the origin of a sandboxed page is opaque, and WebKit does
/// not match `'self'` against it, so a page could not fetch its own files.
pub fn content_security_policy(origin: &str) -> String {
    format!(
        "sandbox allow-scripts; default-src {origin} data: blob:; script-src {origin} 'unsafe-inline'; \
style-src {origin} 'unsafe-inline'; connect-src {origin}; form-action 'none'; base-uri {origin}; \
frame-ancestors 'none'"
    )
}

/// A request's path, split into the token the page's address starts with and the parts of the
/// file below it. `None` when the path is not one of a page: it names no token or no file, names a
/// directory, climbs, or reaches a hidden or unusable part. The token is returned as written (the
/// engine compares it in constant time); it is not decoded, so an escape in it matches nothing.
pub fn requested_parts(target: &str) -> Option<(String, Vec<String>)> {
    let path = target.split(['?', '#']).next()?;
    let rest = path.strip_prefix('/')?;
    let (token, rest) = rest.split_once('/')?;
    let parts = rest
        .split('/')
        .map(decoded)
        .collect::<Option<Vec<String>>>()?;
    (!token.is_empty() && parts.iter().all(|part| name_allowed(part)))
        .then(|| (token.to_owned(), parts))
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

/// Whether a part of a path may name something of a project page, on every system, so that a
/// name means the same wherever the project is opened: not empty (a directory), not hidden (no
/// leading `.`, which also refuses `.` and `..`), no control character, no `/`, `\` or `:` (a
/// separator, a drive, an alternate data stream), no trailing `.` or space (Windows drops them), and
/// not the name of a Windows device whatever its extension (`CON`, `NUL.txt`, `COM1`).
pub fn name_allowed(part: &str) -> bool {
    !part.is_empty()
        && !part.starts_with('.')
        && !part.ends_with(['.', ' '])
        && !part
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        && !is_device_name(part)
}

fn is_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            upper
                .strip_prefix(prefix)
                .is_some_and(|digit| matches!(digit.as_bytes(), [b'0'..=b'9']))
        })
}

/// What a `Range` header asks of a file of `length` bytes.
#[derive(Debug, PartialEq, Eq)]
pub enum RangeAnswer {
    /// The whole file: no header, or one this server does not answer in part (several ranges, a unit
    /// other than bytes).
    Whole,
    /// Bytes `start` to `end`, both included.
    Part { start: u64, end: u64 },
    /// A range that lies outside the file.
    Unsatisfiable,
}

/// How to answer the `Range` header of a request for a file of `length` bytes.
pub fn byte_range(header: Option<&str>, length: u64) -> RangeAnswer {
    let Some(spec) = header.and_then(|header| header.trim().strip_prefix("bytes=")) else {
        return RangeAnswer::Whole;
    };
    if spec.contains(',') {
        return RangeAnswer::Whole;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return RangeAnswer::Whole;
    };
    let (first, last) = (first.trim(), last.trim());
    let number = |text: &str| text.parse::<u64>().ok();
    let range = match (first, last) {
        ("", suffix) => {
            number(suffix).map(|suffix| (length.saturating_sub(suffix), length.checked_sub(1)))
        }
        (start, "") => number(start).map(|start| (start, length.checked_sub(1))),
        (start, end) => number(start)
            .zip(number(end))
            .map(|(start, end)| (start, Some(end.min(length.saturating_sub(1))))),
    };
    match range {
        None => RangeAnswer::Whole,
        Some((start, Some(end))) if start <= end && start < length => {
            RangeAnswer::Part { start, end }
        }
        Some(_) => RangeAnswer::Unsatisfiable,
    }
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
        requested_parts(target).map(|(token, parts)| {
            assert_eq!(token, TOKEN);
            parts
        })
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
    fn every_other_request_is_not_a_file_of_a_page() {
        for target in [
            "/".to_owned(),
            format!("/{TOKEN}"),
            format!("/{TOKEN}/"),
            "//cards.html".to_owned(),
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
            assert_eq!(
                requested_parts(&target).map(|(_, parts)| parts),
                None,
                "{target}"
            );
        }
        // Whatever token is written is returned for the engine to compare, and never decoded.
        assert_eq!(
            requested_parts("/a%41/cards.html"),
            Some(("a%41".into(), vec!["cards.html".into()]))
        );
    }

    /// A name that means one thing on a system and another on a second is not served on either: the
    /// forms of Windows (drive-relative, verbatim, alternate stream, device, trailing dot or space)
    /// are refused on every system, so these cases run on any host.
    #[test]
    fn a_name_is_allowed_only_where_it_means_the_same_on_every_system() {
        for allowed in [
            "index.html",
            "a b.png",
            "x.y.z",
            "console.log",
            "com.html",
            "COM10.txt",
            "lpt.txt",
            "ünï.txt",
            "a-b_c.js",
        ] {
            assert!(name_allowed(allowed), "{allowed:?}");
        }
        for refused in [
            "",
            ".",
            "..",
            ".env",
            ".git",
            "C:x.html",
            "c:",
            "x.html::$DATA",
            "x.html:stream",
            "\\\\?\\C:\\x",
            "a\\b.html",
            "a/b",
            "x.html.",
            "x.html ",
            "x. ",
            "CON",
            "con",
            "NUL.txt",
            "Aux.html",
            "PRN",
            "COM1",
            "com9.html",
            "LPT3.txt",
            "COM1 .txt",
            "tab\tname",
            "nul\0name",
        ] {
            assert!(!name_allowed(refused), "{refused:?}");
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

    /// The header is pinned whole: a sandboxed page has an opaque origin, `'self'` does not match
    /// it in WebKit, so each source that must match the server's own files names its address.
    #[test]
    fn the_policy_gives_the_page_an_opaque_origin_and_only_its_own_address() {
        assert_eq!(
            content_security_policy("http://127.0.0.1:5000"),
            "sandbox allow-scripts; default-src http://127.0.0.1:5000 data: blob:; \
script-src http://127.0.0.1:5000 'unsafe-inline'; style-src http://127.0.0.1:5000 'unsafe-inline'; \
connect-src http://127.0.0.1:5000; form-action 'none'; base-uri http://127.0.0.1:5000; \
frame-ancestors 'none'"
        );
        let policy = content_security_policy("http://127.0.0.1:5000");
        assert!(!policy.contains("allow-same-origin") && !policy.contains("'self'"));
    }

    #[test]
    fn a_range_is_answered_in_part_when_it_is_one_the_file_has() {
        use RangeAnswer::{Part, Unsatisfiable, Whole};
        for (header, answer) in [
            (None, Whole),
            (Some("bytes=0-3"), Part { start: 0, end: 3 }),
            (Some("bytes=2-"), Part { start: 2, end: 9 }),
            (Some("bytes=-4"), Part { start: 6, end: 9 }),
            (Some("bytes=-40"), Part { start: 0, end: 9 }),
            (Some("bytes=5-500"), Part { start: 5, end: 9 }),
            (Some("bytes=9-9"), Part { start: 9, end: 9 }),
            (Some("bytes=10-"), Unsatisfiable),
            (Some("bytes=10-12"), Unsatisfiable),
            (Some("bytes=4-2"), Unsatisfiable),
            (Some("bytes=-0"), Unsatisfiable),
            (Some("bytes=0-1,4-5"), Whole),
            (Some("items=0-1"), Whole),
            (Some("bytes=a-b"), Whole),
            (Some("bytes"), Whole),
        ] {
            assert_eq!(byte_range(header, 10), answer, "{header:?}");
        }
        assert_eq!(byte_range(Some("bytes=0-"), 0), Unsatisfiable);
    }
}

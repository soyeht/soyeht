//! What every part of the engine agrees on about the web methods.

use crate::method;

/// Whether `method` is a command the engine sends to the web host and waits for the answer of.
pub fn is_host_command(method: &str) -> bool {
    matches!(
        method,
        method::WEB_SNAPSHOT
            | method::WEB_CLICK
            | method::WEB_TYPE
            | method::WEB_PRESS
            | method::WEB_HOVER
            | method::WEB_SELECT
            | method::WEB_SCROLL
            | method::WEB_WAIT
            | method::WEB_EVALUATE
            | method::WEB_SCREENSHOT
            | method::WEB_READ
            | method::WEB_CONSOLE
            | method::WEB_NETWORK
            | method::WEB_RESIZE
    )
}

/// Whether an answer to `method` waits on the web host, and so may take as long as the host's
/// own waits (a read of a long page, a navigation): a command, or a navigation. `web.open` is one
/// when an agent opens a pane, which is sent to its address as a navigation.
pub fn waits_for_host(method: &str) -> bool {
    is_host_command(method) || matches!(method, method::WEB_NAVIGATE | method::WEB_OPEN)
}

/// Whether `target` has the scheme, host and port of `page`: where a pane that shows a project
/// file as a page may go (contract 11, section 5b; the cases are
/// `contracts/vectors/project-page-origin.json`, which the Swift client passes as well). Only
/// `http` and `https` addresses with a plain authority (no user information, no backslash, no
/// space) have an origin, so anything odd is a different one.
pub fn same_origin(page: &str, target: &str) -> bool {
    matches!((origin_of(page), origin_of(target)), (Some(page), Some(target)) if page == target)
}

fn origin_of(address: &str) -> Option<(String, String, u16)> {
    if address
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return None;
    }
    let (scheme, rest) = address.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        let (host, after) = inner.split_once(']')?;
        (format!("[{host}]"), after.strip_prefix(':'))
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host.to_owned(), Some(port)),
            None => (authority.to_owned(), None),
        }
    };
    if host.is_empty() || host.contains(':') && !host.starts_with('[') {
        return None;
    }
    let port = match port {
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            digits.parse().ok()?
        }
        Some(_) => return None,
        None => default_port,
    };
    Some((scheme, host.to_ascii_lowercase(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_commands_are_the_web_methods_that_wait_for_the_host() {
        assert!(is_host_command(method::WEB_READ));
        assert!(!is_host_command(method::WEB_OPEN));
        assert!(waits_for_host(method::WEB_NAVIGATE) && !is_host_command(method::WEB_NAVIGATE));
        assert!(waits_for_host(method::WEB_OPEN) && !is_host_command(method::WEB_OPEN));
        assert!(!is_host_command(method::TASK_NEXT));
    }

    #[test]
    fn a_pane_of_a_project_page_goes_only_where_the_shared_cases_say() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/vectors/project-page-origin.json"
        );
        let vector: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cases = vector["cases"].as_array().unwrap();
        assert!(cases.len() > 20);
        for case in cases {
            assert_eq!(
                same_origin(
                    case["page"].as_str().unwrap(),
                    case["target"].as_str().unwrap()
                ),
                case["same"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }
}

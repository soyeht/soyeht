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
/// own waits (a read of a long page, a navigation): a command, or a navigation.
pub fn waits_for_host(method: &str) -> bool {
    is_host_command(method) || method == method::WEB_NAVIGATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_commands_are_the_web_methods_that_wait_for_the_host() {
        assert!(is_host_command(method::WEB_READ));
        assert!(!is_host_command(method::WEB_OPEN));
        assert!(waits_for_host(method::WEB_NAVIGATE) && !is_host_command(method::WEB_NAVIGATE));
        assert!(!is_host_command(method::TASK_NEXT));
    }
}

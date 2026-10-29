//! The name of this machine, as the system reports it.

/// The host name of this machine, or `None` when the system gives none.
pub fn get() -> Option<String> {
    system::get().filter(|name| !name.is_empty())
}

#[cfg(unix)]
mod system {
    use std::ffi::c_char;

    /// Room for the longest host name either system allows (255) and the end of the text.
    const BUFFER: usize = 256;

    pub(super) fn get() -> Option<String> {
        let mut buffer = [0_u8; BUFFER];
        // SAFETY: the buffer outlives the call and the length given is its own.
        let status = unsafe { libc::gethostname(buffer.as_mut_ptr().cast::<c_char>(), BUFFER) };
        if status != 0 {
            return None;
        }
        // A name that fills the buffer has no end of its own: the text stops at the last byte.
        let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(BUFFER);
        Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
    }
}

#[cfg(windows)]
mod system {
    pub(super) fn get() -> Option<String> {
        std::env::var_os("COMPUTERNAME").map(|name| name.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn this_machine_has_a_name_without_a_line_break() {
        if let Some(name) = super::get() {
            assert!(!name.contains(['\n', '\0']));
        }
    }
}

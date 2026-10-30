//! What can change which web view runs, or how, from outside the program: environment variables
//! and the policy keys of the registry that the loader reads (Microsoft's documentation of
//! `CreateCoreWebView2EnvironmentWithOptions`). A web pane never runs with one of them set: a
//! different runtime, a user data folder of somebody else's, a switch that opens a debugging port
//! or a script debugger's pipe would each be a way into the page.

use crate::{Error, Result, shared::refuse_environment};
use windows::{
    Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW,
        RegQueryInfoKeyW,
    },
    core::PCWSTR,
};

/// Environment variables the loader and the browser read.
pub(super) const VARIABLES: [&str; 8] = [
    "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
    "WEBVIEW2_USER_DATA_FOLDER",
    "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
    "WEBVIEW2_CHANNEL_SEARCH_KIND",
    "WEBVIEW2_RELEASE_CHANNELS",
    "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
    "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
];

/// The policy keys under `Software\Policies\Microsoft\Edge\WebView2`: each holds a value named for
/// an application (or `*`), which is how a policy applies.
pub(super) const POLICIES: [&str; 5] = [
    "BrowserExecutableFolder",
    "UserDataFolder",
    "ReleaseChannels",
    "ChannelSearchKind",
    "AdditionalBrowserArguments",
];

const POLICY_ROOT: &str = "Software\\Policies\\Microsoft\\Edge\\WebView2";

/// Fails with `Unavailable` when the environment or the registry carries an override.
pub(super) fn refuse() -> Result<()> {
    refuse_environment(
        &VARIABLES,
        "a WEBVIEW2_ variable of this program's environment changes which web view runs, or how; \
unset it and start the program again",
    )?;
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for name in POLICIES {
            if holds_a_value(hive, &format!("{POLICY_ROOT}\\{name}")) {
                return Err(Error::Unavailable(
                    "a WebView2 policy of this machine (Software\\Policies\\Microsoft\\Edge\\WebView2) changes which \
web view runs, or how; ask the administrator who set it to remove it",
                ));
            }
        }
    }
    Ok(())
}

/// Whether the key `key_path` of `hive` exists and holds at least one value. A key that cannot
/// be opened is not there (the system says "not found" for a key nobody made).
fn holds_a_value(hive: HKEY, key_path: &str) -> bool {
    let path: Vec<u16> = key_path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut key = HKEY::default();
    // SAFETY: the path is zero-terminated and outlives the call; `key` is this call's own.
    let opened = unsafe {
        RegOpenKeyExW(
            hive,
            PCWSTR::from_raw(path.as_ptr()),
            None,
            KEY_READ,
            &raw mut key,
        )
    };
    if opened.is_err() {
        return false;
    }
    let mut values = 0_u32;
    // SAFETY: the key was just opened; only the count of values is asked for.
    let asked = unsafe {
        RegQueryInfoKeyW(
            key,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&raw mut values),
            None,
            None,
            None,
            None,
        )
    };
    // SAFETY: the key is closed once.
    if unsafe { RegCloseKey(key) }.is_err() {
        // A key that cannot be closed is not one worth another word.
    }
    // A key whose values cannot be counted is taken as holding one: the safe side.
    asked.is_err() || values > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overrides_are_the_ones_the_loader_documents() {
        for name in [
            "WEBVIEW2_USER_DATA_FOLDER",
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
        ] {
            assert!(VARIABLES.contains(&name), "{name}");
        }
        assert!(POLICIES.contains(&"AdditionalBrowserArguments"));
    }

    #[test]
    fn a_policy_key_nobody_made_holds_nothing() {
        assert!(!holds_a_value(
            HKEY_CURRENT_USER,
            "Software\\NoSuchPolicyOfOurs"
        ));
    }

    /// A key of this test's own under the user's hive: empty at first, then with one value.
    #[test]
    fn a_key_with_a_value_is_an_override_and_an_empty_key_is_not() {
        use windows::Win32::System::Registry::{
            REG_SZ, RegCreateKeyW, RegDeleteTreeW, RegSetValueExW,
        };
        let path = format!("Software\\SoyehtNextTest{}", std::process::id());
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let name = PCWSTR::from_raw(wide.as_ptr());
        let mut key = HKEY::default();
        // SAFETY: a key under the user's hive, named for this process, removed below.
        unsafe {
            RegCreateKeyW(HKEY_CURRENT_USER, name, &raw mut key)
                .ok()
                .unwrap()
        };
        assert!(!holds_a_value(HKEY_CURRENT_USER, &path), "an empty key");
        let value: Vec<u8> = "x\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
        // SAFETY: the key is open for writing; the value outlives the call, which copies it.
        unsafe {
            RegSetValueExW(key, windows::core::w!("*"), None, REG_SZ, Some(&value))
                .ok()
                .unwrap();
        }
        assert!(
            holds_a_value(HKEY_CURRENT_USER, &path),
            "a key with a value"
        );
        // SAFETY: the key and the tree under it are this test's own.
        unsafe {
            RegCloseKey(key).ok().unwrap();
            RegDeleteTreeW(HKEY_CURRENT_USER, name).ok().unwrap();
        }
        assert!(
            !holds_a_value(HKEY_CURRENT_USER, &path),
            "a key that is gone"
        );
    }
}

//! Cookies from the user's macOS browsers, read the way CodexBar's cookie import reads them:
//! each browser's own cookie database opened read-only, Chromium values decrypted with the key
//! the browser keeps in the login keychain. The first read of a browser's key shows the macOS
//! consent prompt. Nothing is written to any browser.
//!
//! Safari is not read: its cookie file needs Full Disk Access, which macOS grants only in System
//! Settings, never through a prompt.

use super::process;
use soyeht_usage::foreign_db;
use soyeht_usage::host::{Cookie, CookieJar, CookieRead, Secret, SecretRead};
use std::path::{Path, PathBuf};

/// A Chromium browser: its profile root under Application Support and its keychain item.
struct Chromium {
    root: &'static str,
    keychain_service: &'static str,
    keychain_account: &'static str,
}

/// Chrome first, as in CodexBar, then the other Chromium browsers.
const CHROMIUM: [Chromium; 6] = [
    Chromium {
        root: "Google/Chrome",
        keychain_service: "Chrome Safe Storage",
        keychain_account: "Chrome",
    },
    Chromium {
        root: "Arc/User Data",
        keychain_service: "Arc Safe Storage",
        keychain_account: "Arc",
    },
    Chromium {
        root: "BraveSoftware/Brave-Browser",
        keychain_service: "Brave Safe Storage",
        keychain_account: "Brave",
    },
    Chromium {
        root: "Microsoft Edge",
        keychain_service: "Microsoft Edge Safe Storage",
        keychain_account: "Microsoft Edge",
    },
    Chromium {
        root: "Vivaldi",
        keychain_service: "Vivaldi Safe Storage",
        keychain_account: "Vivaldi",
    },
    Chromium {
        root: "Chromium",
        keychain_service: "Chromium Safe Storage",
        keychain_account: "Chromium",
    },
];

const MAX_PROFILES: usize = 32;

pub(super) fn cookies(home: &Path, domain: &str) -> CookieRead {
    let support = home.join("Library/Application Support");
    let mut found = CookieRead::default();
    for browser in &CHROMIUM {
        let databases = profiles(&support.join(browser.root), |profile| {
            [profile.join("Cookies"), profile.join("Network/Cookies")]
        });
        let mut key = None;
        for database in databases {
            let rows = foreign_db::chromium_cookies(&database, domain);
            if rows.is_empty() {
                continue;
            }
            // Ask for the browser's key only once a profile has cookies for the domain.
            let key = match &key {
                Some(key) => key,
                None => match chromium_key(browser) {
                    Ok(derived) => key.insert(derived),
                    Err(denied) => {
                        found.denied |= denied;
                        break;
                    }
                },
            };
            let cookies = rows
                .into_iter()
                .filter_map(|cookie| {
                    decrypt(key, &cookie.encrypted, cookie.version).map(|value| Cookie {
                        name: cookie.name,
                        value: Secret::new(value),
                    })
                })
                .collect::<Vec<_>>();
            if !cookies.is_empty() {
                found.jars.push(CookieJar { cookies });
            }
        }
    }
    let firefox = profiles(&support.join("Firefox/Profiles"), |profile| {
        [profile.join("cookies.sqlite"), PathBuf::new()]
    });
    for database in firefox {
        let cookies = foreign_db::firefox_cookies(&database, domain);
        if !cookies.is_empty() {
            found.jars.push(CookieJar { cookies });
        }
    }
    found
}

/// Existing cookie databases in each profile directory under `root`.
fn profiles(root: &Path, candidates: impl Fn(&Path) -> [PathBuf; 2]) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut directories: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();
    directories
        .iter()
        .take(MAX_PROFILES)
        .flat_map(|profile| candidates(profile))
        .filter(|path| path.is_file())
        .collect()
}

/// The browser's cookie key, or whether the keychain refused it (`true`) rather than not
/// having it (`false`).
fn chromium_key(browser: &Chromium) -> Result<[u8; 16], bool> {
    match process::keychain_password(
        Path::new(process::SECURITY),
        browser.keychain_service,
        Some(browser.keychain_account),
    ) {
        SecretRead::Found(password) => {
            crypto::derive_key(password.expose().as_bytes()).ok_or(false)
        }
        SecretRead::Denied => Err(true),
        SecretRead::Missing | SecretRead::Unsupported => Err(false),
    }
}

/// A `v10` value: AES-128-CBC with a fixed IV. From database version 24 the plaintext starts
/// with the SHA-256 of the cookie's host, which is not part of the value.
fn decrypt(key: &[u8; 16], encrypted: &[u8], version: i64) -> Option<String> {
    let ciphertext = encrypted.strip_prefix(b"v10")?;
    let plain = crypto::aes_cbc_decrypt(key, ciphertext)?;
    let value = if version >= 24 {
        plain.get(32..)?
    } else {
        &plain
    };
    String::from_utf8(value.to_vec()).ok()
}

/// CommonCrypto, part of macOS: PBKDF2 for the browser key and AES-CBC for the values.
mod crypto {
    const SALT: &[u8] = b"saltysalt";
    const ROUNDS: u32 = 1003;
    const IV: [u8; 16] = [b' '; 16];
    const PBKDF2: u32 = 2;
    const PRF_HMAC_SHA1: u32 = 1;
    const DECRYPT: u32 = 1;
    const AES: u32 = 0;
    const PKCS7_PADDING: u32 = 1;
    const SUCCESS: i32 = 0;

    #[link(name = "System", kind = "dylib")]
    unsafe extern "C" {
        fn CCKeyDerivationPBKDF(
            algorithm: u32,
            password: *const u8,
            password_len: usize,
            salt: *const u8,
            salt_len: usize,
            prf: u32,
            rounds: u32,
            derived: *mut u8,
            derived_len: usize,
        ) -> i32;
        fn CCCrypt(
            op: u32,
            alg: u32,
            options: u32,
            key: *const u8,
            key_length: usize,
            iv: *const u8,
            data_in: *const u8,
            data_in_length: usize,
            data_out: *mut u8,
            data_out_available: usize,
            data_out_moved: *mut usize,
        ) -> i32;
    }

    pub(super) fn derive_key(password: &[u8]) -> Option<[u8; 16]> {
        let mut key = [0_u8; 16];
        // SAFETY: every pointer refers to a live buffer of the length passed with it.
        let status = unsafe {
            CCKeyDerivationPBKDF(
                PBKDF2,
                password.as_ptr(),
                password.len(),
                SALT.as_ptr(),
                SALT.len(),
                PRF_HMAC_SHA1,
                ROUNDS,
                key.as_mut_ptr(),
                key.len(),
            )
        };
        (status == SUCCESS).then_some(key)
    }

    pub(super) fn aes_cbc_decrypt(key: &[u8; 16], ciphertext: &[u8]) -> Option<Vec<u8>> {
        if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
            return None;
        }
        aes_cbc(DECRYPT, key, ciphertext)
    }

    #[cfg(test)]
    pub(super) fn aes_cbc_encrypt(key: &[u8; 16], plain: &[u8]) -> Option<Vec<u8>> {
        aes_cbc(ENCRYPT, key, plain)
    }

    #[cfg(test)]
    const ENCRYPT: u32 = 0;

    fn aes_cbc(operation: u32, key: &[u8; 16], input: &[u8]) -> Option<Vec<u8>> {
        let mut output = vec![0_u8; input.len() + 16];
        let mut moved = 0_usize;
        // SAFETY: key and IV are 16 bytes; input and output buffers are live with the lengths
        // passed; `moved` is a live integer CommonCrypto writes the output length into.
        let status = unsafe {
            CCCrypt(
                operation,
                AES,
                PKCS7_PADDING,
                key.as_ptr(),
                key.len(),
                IV.as_ptr(),
                input.as_ptr(),
                input.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut moved,
            )
        };
        if status != SUCCESS || moved > output.len() {
            return None;
        }
        output.truncate(moved);
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_values_decrypt_with_the_derived_key() {
        let key = crypto::derive_key(b"recorded-safe-storage-password").unwrap();
        let other = crypto::derive_key(b"another-password").unwrap();
        assert_ne!(key, other);
        let mut encrypted = b"v10".to_vec();
        encrypted.extend(crypto::aes_cbc_encrypt(&key, b"session-value").unwrap());
        assert_eq!(
            decrypt(&key, &encrypted, 0).as_deref(),
            Some("session-value")
        );
        assert_ne!(
            decrypt(&other, &encrypted, 0).as_deref(),
            Some("session-value")
        );
        let mut hashed = vec![7_u8; 32];
        hashed.extend_from_slice(b"session-value");
        let mut encrypted = b"v10".to_vec();
        encrypted.extend(crypto::aes_cbc_encrypt(&key, &hashed).unwrap());
        assert_eq!(
            decrypt(&key, &encrypted, 24).as_deref(),
            Some("session-value")
        );
        assert_eq!(decrypt(&key, b"v11abc", 0), None);
    }
}

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use soyeht_image::{Error, Vault};
use support::Scratch;

#[test]
fn a_key_is_stored_read_replaced_and_cleared() {
    let scratch = Scratch::new();
    let vault = Vault::new(&scratch.0);
    assert!(vault.read("some-provider").unwrap().is_none());
    vault.set("some-provider", " sk-first \n").unwrap();
    assert!(vault.read("some-provider").unwrap().is_some());
    vault.set("some-provider", "sk-second").unwrap();
    assert!(vault.clear("some-provider").unwrap());
    assert!(!vault.clear("some-provider").unwrap());
    assert!(vault.read("some-provider").unwrap().is_none());
}

#[test]
fn a_key_never_prints() {
    let scratch = Scratch::new();
    let vault = Vault::new(&scratch.0);
    vault.set("some-provider", "sk-very-secret").unwrap();
    let key = vault.read("some-provider").unwrap().unwrap();
    assert!(!format!("{key:?}").contains("secret"));
}

#[test]
fn a_key_that_cannot_travel_in_a_header_is_refused() {
    let scratch = Scratch::new();
    let vault = Vault::new(&scratch.0);
    for bad in ["", "   ", "has space", "line\nbreak", &"k".repeat(1001)] {
        assert!(
            matches!(vault.set("some-provider", bad), Err(Error::InvalidKey(_))),
            "{bad:?}"
        );
    }
    // A quote is printable, and the request escapes it.
    vault.set("some-provider", "quote\"inside").unwrap();
}

#[test]
fn a_provider_id_cannot_leave_the_folder() {
    let scratch = Scratch::new();
    let vault = Vault::new(&scratch.0);
    for bad in ["", "../x", "a/b", "A", ".hidden", "a..b", "a\\b"] {
        assert!(
            matches!(vault.set(bad, "sk-key"), Err(Error::UnknownProvider(_))),
            "{bad:?}"
        );
        assert!(matches!(vault.read(bad), Err(Error::UnknownProvider(_))));
    }
}

#[cfg(unix)]
#[test]
fn the_key_file_and_its_folder_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    Vault::new(&scratch.0)
        .set("some-provider", "sk-key")
        .unwrap();
    let folder = scratch.path("image-keys");
    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&folder), 0o700);
    assert_eq!(mode(&folder.join("some-provider")), 0o600);
}

#[test]
fn a_provider_id_with_a_version_dot_is_a_plain_name() {
    let scratch = Scratch::new();
    let vault = Vault::new(&scratch.0);
    vault.set("model-2.1-name", "sk-key").unwrap();
    assert!(vault.read("model-2.1-name").unwrap().is_some());
}

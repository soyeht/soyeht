#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_image::{Error, Vault, testing::Scratch};

const HOST: &str = "api.example.test:443";
const OTHER: &str = "images.other.test:443";

#[test]
fn a_key_is_stored_read_replaced_and_cleared_per_address() {
    let scratch = Scratch::new("image-vault");
    let vault = Vault::new(&scratch.0);
    assert!(matches!(
        vault.key_for("some-provider", HOST),
        Err(Error::MissingKey)
    ));
    vault.set("some-provider", HOST, " sk-first \n").unwrap();
    vault.set("some-provider", HOST, "sk-second").unwrap();
    vault.set("some-provider", OTHER, "sk-other").unwrap();
    assert!(vault.key_for("some-provider", HOST).is_ok());
    assert_eq!(vault.hosts("some-provider").unwrap(), [HOST, OTHER]);
    assert_eq!(vault.clear("some-provider", Some(HOST)).unwrap(), 1);
    assert_eq!(vault.clear("some-provider", Some(HOST)).unwrap(), 0);
    assert_eq!(vault.clear("some-provider", None).unwrap(), 1);
    assert!(vault.hosts("some-provider").unwrap().is_empty());
}

#[test]
fn a_key_given_for_one_address_is_not_found_for_another() {
    let scratch = Scratch::new("image-vault");
    let vault = Vault::new(&scratch.0);
    vault.set("some-provider", HOST, "sk-key").unwrap();
    assert!(matches!(
        vault.key_for("some-provider", OTHER),
        Err(Error::KeyForAnotherAddress)
    ));
    assert!(matches!(
        vault.key_for("another-provider", HOST),
        Err(Error::MissingKey)
    ));
}

#[test]
fn a_key_never_prints() {
    let scratch = Scratch::new("image-vault");
    let vault = Vault::new(&scratch.0);
    vault.set("some-provider", HOST, "sk-very-secret").unwrap();
    let key = vault.key_for("some-provider", HOST).unwrap();
    assert!(!format!("{key:?}").contains("secret"));
}

#[test]
fn a_key_that_cannot_travel_in_a_header_is_refused() {
    let scratch = Scratch::new("image-vault");
    let vault = Vault::new(&scratch.0);
    for bad in [
        "",
        "   ",
        "has space",
        "line\nbreak",
        "quote\"inside",
        "back\\slash",
        &"k".repeat(1001),
    ] {
        assert!(
            matches!(
                vault.set("some-provider", HOST, bad),
                Err(Error::InvalidKey(_))
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn a_hand_edited_file_cannot_add_a_second_header_line() {
    let scratch = Scratch::new("image-vault");
    let vault = Vault::new(&scratch.0);
    vault.set("some-provider", HOST, "sk-key").unwrap();
    let folder = scratch.path("image-keys").join("some-provider");
    let file = std::fs::read_dir(&folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&file, format!("{HOST}\nsk-key\nX-Evil: 1\n")).unwrap();
    assert!(matches!(
        vault.key_for("some-provider", HOST),
        Err(Error::InvalidKey(_))
    ));
}

#[test]
fn a_provider_id_cannot_leave_the_folder() {
    let scratch = Scratch::new("image-vault");
    let vault = Vault::new(&scratch.0);
    for bad in ["", "../x", "a/b", "A", ".hidden", "a..b", "a\\b"] {
        assert!(
            matches!(
                vault.set(bad, HOST, "sk-key"),
                Err(Error::UnknownProvider(_))
            ),
            "{bad:?}"
        );
        assert!(matches!(vault.hosts(bad), Err(Error::UnknownProvider(_))));
        assert!(matches!(
            vault.clear(bad, None),
            Err(Error::UnknownProvider(_))
        ));
    }
}

#[test]
fn a_provider_id_with_a_version_dot_is_a_plain_name() {
    let scratch = Scratch::new("image-vault");
    let vault = Vault::new(&scratch.0);
    vault.set("model-2.1-name", HOST, "sk-key").unwrap();
    assert!(vault.key_for("model-2.1-name", HOST).is_ok());
}

#[cfg(unix)]
#[test]
fn the_key_file_and_its_folders_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("image-vault");
    Vault::new(&scratch.0)
        .set("some-provider", HOST, "sk-key")
        .unwrap();
    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let folder = scratch.path("image-keys");
    assert_eq!(mode(&folder), 0o700);
    assert_eq!(mode(&folder.join("some-provider")), 0o700);
    let file = std::fs::read_dir(folder.join("some-provider"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(mode(&file), 0o600);
}

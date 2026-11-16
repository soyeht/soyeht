use super::*;
use uuid::Uuid;

#[test]
fn a_folder_name_is_lower_case_words_joined_by_hyphens() {
    assert_eq!(folder_name("Spanish reading"), "spanish-reading");
    assert_eq!(
        folder_name("  Tax -- 2026 / receipts  "),
        "tax-2026-receipts"
    );
    assert_eq!(folder_name("Leitura de livros"), "leitura-de-livros");
    assert_eq!(folder_name("../../etc"), "etc");
    assert_eq!(folder_name("Ação física"), "ação-física");
    // A name with nothing to keep still makes a folder.
    assert_eq!(folder_name("???"), FALLBACK_NAME);
    assert_eq!(folder_name(""), FALLBACK_NAME);
}

#[test]
fn a_folder_name_is_cut_and_never_a_device_name_of_windows() {
    let long = folder_name(&"word ".repeat(40));
    assert!(long.chars().count() <= limits::PROJECT_FOLDER_NAME_CHARS);
    assert!(!long.ends_with('-'));
    for device in [
        "con",
        "NUL",
        "com1",
        "LPT9",
        "aux",
        "COM\u{b9}",
        "lpt\u{b3}",
        "com\u{b2}",
    ] {
        assert_eq!(
            folder_name(device),
            format!("{FALLBACK_NAME}-{}", device.to_lowercase()),
            "{device}"
        );
    }
    // A name that only starts like a device, and the two that are a device only with a number.
    for name in ["com10", "console", "com", "lpt", "com0", "auxiliary"] {
        assert_eq!(folder_name(name), name);
    }
}

#[test]
fn a_letter_and_its_accent_are_one_character_however_they_were_typed() {
    let composed = "A\u{e7}\u{e3}o f\u{ed}sica";
    let decomposed = "Ac\u{327}a\u{303}o fi\u{301}sica";
    assert_eq!(folder_name(composed), "a\u{e7}\u{e3}o-f\u{ed}sica");
    assert_eq!(folder_name(decomposed), folder_name(composed));
}

fn directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!("soyeht-projects-{}", Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    path
}

#[test]
fn folders_of_one_name_are_told_apart_and_none_is_reused() {
    let root = directory();
    let first = make_folder(&root, "book").unwrap();
    let second = make_folder(&root, "book").unwrap();
    let third = make_folder(&root, "book").unwrap();
    assert_eq!(first.file_name().unwrap(), "book");
    assert_eq!(second.file_name().unwrap(), "book-2");
    assert_eq!(third.file_name().unwrap(), "book-3");
    // What the person already has there stays as it is.
    std::fs::write(first.join("notes.md"), b"mine").unwrap();
    assert_eq!(
        make_folder(&root, "book").unwrap().file_name().unwrap(),
        "book-4"
    );
    assert_eq!(std::fs::read(first.join("notes.md")).unwrap(), b"mine");
    std::fs::remove_dir_all(&root).unwrap();
}

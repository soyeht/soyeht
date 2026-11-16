use super::{raster_tests::SPEC, synthetic::Spec};
use crate::{LoadError, load_folder};
use soyeht_avatar::{ErrorKind, MAX_DOCUMENT_BYTES};
use soyeht_platform::log::Failure;
use std::{fs, io, path::PathBuf};

/// A package folder written to disk, removed when the test is done.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str, spec: Spec) -> Self {
        let root =
            std::env::temp_dir().join(format!("soyeht-avatar-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(root.join("eyes")).unwrap();
        fs::write(root.join("avatar.json"), spec.document().to_string()).unwrap();
        for file in spec.assets() {
            let (w, h, bytes) = spec.picture_bytes(&file.path).unwrap();
            image::RgbaImage::from_raw(w, h, bytes)
                .unwrap()
                .save(root.join(&file.path))
                .unwrap();
        }
        Self(root)
    }

    fn rewrite_document(&self, edit: impl FnOnce(&mut serde_json::Value)) {
        let path = self.0.join("avatar.json");
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        edit(&mut document);
        fs::write(path, document.to_string()).unwrap();
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn kind(result: Result<crate::Avatar, LoadError>) -> String {
    result
        .err()
        .map_or("loaded".to_owned(), |e| e.failure_kind())
}

#[test]
fn a_package_folder_loads() {
    let folder = Folder::new("loads", SPEC);
    let avatar = load_folder(&folder.0).unwrap();
    assert_eq!(avatar.package().canvas.width, SPEC.side);
    assert_eq!(avatar.package().eyes.len(), 8);
}

#[test]
fn the_rules_of_the_package_decide_what_loads() {
    let folder = Folder::new("rules", SPEC);
    folder.rewrite_document(|d| d["version"] = "0.2".into());
    assert_eq!(kind(load_folder(&folder.0)), "package_unsupportedVersion");

    let folder = Folder::new("missing", SPEC);
    fs::remove_file(folder.0.join("eyes/blink.png")).unwrap();
    assert_eq!(kind(load_folder(&folder.0)), "package_missingAsset");

    let folder = Folder::new("size", SPEC);
    folder.rewrite_document(|d| d["files"][0]["width"] = (SPEC.side + 1).into());
    assert_eq!(kind(load_folder(&folder.0)), "package_assetSize");

    let folder = Folder::new("garbage", SPEC);
    fs::write(folder.0.join("avatar.json"), "not json").unwrap();
    assert_eq!(kind(load_folder(&folder.0)), "package_malformed");
}

#[test]
fn a_picture_that_is_not_a_png_is_refused_before_it_is_decoded() {
    let folder = Folder::new("notpng", SPEC);
    fs::write(folder.0.join("base.png"), vec![7u8; 4000]).unwrap();
    assert_eq!(kind(load_folder(&folder.0)), "package_assetSize");
}

#[test]
fn a_path_that_leaves_the_folder_is_never_opened() {
    let folder = Folder::new("escape", SPEC);
    let outside = folder.0.with_extension("outside.png");
    let (w, h, bytes) = SPEC.picture_bytes("base.png").unwrap();
    image::RgbaImage::from_raw(w, h, bytes)
        .unwrap()
        .save(&outside)
        .unwrap();
    let name = format!("../{}", outside.file_name().unwrap().to_string_lossy());
    folder.rewrite_document(|d| {
        d["base"] = name.clone().into();
        d["files"][0]["path"] = name.into();
    });
    let result = kind(load_folder(&folder.0));
    fs::remove_file(&outside).unwrap();
    assert!(
        ["package_file", "package_missingAsset"].contains(&result.as_str()),
        "{result}"
    );
}

#[test]
fn a_document_over_the_limit_is_not_read() {
    let folder = Folder::new("huge", SPEC);
    fs::write(
        folder.0.join("avatar.json"),
        vec![b' '; MAX_DOCUMENT_BYTES + 1],
    )
    .unwrap();
    assert_eq!(kind(load_folder(&folder.0)), "io_InvalidData");
}

#[test]
fn a_folder_with_no_document_is_an_io_failure() {
    let missing = std::env::temp_dir().join(format!("soyeht-avatar-none-{}", std::process::id()));
    assert!(matches!(
        load_folder(&missing),
        Err(LoadError::Io(io::ErrorKind::NotFound))
    ));
    assert_eq!(ErrorKind::Malformed.as_str(), "malformed");
}

#[cfg(unix)]
mod links {
    use super::*;
    use soyeht_platform::fs::symlink;

    #[test]
    fn a_link_in_the_name_of_a_picture_is_never_followed() {
        let folder = Folder::new("link-name", SPEC);
        let real = folder.0.join("elsewhere.png");
        fs::rename(folder.0.join("base.png"), &real).unwrap();
        symlink(&real, folder.0.join("base.png")).unwrap();
        assert_eq!(kind(load_folder(&folder.0)), "package_missingAsset");
    }

    #[test]
    fn a_link_in_a_folder_of_the_path_is_never_followed() {
        let folder = Folder::new("link-dir", SPEC);
        fs::rename(folder.0.join("eyes"), folder.0.join("real-eyes")).unwrap();
        symlink(folder.0.join("real-eyes"), folder.0.join("eyes")).unwrap();
        assert_eq!(kind(load_folder(&folder.0)), "package_missingAsset");
    }

    #[test]
    fn a_pipe_in_place_of_a_picture_is_refused_and_does_not_block() {
        let folder = Folder::new("pipe", SPEC);
        let pipe = folder.0.join("eyes/blink.png");
        fs::remove_file(&pipe).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap();
        assert!(made.success());
        // It is not a regular file: the load rules are told it is more than a picture may be.
        assert_eq!(kind(load_folder(&folder.0)), "package_tooLarge");
    }

    #[test]
    fn a_link_in_place_of_the_document_is_never_followed() {
        let folder = Folder::new("link-document", SPEC);
        let real = folder.0.join("document.json");
        fs::rename(folder.0.join("avatar.json"), &real).unwrap();
        symlink(&real, folder.0.join("avatar.json")).unwrap();
        assert_eq!(kind(load_folder(&folder.0)), "io_InvalidInput");
    }
}

#[test]
fn a_reserved_device_name_is_refused_by_the_rule_before_anything_is_opened() {
    use soyeht_avatar::plain_png_path;
    for name in ["CON.png", "nul.png", "com1.png", "eyes/LPT9.png", "aux.PNG"] {
        assert!(!plain_png_path(name), "{name}");
    }
    // A package that lists one: refused by the rules, as a bad file name, with nothing opened (a
    // folder with no such file says the same).
    let folder = Folder::new("device", SPEC);
    folder.rewrite_document(|d| {
        d["base"] = "CON.png".into();
        d["files"][0]["path"] = "CON.png".into();
    });
    assert_eq!(kind(load_folder(&folder.0)), "package_file");
}

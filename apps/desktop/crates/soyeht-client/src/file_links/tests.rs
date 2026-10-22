use super::*;
use std::{fs, path::PathBuf};

/// A folder of its own under the system's temporary one, removed when the test ends.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("soyeht-file-links-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, relative: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "text\n").unwrap();
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn reference(path: &str, line: u32, column: Option<u32>) -> Reference {
    Reference {
        path: path.to_owned(),
        line,
        column,
    }
}

#[test]
fn a_reference_is_found_with_the_bytes_it_takes() {
    let text = "né ü src/b.rs:3 and src/a.rs:4:2.";
    let found = find(text);
    assert_eq!(found.len(), 2);
    assert_eq!(&text[found[0].range.clone()], "src/b.rs:3");
    assert_eq!(&text[found[1].range.clone()], "src/a.rs:4:2");
    assert_eq!(found[1].reference, reference("src/a.rs", 3, Some(1)));
}

#[test]
fn a_reference_is_found_by_the_character_it_covers() {
    let characters: Vec<char> = "see a/b.rs:12 now".chars().collect();
    let found = find_at(&characters, 6).unwrap();
    assert_eq!(found.range, 4..13);
    assert_eq!(found.reference, reference("a/b.rs", 11, None));
    assert_eq!(find_at(&characters, 2), None);
    assert_eq!(find_at(&characters, 14), None);
}

#[test]
fn a_label_that_only_looks_like_a_reference_takes_its_characters_with_it() {
    // `../x.rs:1` is refused as a whole; `x.rs:1` inside it is not looked for again.
    assert!(find("../x.rs:1").is_empty());
    assert!(find("a/../b.rs:1").is_empty());
}

#[test]
fn a_very_long_run_is_no_path() {
    let text = format!("{}.rs:1", "a".repeat(5000));
    assert!(find(&text).is_empty());
}

#[test]
fn only_so_many_references_are_found() {
    let text = "a.rs:1 ".repeat(300);
    assert_eq!(find(&text).len(), MAX_REFERENCES);
}

#[test]
fn a_file_of_the_workspace_resolves_to_its_path_from_the_folder() {
    let folder = Folder::new("inside");
    folder.file("src/main.rs");
    let opening = resolve(&folder.0, &reference("src/main.rs", 4, Some(2))).unwrap();
    assert_eq!(
        opening,
        Opening {
            path: "src/main.rs".to_owned(),
            line: 4,
            column: Some(2)
        }
    );
}

#[test]
fn a_file_that_is_not_there_or_is_a_folder_is_not_a_link() {
    let folder = Folder::new("missing");
    folder.file("src/main.rs");
    assert_eq!(
        resolve(&folder.0, &reference("src/other.rs", 0, None)),
        None
    );
    assert_eq!(resolve(&folder.0, &reference("src", 0, None)), None);
    assert_eq!(
        resolve(&folder.0.join("gone"), &reference("a.rs", 0, None)),
        None
    );
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_workspace_is_not_a_link() {
    let outside = Folder::new("outside");
    outside.file("secret.txt");
    let folder = Folder::new("escape");
    folder.file("real.txt");
    std::os::unix::fs::symlink(outside.0.join("secret.txt"), folder.0.join("escape.txt")).unwrap();
    assert_eq!(resolve(&folder.0, &reference("escape.txt", 0, None)), None);
    assert!(resolve(&folder.0, &reference("real.txt", 0, None)).is_some());
}

#[cfg(unix)]
#[test]
fn a_folder_that_is_a_link_out_of_the_workspace_hides_the_files_in_it() {
    let outside = Folder::new("outside-dir");
    outside.file("secret.txt");
    let folder = Folder::new("escape-dir");
    folder.file("real.txt");
    std::os::unix::fs::symlink(&outside.0, folder.0.join("link")).unwrap();
    assert_eq!(
        resolve(&folder.0, &reference("link/secret.txt", 0, None)),
        None
    );
    assert!(resolve(&folder.0, &reference("real.txt", 0, None)).is_some());
}

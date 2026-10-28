use super::*;
use crate::fs::{file_is_owner_only, set_private_dir};
use std::{collections::BTreeSet, sync::Arc, thread};

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "soyeht-platform-atomic-{name}-{}-{}",
        std::process::id(),
        temporary_name()
    ));
    fs::create_dir_all(&directory).unwrap();
    directory
}

/// Every entry of `directory` that is a temporary file of this module.
fn leftovers(directory: &Path) -> Vec<String> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| is_temporary_name(name))
        .map(|name| name.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_file_is_created_and_then_replaced_and_no_temporary_file_stays() {
    let directory = scratch("replace");
    let target = directory.join("state.json");
    write_atomic(&target, b"first", WriteMode::OwnerOnly).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"first");
    write_atomic(&target, b"second, and longer", WriteMode::OwnerOnly).unwrap();
    write_atomic(&target, b"3", WriteMode::KeepPermissions).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"3");
    assert_eq!(leftovers(&directory), Vec::<String>::new());
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn an_owner_only_file_is_owner_only_from_the_moment_it_holds_bytes() {
    let directory = scratch("owner-only-staged");
    set_private_dir(&directory).unwrap();
    let target = directory.join("token");
    let staged = stage_replacement(&target, b"secret", WriteMode::OwnerOnly).unwrap();
    // The temporary file is complete here and the target does not exist yet.
    assert!(file_is_owner_only(staged.file()).unwrap());
    assert!(!target.exists());
    let file = staged.commit().unwrap();
    assert!(file_is_owner_only(&file).unwrap());
    assert!(file_is_owner_only(&File::open(&target).unwrap()).unwrap());
    fs::remove_dir_all(&directory).unwrap();
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn a_world_readable_target_becomes_owner_only() {
        let directory = scratch("widen");
        let target = directory.join("token");
        fs::write(&target, "old").unwrap();
        set_mode(&target, 0o644);
        write_atomic(&target, b"new", WriteMode::OwnerOnly).unwrap();
        assert_eq!(mode(&target), 0o600);
        assert_eq!(fs::read(&target).unwrap(), b"new");
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn the_temporary_file_is_mode_600_while_it_exists() {
        let directory = scratch("temporary-mode");
        let target = directory.join("token");
        let staged = stage_replacement(&target, b"secret", WriteMode::OwnerOnly).unwrap();
        let temporary = leftovers(&directory);
        assert_eq!(temporary.len(), 1);
        assert_eq!(mode(&directory.join(&temporary[0])), 0o600);
        drop(staged);
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn replacing_a_user_file_keeps_its_permissions_even_when_it_is_read_only() {
        let directory = scratch("keep");
        let target = directory.join("notes.txt");
        fs::write(&target, "old").unwrap();
        for kept in [0o640, 0o666, 0o600, 0o444] {
            set_mode(&target, kept);
            write_atomic(&target, b"new", WriteMode::KeepPermissions).unwrap();
            assert_eq!(mode(&target), kept, "mode {kept:o}");
        }
        assert_eq!(fs::read(&target).unwrap(), b"new");
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_new_user_file_gets_the_mode_of_an_ordinary_creation() {
        let directory = scratch("keep-new");
        let ordinary = directory.join("ordinary");
        fs::write(&ordinary, "").unwrap();
        let target = directory.join("fresh.txt");
        write_atomic(&target, b"x", WriteMode::KeepPermissions).unwrap();
        assert_eq!(mode(&target), mode(&ordinary));
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_planted_link_at_the_target_is_replaced_and_what_it_points_at_is_not_written() {
        let directory = scratch("target-link");
        let outside = directory.join("outside");
        fs::write(&outside, "untouched").unwrap();
        let target = directory.join("token");
        std::os::unix::fs::symlink(&outside, &target).unwrap();
        write_atomic(&target, b"secret", WriteMode::OwnerOnly).unwrap();
        assert_eq!(fs::read(&outside).unwrap(), b"untouched");
        assert!(!fs::symlink_metadata(&target).unwrap().is_symlink());
        assert_eq!(fs::read(&target).unwrap(), b"secret");
        fs::remove_dir_all(&directory).unwrap();
    }
}

#[test]
fn a_link_planted_at_a_temporary_name_is_neither_followed_nor_removed() {
    let directory = scratch("planted");
    let outside = directory.join("outside");
    fs::write(&outside, "untouched").unwrap();
    let planted = directory.join(".soyeht-write-planted");
    if crate::fs::symlink(&outside, &planted).is_err() {
        // Windows without the right to create links: nothing to plant.
        fs::remove_dir_all(&directory).unwrap();
        return;
    }
    let mut names = ["planted", "fresh"].map(|tail| format!("{TEMPORARY_PREFIX}{tail}"));
    let mut next = 0;
    let target = directory.join("token");
    let staged = stage_named(&target, b"secret", WriteMode::OwnerOnly, || {
        next += 1;
        std::mem::take(&mut names[next - 1])
    })
    .unwrap();
    staged.commit().unwrap();
    assert_eq!(fs::read(&outside).unwrap(), b"untouched");
    assert_eq!(fs::read(&target).unwrap(), b"secret");
    assert!(fs::symlink_metadata(&planted).unwrap().is_symlink());
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn only_taken_names_are_skipped_and_running_out_of_names_is_an_error() {
    let directory = scratch("exhausted");
    fs::write(directory.join(".soyeht-write-same"), "x").unwrap();
    let target = directory.join("token");
    let error = stage_named(&target, b"secret", WriteMode::OwnerOnly, || {
        format!("{TEMPORARY_PREFIX}same")
    })
    .err()
    .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        fs::read(directory.join(".soyeht-write-same")).unwrap(),
        b"x"
    );
    assert!(!target.exists());
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_failed_rename_leaves_the_target_alone_and_no_temporary_file() {
    let directory = scratch("failed-rename");
    // A directory cannot be replaced by a file, on any system.
    let target = directory.join("occupied");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("inside"), "kept").unwrap();
    assert!(write_atomic(&target, b"bytes", WriteMode::OwnerOnly).is_err());
    assert_eq!(fs::read(target.join("inside")).unwrap(), b"kept");
    assert_eq!(leftovers(&directory), Vec::<String>::new());
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_failure_before_the_rename_leaves_nothing_behind() {
    let directory = scratch("failed-stage");
    let missing = directory.join("no-such-directory/token");
    let error = write_atomic(&missing, b"x", WriteMode::OwnerOnly).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(leftovers(&directory), Vec::<String>::new());
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_staged_write_that_is_dropped_leaves_the_target_and_removes_its_temporary_file() {
    let directory = scratch("dropped");
    let target = directory.join("registry.json");
    fs::write(&target, "old").unwrap();
    let staged = stage_replacement(&target, b"new", WriteMode::OwnerOnly).unwrap();
    assert_eq!(leftovers(&directory).len(), 1);
    drop(staged);
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(leftovers(&directory), Vec::<String>::new());
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_target_without_a_name_is_refused() {
    let error = write_atomic(Path::new(""), b"x", WriteMode::OwnerOnly).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn writers_of_one_target_never_share_a_temporary_name() {
    let names = Arc::new(std::sync::Mutex::new(BTreeSet::new()));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let names = Arc::clone(&names);
            thread::spawn(move || {
                for _ in 0..500 {
                    assert!(names.lock().unwrap().insert(temporary_name()));
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(names.lock().unwrap().len(), 4000);
}

#[test]
fn concurrent_writers_leave_one_complete_version_and_no_temporary_file() {
    let directory = scratch("concurrent");
    let target = Arc::new(directory.join("shared.json"));
    let workers: Vec<_> = (0..8_u8)
        .map(|id| {
            let target = Arc::clone(&target);
            thread::spawn(move || {
                let bytes = vec![b'a' + id; 64 * 1024];
                for _ in 0..25 {
                    write_atomic(&target, &bytes, WriteMode::OwnerOnly).unwrap();
                    let seen = fs::read(&*target).unwrap();
                    // Whole, and one writer's: never a mix.
                    assert_eq!(seen.len(), bytes.len());
                    assert!(seen.iter().all(|byte| *byte == seen[0]));
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(leftovers(&directory), Vec::<String>::new());
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn the_name_rule_matches_only_this_module_s_temporary_files() {
    assert!(is_temporary_name(OsStr::new(&temporary_name())));
    for ordinary in ["note.txt", ".soyeht-saved", ".gitignore", "soyeht-write-x"] {
        assert!(!is_temporary_name(OsStr::new(ordinary)), "{ordinary}");
    }
}

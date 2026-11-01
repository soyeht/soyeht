use super::*;
use crate::fs::{set_private_dir, symlink};

use crate::testing::scratch_dir as scratch;

#[test]
fn a_missing_directory_is_made_with_its_parents_and_is_owner_only() {
    let root = scratch("create");
    let leaf = root.join("a").join("b").join("c");
    ensure_private_dir(&leaf).unwrap();
    assert!(leaf.is_dir());
    assert!(dir_is_owner_only(&leaf).unwrap());
    check_private_dir(&leaf).unwrap();
    // Again: nothing to do.
    ensure_private_dir(&leaf).unwrap();
    fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn a_directory_others_can_reach_is_made_owner_only_by_ensure_and_refused_by_check() {
    use std::os::unix::fs::PermissionsExt;

    let root = scratch("repair");
    let open = root.join("open");
    fs::create_dir(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        check_private_dir(&open),
        Err(PrivateDirError::NotPrivate)
    ));
    ensure_private_dir(&open).unwrap();
    assert_eq!(
        fs::metadata(&open).unwrap().permissions().mode() & 0o777,
        0o700
    );
    fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn a_directory_of_another_user_is_said_to_belong_to_them_not_refused_with_permission_denied() {
    // The root of the file system belongs to the superuser. Nothing is changed: it is only read.
    // SAFETY: geteuid has no arguments or memory preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    for result in [
        check_private_dir(Path::new("/")),
        ensure_private_dir(Path::new("/")),
    ] {
        let error = result.unwrap_err();
        assert!(matches!(error, PrivateDirError::OtherOwner), "{error}");
        assert_eq!(error.to_string(), "the directory belongs to another user");
        assert_eq!(
            io::Error::from(error).to_string(),
            "the directory belongs to another user"
        );
    }
}

#[test]
fn a_link_is_refused_whether_it_points_at_a_directory_or_nowhere() {
    let root = scratch("link");
    let real = root.join("real");
    fs::create_dir(&real).unwrap();
    set_private_dir(&real).unwrap();
    for target in [real.clone(), root.join("nowhere")] {
        let link = root.join("link");
        drop(fs::remove_file(&link));
        if symlink(&target, &link).is_err() {
            // Windows without Developer Mode cannot make one; there is nothing to refuse.
            continue;
        }
        assert!(matches!(
            ensure_private_dir(&link),
            Err(PrivateDirError::Unsafe)
        ));
        assert!(matches!(
            check_private_dir(&link),
            Err(PrivateDirError::Unsafe)
        ));
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_file_where_the_directory_belongs_is_refused_and_left_alone() {
    let root = scratch("file");
    let file = root.join("file");
    fs::write(&file, "keep").unwrap();
    assert!(matches!(
        ensure_private_dir(&file),
        Err(PrivateDirError::Unsafe)
    ));
    assert_eq!(fs::read(&file).unwrap(), b"keep");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_errors_convert_to_io_errors_by_what_they_say() {
    assert_eq!(
        io::Error::from(PrivateDirError::Unsafe).kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        io::Error::from(PrivateDirError::NotPrivate).kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        io::Error::from(PrivateDirError::Io(io::ErrorKind::NotFound.into())).kind(),
        io::ErrorKind::NotFound
    );
}

#[cfg(unix)]
#[test]
fn the_repair_never_changes_what_a_link_points_at() {
    use std::os::unix::fs::PermissionsExt;

    let root = scratch("repair-link");
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    let link = root.join("link");
    symlink(&target, &link).unwrap();
    // The repair itself, as it runs after a check that a swap can outdate.
    let error = system::set_private_dir_no_follow(&link).unwrap_err();
    assert!(system::is_link_or_not_directory(&error), "{error:?}");
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o755
    );
    // And on a real directory it sets the mode.
    system::set_private_dir_no_follow(&target).unwrap();
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o700
    );
    fs::remove_dir_all(&root).unwrap();
}

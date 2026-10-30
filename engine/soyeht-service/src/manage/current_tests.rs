//! Whether a registration is the one `install` would write now, and what a repair that changes
//! nothing leaves alone (contract 13, section 3).

use super::{
    tests::{BOTH, installed, programs},
    *,
};
use crate::{
    commands::fake::{Recorder, ok},
    layout::test_layout,
    testing::Scratch,
};

#[test]
fn a_registration_is_current_for_the_programs_it_was_written_for_only() {
    for system in [System::Macos, System::Linux, System::Windows] {
        let fake = Recorder::answering(|_| ok("state = running"));
        let (_scratch, layout) = installed(system, "current", &fake, &BOTH);
        let manager = Manager::new(&layout, &fake);
        for component in BOTH {
            assert!(
                manager.is_current(&programs(), component).unwrap(),
                "{system:?}"
            );
            // The same programs in another folder (the app was moved) are not current.
            let moved = programs().join("moved");
            assert!(
                !manager.is_current(&moved, component).unwrap(),
                "{system:?}"
            );
        }
    }
}

#[test]
fn nothing_registered_and_an_edited_file_are_not_current() {
    let scratch = Scratch::new("current-none");
    let layout = test_layout(System::Macos, &scratch.0);
    let fake = Recorder::answering(|_| ok("state = running"));
    let manager = Manager::new(&layout, &fake);
    assert!(!manager.is_current(&programs(), Component::Engine).unwrap());
    manager.install(&programs(), &[Component::Engine]).unwrap();
    assert!(manager.is_current(&programs(), Component::Engine).unwrap());
    let file = layout.definition_file(Component::Engine);
    let mut text = fs::read_to_string(&file).unwrap();
    text.push('\n');
    fs::write(&file, text).unwrap();
    assert!(!manager.is_current(&programs(), Component::Engine).unwrap());
}

#[cfg(unix)]
#[test]
fn a_definition_that_is_already_right_is_not_written_again() {
    use std::os::unix::fs::MetadataExt as _;
    let scratch = Scratch::new("write-same");
    let file = scratch.0.join("agents/a.plist");
    write_definition(&file, b"one").unwrap();
    let inode = fs::metadata(&file).unwrap().ino();
    // A write replaces the file by renaming a new one over it, which gives it a new inode.
    write_definition(&file, b"one").unwrap();
    assert_eq!(fs::metadata(&file).unwrap().ino(), inode);
    write_definition(&file, b"two").unwrap();
    assert_ne!(fs::metadata(&file).unwrap().ino(), inode);
    assert_eq!(fs::read(&file).unwrap(), b"two");
}

#[cfg(unix)]
#[test]
fn a_link_with_the_right_text_is_still_replaced_by_a_file() {
    let scratch = Scratch::new("write-link");
    let target = scratch.0.join("target");
    fs::write(&target, b"one").unwrap();
    let file = scratch.0.join("a.plist");
    std::os::unix::fs::symlink(&target, &file).unwrap();
    write_definition(&file, b"one").unwrap();
    assert!(
        !fs::symlink_metadata(&file)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

use super::*;
use soyeht_platform::fs::{dir_is_owner_only, file_is_owner_only};

/// A folder of its own under the system's temporary one, removed when it is dropped.
pub(crate) struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> Self {
        Self(std::env::temp_dir().join(format!("soyeht-recorder-test-{}", uuid::Uuid::new_v4())))
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
        }
    }
}

pub(crate) const DAY_ONE: u64 = 1_793_000_000_000;
const DAY_TWO: u64 = DAY_ONE + 86_400_000;

fn limits(file_bytes: u64, keep: usize, total_bytes: u64) -> Limits {
    Limits {
        file_bytes,
        keep,
        total_bytes,
    }
}

fn names(directory: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

fn put(store: &mut Store, pane: &str, wall_ms: u64, line: &str) -> Option<u64> {
    let segment = store.prepare(pane, wall_ms, line.len() + 1).unwrap()?;
    store.write(pane, line).unwrap();
    Some(segment)
}

#[test]
fn recordings_go_to_a_dated_folder_and_the_folders_and_files_are_owner_only() {
    let scratch = Scratch::new();
    let root = scratch.join("screens");
    let mut store = Store::new(root.clone(), limits(1 << 20, 3, 1 << 30)).unwrap();
    put(&mut store, "pane-1", DAY_ONE, "{}");
    let day = root.join(day_of(DAY_ONE));
    assert_eq!(names(&day), ["pane-1.jsonl"]);
    assert!(dir_is_owner_only(&root).unwrap());
    assert!(dir_is_owner_only(&day).unwrap());
    let file = File::open(day.join("pane-1.jsonl")).unwrap();
    assert!(file_is_owner_only(&file).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(&day), 0o700);
        assert_eq!(mode(&day.join("pane-1.jsonl")), 0o600);
    }
}

#[cfg(unix)]
#[test]
fn a_folder_that_others_can_read_is_made_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    fs::create_dir_all(scratch.join("screens")).unwrap();
    fs::set_permissions(scratch.join("screens"), fs::Permissions::from_mode(0o755)).unwrap();
    Store::new(scratch.join("screens"), limits(1 << 20, 3, 1 << 30)).unwrap();
    assert!(dir_is_owner_only(&scratch.join("screens")).unwrap());
}

#[test]
fn a_pane_id_cannot_name_a_file_outside_the_folder() {
    let scratch = Scratch::new();
    let root = scratch.join("screens");
    let mut store = Store::new(root.clone(), limits(1 << 20, 3, 1 << 30)).unwrap();
    put(&mut store, "../../escape", DAY_ONE, "{}");
    assert_eq!(names(&root.join(day_of(DAY_ONE))), ["______escape.jsonl"]);
    assert_eq!(names(&root), [day_of(DAY_ONE)]);
}

#[test]
fn a_file_that_reaches_its_cap_is_rotated_and_only_the_newest_rotations_are_kept() {
    let scratch = Scratch::new();
    let root = scratch.join("screens");
    let mut store = Store::new(root.clone(), limits(300, 2, 1 << 30)).unwrap();
    let line = "x".repeat(99);
    let mut segments = Vec::new();
    for _ in 0..40 {
        segments.push(put(&mut store, "p", DAY_ONE, &line).unwrap());
    }
    let day = root.join(day_of(DAY_ONE));
    assert_eq!(names(&day), ["p.12.jsonl", "p.13.jsonl", "p.jsonl"]);
    for name in names(&day) {
        let size = fs::metadata(day.join(&name)).unwrap().len();
        assert!(size <= 300, "{name} holds {size} bytes");
    }
    // Each rotation is a new file, which the recorder starts anew; the id changes only then.
    segments.dedup();
    assert_eq!(segments.len(), 14);
    // What was deleted is no longer counted against the run.
    assert_eq!(store.total, 2 * 300 + 100);
}

#[test]
fn a_run_that_reaches_its_total_cap_drops_records_and_counts_them() {
    let scratch = Scratch::new();
    let root = scratch.join("screens");
    let mut store = Store::new(root.clone(), limits(1 << 20, 3, 1000)).unwrap();
    let line = "y".repeat(99);
    let mut written = 0;
    for _ in 0..30 {
        if put(&mut store, "a", DAY_ONE, &line).is_some() {
            written += 1;
        }
    }
    assert_eq!(written, 10);
    assert_eq!(store.dropped(), 20);
    // Another pane is held to the same total.
    assert_eq!(put(&mut store, "b", DAY_ONE, &line), None);
    let size = fs::metadata(root.join(day_of(DAY_ONE)).join("a.jsonl"))
        .unwrap()
        .len();
    assert_eq!(size, 1000);
}

#[test]
fn a_new_day_starts_a_new_folder_and_a_file_of_an_earlier_run_is_put_aside() {
    let scratch = Scratch::new();
    let root = scratch.join("screens");
    let mut first = Store::new(root.clone(), limits(1 << 20, 3, 1 << 30)).unwrap();
    let before = put(&mut first, "p", DAY_ONE, "one").unwrap();
    let after = put(&mut first, "p", DAY_TWO, "two").unwrap();
    assert_ne!(before, after);
    assert_eq!(names(&root), [day_of(DAY_ONE), day_of(DAY_TWO)]);

    let mut second = Store::new(root.clone(), limits(1 << 20, 3, 1 << 30)).unwrap();
    put(&mut second, "p", DAY_ONE, "three");
    let day = root.join(day_of(DAY_ONE));
    assert_eq!(names(&day), ["p.1.jsonl", "p.jsonl"]);
    assert_eq!(fs::read_to_string(day.join("p.1.jsonl")).unwrap(), "one\n");
    assert_eq!(fs::read_to_string(day.join("p.jsonl")).unwrap(), "three\n");
}

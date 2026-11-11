//! What the window learned about references, so that drawing a reply does not ask the disk for the
//! same file on every frame: the answers and how long each is trusted, and which files are being
//! asked about now. The asking itself (`resolve`) blocks, and the window runs it off its thread.

use super::{Opening, Reference};
use crate::bounded_map::BoundedMap;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// How long the answer for a file that is there is kept.
const FOUND_FOR: Duration = Duration::from_secs(30);
/// How long the answer for a file that is not there is kept: an agent may be writing it.
const MISSING_FOR: Duration = Duration::from_secs(2);

/// A file in a workspace: the folder and the path from it.
pub type Key = (PathBuf, String);

/// What the window learned about references, so that drawing a reply does not ask the disk for
/// the same file on every frame. The answer is the file's path from the workspace's folder, or
/// `None` for a file that is not in the workspace.
#[derive(Default)]
pub struct Probes {
    answers: BoundedMap<Key, (Instant, Option<String>)>,
    /// The files being asked for now, so that a frame does not ask twice.
    asking: HashSet<Key>,
}

/// What the window knows about a reference.
pub struct Known {
    pub opening: Option<Opening>,
    /// Whether the answer is recent enough to be used without asking again.
    pub fresh: bool,
}

impl Probes {
    /// What is known of `reference` in the workspace at `root`, as of `now`. A file never asked
    /// about is not there as far as the window knows.
    pub fn known(&self, (root, reference): (&Path, &Reference), now: Instant) -> Known {
        let answer = self
            .answers
            .get(&(root.to_path_buf(), reference.path.clone()));
        let fresh = answer.is_some_and(|(at, path)| {
            let keeps = if path.is_some() {
                FOUND_FOR
            } else {
                MISSING_FOR
            };
            now.saturating_duration_since(*at) < keeps
        });
        // The cache is by file; the place in it is this reference's.
        let opening = answer
            .and_then(|(_, path)| path.as_ref())
            .map(|path| Opening {
                path: path.clone(),
                line: reference.line,
                column: reference.column,
            });
        Known { opening, fresh }
    }

    /// Whether the caller should ask the disk about `key`: no one is doing it already.
    pub fn begin(&mut self, key: &Key) -> bool {
        self.asking.insert(key.clone())
    }

    /// Keeps what the disk said about `key` when the ask began at `asked`, and reports whether it
    /// differs from what was shown. An answer to an ask that began before the one kept is dropped:
    /// two asks can be in flight, and the older read must never win.
    pub fn record(&mut self, key: Key, asked: Instant, path: Option<String>) -> bool {
        self.asking.remove(&key);
        let kept = self.answers.get(&key);
        if kept.is_some_and(|(at, _)| *at > asked) {
            return false;
        }
        let changed = kept.and_then(|(_, path)| path.as_ref()) != path.as_ref();
        self.answers.insert(key, (asked, path));
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(line: u32) -> Reference {
        Reference {
            path: "a/b.rs".to_owned(),
            line,
            column: None,
        }
    }

    fn key() -> Key {
        (PathBuf::from("/w"), "a/b.rs".to_owned())
    }

    fn known(probes: &Probes, line: u32, now: Instant) -> Known {
        probes.known((Path::new("/w"), &reference(line)), now)
    }

    #[test]
    fn a_file_never_asked_about_is_unknown_and_not_there() {
        let known = known(&Probes::default(), 1, Instant::now());
        assert!(!known.fresh);
        assert!(known.opening.is_none());
    }

    #[test]
    fn an_answer_is_kept_by_file_and_gives_each_reference_its_own_place() {
        let mut probes = Probes::default();
        let start = Instant::now();
        probes.record(key(), start, Some("a/b.rs".to_owned()));
        let (first, second) = (known(&probes, 4, start), known(&probes, 9, start));
        assert!(first.fresh && second.fresh);
        assert_eq!(first.opening.unwrap().line, 4);
        assert_eq!(second.opening.unwrap().line, 9);
    }

    #[test]
    fn a_missing_file_is_asked_for_again_sooner_than_one_that_is_there() {
        let mut probes = Probes::default();
        let start = Instant::now();
        probes.record(key(), start, None);
        assert!(known(&probes, 1, start + MISSING_FOR / 2).fresh);
        let later = start + MISSING_FOR + Duration::from_millis(1);
        assert!(!known(&probes, 1, later).fresh);
        probes.record(key(), later, Some("a/b.rs".to_owned()));
        let much_later = later + FOUND_FOR / 2;
        let kept = known(&probes, 1, much_later);
        assert!(kept.fresh && kept.opening.is_some());
        assert!(!known(&probes, 1, later + FOUND_FOR).fresh);
    }

    #[test]
    fn a_file_is_asked_about_once_at_a_time_and_a_change_is_reported_once() {
        let mut probes = Probes::default();
        let now = Instant::now();
        assert!(probes.begin(&key()));
        assert!(!probes.begin(&key()), "already being asked");
        // Unknown counts as not there, so a first answer of "not there" changes nothing.
        assert!(!probes.record(key(), now, None));
        assert!(probes.begin(&key()), "the answer ended the asking");
        assert!(probes.record(key(), now, Some("a/b.rs".to_owned())));
        assert!(!probes.record(key(), now, Some("a/b.rs".to_owned())));
        assert!(probes.record(key(), now, None));
    }

    #[test]
    fn an_answer_to_an_older_ask_never_replaces_the_one_of_a_newer_ask() {
        let mut probes = Probes::default();
        let (older, newer) = (Instant::now(), Instant::now() + Duration::from_millis(5));
        // The newer ask finishes first and says the file is there; the older one then arrives.
        assert!(probes.record(key(), newer, Some("a/b.rs".to_owned())));
        assert!(!probes.record(key(), older, None));
        let kept = probes.known((Path::new("/w"), &reference(1)), newer);
        assert!(kept.opening.is_some(), "the older read won");
        // An ask that began later replaces it, and so does one that began at the same moment.
        assert!(probes.record(key(), newer, None));
        assert!(known(&probes, 1, newer).opening.is_none());
    }

    #[test]
    fn answers_are_kept_in_the_shared_bounded_map() {
        let mut probes = Probes::default();
        let now = Instant::now();
        let file = |index: usize| (PathBuf::from("/w"), format!("f{index}.rs"));
        for index in 0..=crate::bounded_map::CACHE_ENTRIES {
            probes.record(file(index), now, None);
        }
        assert!(probes.answers.get(&file(0)).is_none());
        assert!(probes.answers.get(&file(1)).is_some());
    }
}

//! `path:line` references in the window: a click on one in terminal output or in an agent's reply
//! opens the file in the editor at that line. What a reference is lives in
//! `soyeht_client::file_links` (the Mac's rule, `contracts/vectors/file-paths.json`); this asks
//! whether the file is in the pane's workspace, so that text that only looks like a reference is
//! not drawn as a link, and opens the ones that are.

use crate::{destination::Destination, workbench::Workbench};
use gpui::{AsyncApp, Context, WeakEntity};
use soyeht_client::{
    bounded_map::BoundedMap,
    file_links::{self, Opening, Reference},
};
use soyeht_protocol::TextPosition;
use std::{
    cell::RefCell,
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

/// How long the answer for a file that is there is kept.
const FOUND_FOR: Duration = Duration::from_secs(30);
/// How long the answer for a file that is not there is kept: an agent may be writing it.
const MISSING_FOR: Duration = Duration::from_secs(2);

/// A file in a workspace: the folder and the path from it.
type Key = (PathBuf, String);

/// What the window learned about references, so that drawing a reply does not ask the disk for
/// the same file on every frame. The answer is the file's path from the workspace's folder, or
/// `None` for a file that is not in the workspace.
#[derive(Default)]
pub(crate) struct Probes {
    answers: BoundedMap<Key, (Instant, Option<String>)>,
    /// The files being asked for now, so that a frame does not ask twice.
    asking: HashSet<Key>,
}

/// What the window knows about a reference.
struct Known {
    opening: Option<Opening>,
    /// Whether the answer is recent enough to be used without asking again.
    fresh: bool,
}

impl Probes {
    /// What is known of `reference` in the workspace at `root`, as of `now`. A file never asked
    /// about is not there as far as the window knows.
    fn known(&self, (root, reference): (&Path, &Reference), now: Instant) -> Known {
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
    fn begin(&mut self, key: &Key) -> bool {
        self.asking.insert(key.clone())
    }

    /// Keeps what the disk said about `key` when the ask began at `asked`, and reports whether it
    /// differs from what was shown. An answer to an ask that began before the one kept is dropped:
    /// two asks can be in flight, and the older read must never win.
    fn record(&mut self, key: Key, asked: Instant, path: Option<String>) -> bool {
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

/// What the disk said about a reference, once it has said it.
pub(crate) struct Probed {
    pub opening: Option<Opening>,
}

/// What a reply needs to draw the references in it and open them: the window, the workspace the
/// pane belongs to and its folder. This is the one way to resolve a reference, and the disk is
/// asked off the UI thread (`soyeht_client::file_links::resolve` does blocking IO).
#[derive(Clone)]
pub(crate) struct Files {
    workbench: WeakEntity<Workbench>,
    app: AsyncApp,
    workspace_id: String,
    root: PathBuf,
    probes: Rc<RefCell<Probes>>,
}

/// A reference that points at a file of the workspace, ready to open.
#[derive(Clone)]
pub(crate) struct OpenFile {
    workbench: WeakEntity<Workbench>,
    workspace_id: String,
    opening: Opening,
}

impl Files {
    /// Where `reference` points, when the file is in the workspace, from what the window learned
    /// a moment ago: for drawing and hovering, which happen on every frame. An answer that is
    /// missing or old is asked for in the background and the window is drawn again if it
    /// changed, so a reference becomes a link a moment after it appears.
    pub fn resolve(&self, reference: &Reference) -> Option<OpenFile> {
        let known = self.known(reference);
        if !known.fresh && self.probes.borrow_mut().begin(&self.key(reference)) {
            self.check(reference, |_, _, _| {});
        }
        Some(OpenFile {
            workbench: self.workbench.clone(),
            workspace_id: self.workspace_id.clone(),
            opening: known.opening?,
        })
    }

    /// Whether the window knows that the file is not in the workspace.
    pub fn known_missing(&self, reference: &Reference) -> bool {
        let known = self.known(reference);
        known.fresh && known.opening.is_none()
    }

    /// Asks the disk, off the UI thread, whether the file `reference` names is in the workspace,
    /// keeps the answer, draws the window again when it changed, and hands it to `then`.
    pub fn check(
        &self,
        reference: &Reference,
        then: impl FnOnce(&mut Workbench, Probed, &mut Context<Workbench>) + 'static,
    ) {
        let (workbench, probes) = (self.workbench.clone(), self.probes.clone());
        let (root, reference) = (self.root.clone(), reference.clone());
        let asked_at = Instant::now();
        self.app
            .spawn(async move |cx| {
                let (asked_root, asked) = (root.clone(), reference.clone());
                let found = cx
                    .background_executor()
                    .spawn(async move { file_links::resolve(&asked_root, &asked) })
                    .await;
                let answered = workbench.update(cx, |this, cx| {
                    let changed = probes.borrow_mut().record(
                        (root, reference.path),
                        asked_at,
                        found.as_ref().map(|found| found.path.clone()),
                    );
                    if changed {
                        cx.notify();
                    }
                    then(this, Probed { opening: found }, cx);
                });
                // The window closed while the disk was answering: nothing is left to tell.
                if answered.is_err() {
                    soyeht_platform::log::event("desktop.file_open_window_closed", &[]);
                }
            })
            .detach();
    }

    /// Opens the file `reference` names, if the disk says it is in the workspace.
    pub fn open(&self, reference: &Reference) {
        let workspace_id = self.workspace_id.clone();
        self.check(reference, move |this, probed, cx| {
            if let Some(opening) = probed.opening {
                this.open_reference(&workspace_id, opening, cx);
            }
        });
    }

    fn known(&self, reference: &Reference) -> Known {
        let probes = self.probes.borrow();
        probes.known((&self.root, reference), Instant::now())
    }

    fn key(&self, reference: &Reference) -> Key {
        (self.root.clone(), reference.path.clone())
    }
}

impl OpenFile {
    pub fn open(&self, cx: &mut gpui::App) {
        let (workspace_id, opening) = (self.workspace_id.clone(), self.opening.clone());
        // The window closed since it was drawn: there is nowhere to open it.
        if self
            .workbench
            .update(cx, |this, cx| {
                this.open_reference(&workspace_id, opening, cx)
            })
            .is_err()
        {
            soyeht_platform::log::event("desktop.file_open_window_closed", &[]);
        }
    }
}

impl Workbench {
    /// The workspace a pane belongs to and its folder.
    pub(crate) fn pane_root(&self, pane_id: &str) -> Option<(String, PathBuf)> {
        let workspace = self
            .state
            .workspace(&self.state.pane(pane_id)?.workspace_id)?;
        Some((
            workspace.workspace_id.clone(),
            PathBuf::from(&workspace.path),
        ))
    }

    /// The references of the pane's text, which resolve in its workspace.
    pub(crate) fn files_of(
        &self,
        pane_id: &str,
        probes: Rc<RefCell<Probes>>,
        cx: &mut Context<Self>,
    ) -> Option<Files> {
        let (workspace_id, root) = self.pane_root(pane_id)?;
        Some(Files {
            workbench: cx.entity().downgrade(),
            app: cx.to_async(),
            workspace_id,
            root,
            probes,
        })
    }

    /// Opens a file of the workspace in the editor with the caret at the reference's place.
    pub(crate) fn open_reference(
        &mut self,
        workspace_id: &str,
        opening: Opening,
        cx: &mut Context<Self>,
    ) {
        self.state.select_workspace(workspace_id);
        let at = TextPosition {
            line: opening.line,
            column: opening.column.unwrap_or(0),
        };
        let effects = self.state.editor_open_at(workspace_id, &opening.path, at);
        self.select_destination(Destination::Editor, cx);
        self.run_effects(effects);
        cx.notify();
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
        for index in 0..=soyeht_client::bounded_map::CACHE_ENTRIES {
            probes.record(file(index), now, None);
        }
        assert!(probes.answers.get(&file(0)).is_none());
        assert!(probes.answers.get(&file(1)).is_some());
    }
}

//! `path:line` references in the window: a click on one in terminal output or in an agent's reply
//! opens the file in the editor at that line. What a reference is lives in
//! `soyeht_client::file_links` (the Mac's rule, `contracts/vectors/file-paths.json`); this asks
//! whether the file is in the pane's workspace, so that text that only looks like a reference is
//! not drawn as a link, and opens the ones that are.

use crate::{destination::Destination, workbench::Workbench};
use gpui::{Context, WeakEntity};
use soyeht_client::file_links::{self, Opening, Reference};
use soyeht_protocol::TextPosition;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

/// How long the answer for a file that is there is kept.
const FOUND_FOR: Duration = Duration::from_secs(30);
/// How long the answer for a file that is not there is kept: an agent may be writing it.
const MISSING_FOR: Duration = Duration::from_secs(2);
/// The most answers kept; past it the oldest goes.
const KEPT: usize = 1024;

/// What the window learned about references, so that drawing a reply does not ask the disk for
/// the same file on every frame.
#[derive(Default)]
pub(crate) struct Probes {
    answers: HashMap<(PathBuf, String), (Instant, Option<Opening>)>,
}

impl Probes {
    /// Where `reference` points in the workspace at `root`, as of `now`; `ask` asks the disk and
    /// is called only when the last answer is old.
    pub(crate) fn resolve(
        &mut self,
        (root, reference): (&Path, &Reference),
        now: Instant,
        ask: impl FnOnce() -> Option<Opening>,
    ) -> Option<Opening> {
        let key = (root.to_path_buf(), reference.path.clone());
        let fresh = self.answers.get(&key).filter(|(at, found)| {
            let keeps = if found.is_some() {
                FOUND_FOR
            } else {
                MISSING_FOR
            };
            now.saturating_duration_since(*at) < keeps
        });
        let found = match fresh {
            Some((_, found)) => found.clone(),
            None => {
                if self.answers.len() >= KEPT && !self.answers.contains_key(&key) {
                    // The oldest answer makes room: what is on screen now is the newest.
                    let oldest = self
                        .answers
                        .iter()
                        .min_by_key(|(_, (at, _))| *at)
                        .map(|(key, _)| key.clone());
                    if let Some(oldest) = oldest {
                        self.answers.remove(&oldest);
                    }
                }
                let found = ask();
                self.answers.insert(key, (now, found.clone()));
                found
            }
        };
        // The cache is by file; the place in it is this reference's.
        found.map(|opening| Opening {
            line: reference.line,
            column: reference.column,
            ..opening
        })
    }
}

/// What a reply needs to draw the references in it and open them: the window, the workspace the
/// pane belongs to and its folder.
#[derive(Clone)]
pub(crate) struct Files {
    workbench: WeakEntity<Workbench>,
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
    /// Where `reference` points, when the file is in the workspace.
    pub fn resolve(&self, reference: &Reference) -> Option<OpenFile> {
        let opening =
            self.probes
                .borrow_mut()
                .resolve((&self.root, reference), Instant::now(), || {
                    file_links::resolve(&self.root, reference)
                })?;
        Some(OpenFile {
            workbench: self.workbench.clone(),
            workspace_id: self.workspace_id.clone(),
            opening,
        })
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

    /// Where `reference` points in the workspace at `root`, from what the window learned a moment
    /// ago: for drawing and hovering, which happen on every frame. A click asks the disk itself.
    pub(crate) fn resolve_shown(&self, root: &Path, reference: &Reference) -> Option<Opening> {
        self.markdown
            .probes
            .borrow_mut()
            .resolve((root, reference), Instant::now(), || {
                file_links::resolve(root, reference)
            })
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

    fn opening() -> Option<Opening> {
        Some(Opening {
            path: "a/b.rs".to_owned(),
            line: 0,
            column: None,
        })
    }

    #[test]
    fn a_file_is_asked_for_once_while_the_answer_is_fresh_and_keeps_each_references_place() {
        let mut probes = Probes::default();
        let (root, start) = (Path::new("/w"), Instant::now());
        let first = probes.resolve((root, &reference(4)), start, opening);
        assert_eq!(first.unwrap().line, 4);
        let second = probes.resolve((root, &reference(9)), start, || panic!("asked again"));
        assert_eq!(second.unwrap().line, 9);
    }

    #[test]
    fn a_missing_file_is_asked_for_again_sooner_than_one_that_is_there() {
        let mut probes = Probes::default();
        let (root, start) = (Path::new("/w"), Instant::now());
        assert!(
            probes
                .resolve((root, &reference(1)), start, || None)
                .is_none()
        );
        let soon = start + MISSING_FOR / 2;
        assert!(
            probes
                .resolve((root, &reference(1)), soon, || panic!("asked"))
                .is_none()
        );
        let later = start + MISSING_FOR + Duration::from_millis(1);
        assert!(
            probes
                .resolve((root, &reference(1)), later, opening)
                .is_some()
        );
        let much_later = later + FOUND_FOR / 2;
        assert!(
            probes
                .resolve((root, &reference(1)), much_later, || panic!("asked"))
                .is_some()
        );
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn a_full_cache_drops_its_oldest_answer_and_keeps_the_rest() {
        let mut probes = Probes::default();
        let root = Path::new("/w");
        let start = Instant::now();
        let at = |index: usize| start + Duration::from_millis(index as u64);
        let reference = |index: usize| Reference {
            path: format!("f{index}.rs"),
            line: 0,
            column: None,
        };
        for index in 0..KEPT {
            probes.resolve((root, &reference(index)), at(index), || None);
        }
        assert_eq!(probes.answers.len(), KEPT);
        probes.resolve((root, &reference(KEPT)), at(KEPT), || None);
        assert_eq!(probes.answers.len(), KEPT);
        let key = |index: usize| (root.to_path_buf(), reference(index).path);
        assert!(!probes.answers.contains_key(&key(0)));
        assert!(probes.answers.contains_key(&key(1)));
        assert!(probes.answers.contains_key(&key(KEPT)));
        // A refresh of an answer that is kept does not push another out.
        probes.resolve(
            (root, &reference(1)),
            at(KEPT + MISSING_FOR.as_millis() as usize + 1),
            || None,
        );
        assert_eq!(probes.answers.len(), KEPT);
        assert!(probes.answers.contains_key(&key(2)));
    }
}

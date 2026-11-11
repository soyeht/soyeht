//! `path:line` references in the window: a click on one in terminal output or in an agent's reply
//! opens the file in the editor at that line. What a reference is lives in
//! `soyeht_client::file_links` (the Mac's rule, `contracts/vectors/file-paths.json`); this asks
//! whether the file is in the pane's workspace, so that text that only looks like a reference is
//! not drawn as a link, and opens the ones that are.

use crate::{destination::Destination, workbench::Workbench};
use gpui::{AsyncApp, Context, WeakEntity};
use soyeht_client::file_links::{
    self, Opening, Reference,
    probes::{Key, Known, Probes},
};
use soyeht_protocol::TextPosition;
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Instant};

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

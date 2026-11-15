//! A project file shown as a page (contract 11, section 5b; ADR 0166, section 9).
//!
//! `web.open {path}` names a file of the workspace. The engine serves it, and the files below its
//! directory, read-only on the loopback interface for the life of the pane, and the pane shows
//! the address like any other page. The rules that keep a local file from widening what a page can
//! read are `soyeht_web::project_page`'s (the token, the host, the policy of every answer) and
//! [`serve`]'s (the files below the root, re-checked on every request). A change under the
//! directory reloads the pane's page once the burst settles ([`watch`]).

mod serve;
#[cfg(test)]
mod tests;
mod watch;

use crate::errors::{self, Result};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::{sync::Notify, task::JoinHandle};
use uuid::Uuid;

/// The file a `web.open` names, resolved: canonical, inside the workspace, and a regular file.
pub(crate) struct ProjectFile {
    /// The file, canonical.
    file: PathBuf,
    /// Its directory: what the page may read.
    root: PathBuf,
    /// Its name, which the address ends with.
    name: String,
}

impl ProjectFile {
    /// The file `requested` names inside `workspace_root`. An absolute path, a part that climbs
    /// or starts with `.`, a link that leads out of the folder, a file that is not there and
    /// anything that is not a regular file are refused.
    pub(crate) fn resolve(workspace_root: &Path, requested: &str) -> Result<Self> {
        let outside = || errors::invalid("Project file path must stay within the workspace");
        let path = Path::new(requested);
        let plain = path.components().all(|part| match part {
            Component::Normal(name) => !name.to_string_lossy().starts_with('.'),
            Component::CurDir => true,
            _ => false,
        });
        if path.is_absolute() || !plain {
            return Err(outside());
        }
        let root =
            soyeht_platform::fs::canonicalize(workspace_root).map_err(errors::workspace_root)?;
        let file = soyeht_platform::fs::canonicalize(&root.join(path)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                errors::not_found("Project file not found")
            } else {
                errors::io(error)
            }
        })?;
        let relative = file.strip_prefix(&root).map_err(|_| outside())?;
        if relative.components().any(hidden) {
            return Err(outside());
        }
        let metadata = std::fs::metadata(&file).map_err(errors::io)?;
        if !metadata.is_file() {
            return Err(errors::invalid("Project path is not a file"));
        }
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| errors::invalid("Project file name must be text"))?
            .to_owned();
        let directory = file
            .parent()
            .ok_or_else(|| errors::invalid("Project file has no folder"))?
            .to_path_buf();
        Ok(Self {
            file,
            root: directory,
            name,
        })
    }
}

/// Whether a part of a path is hidden (starts with `.`).
fn hidden(part: Component<'_>) -> bool {
    matches!(part, Component::Normal(name) if name.to_string_lossy().starts_with('.'))
}

/// The page of one pane: the server that answers for it and the watch that reloads it.
pub(crate) struct ProjectPage {
    workspace_id: String,
    file: PathBuf,
    /// `http://127.0.0.1:<port>/<token>/`: what the pane's address starts with while it shows
    /// this page.
    base: String,
    /// The address of the file.
    entry: String,
    /// Raised once per burst of changes under the root.
    changes: Arc<AtomicU64>,
    seen: u64,
    stop: Arc<AtomicBool>,
    server: JoinHandle<()>,
}

impl ProjectPage {
    /// The address that shows the file.
    pub(crate) fn entry_url(&self) -> &str {
        &self.entry
    }
}

impl Drop for ProjectPage {
    fn drop(&mut self) {
        self.server.abort();
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The pages the engine serves, by pane.
#[derive(Default)]
pub(crate) struct ProjectPages {
    pages: BTreeMap<String, ProjectPage>,
    /// Raised when a page changed, so the tick sends the reload.
    kick: Arc<Notify>,
}

impl ProjectPages {
    /// The signal a changed page raises (see [`crate::core::Core::open`]).
    pub(crate) fn kicked_by(&mut self, kick: Arc<Notify>) {
        self.kick = kick;
    }

    /// The pane of `workspace_id` that shows `file`, if there is one.
    pub(crate) fn pane_showing(&self, workspace_id: &str, file: &ProjectFile) -> Option<&str> {
        self.pages
            .iter()
            .find(|(_, page)| page.workspace_id == workspace_id && page.file == file.file)
            .map(|(pane_id, _)| pane_id.as_str())
    }

    /// Starts serving `file` for the workspace: the listener, the token and the watch.
    pub(crate) fn start(&self, workspace_id: &str, file: ProjectFile) -> Result<ProjectPage> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| errors::internal("Serving a project page needs the engine's runtime"))?;
        let listener = crate::network::listen(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
            .map_err(errors::io)?;
        let port = listener.local_addr().map_err(errors::io)?.port();
        let token = Uuid::new_v4().simple().to_string();
        let page_error = |_| errors::internal("The project page address is not valid");
        let entry = soyeht_web::page_url(port, &token, std::slice::from_ref(&file.name))
            .map_err(page_error)?;
        let base = soyeht_web::page_url(port, &token, &[]).map_err(page_error)?;
        let changes = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        watch::follow(
            file.root.clone(),
            Arc::clone(&self.kick),
            Arc::clone(&changes),
            Arc::clone(&stop),
        );
        let server = runtime.spawn(serve::run(
            listener,
            serve::Site::new(port, token, file.root),
        ));
        Ok(ProjectPage {
            workspace_id: workspace_id.into(),
            file: file.file,
            base,
            entry,
            changes,
            seen: 0,
            stop,
            server,
        })
    }

    /// Keeps `page` for `pane_id`, unless the pane already has one: a repeated open of the same
    /// call finds its pane, and the server it started is not the pane's.
    pub(crate) fn attach(&mut self, pane_id: &str, page: ProjectPage) {
        self.pages.entry(pane_id.into()).or_insert(page);
    }

    /// Ends the page of a pane that closed.
    pub(crate) fn forget(&mut self, pane_id: &str) {
        self.pages.remove(pane_id);
    }

    /// The panes whose files changed since this was last asked, with the address their page starts
    /// with.
    pub(crate) fn take_changed(&mut self) -> Vec<(String, String)> {
        self.pages
            .iter_mut()
            .filter_map(|(pane_id, page)| {
                let now = page.changes.load(Ordering::Relaxed);
                let changed = now != page.seen;
                page.seen = now;
                changed.then(|| (pane_id.clone(), page.base.clone()))
            })
            .collect()
    }
}

//! A project file shown as a page (contract 11, section 5b; ADR 0166, section 9).
//!
//! `web.open {path}` names a file of the workspace. The engine serves it, and the files below its
//! directory, read-only on the loopback interface for the life of the pane, and the pane shows the
//! address like any other page. What a page may read, who may ask and what the answers carry are
//! the contract's; the rules that are pure are `soyeht_web::project_page`'s and the checks on the
//! files are [`serve`]'s, made again on every request. A change to a file the page has been given
//! reloads the pane's page ([`watch`]), and a pane that holds `projectPath` is served again when
//! the engine starts.

#[cfg(test)]
mod pane_tests;
mod serve;
#[cfg(test)]
mod tests;
mod watch;

use crate::{
    auth,
    errors::{self, Result},
};
use soyeht_web::name_allowed;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{sync::Notify, task::JoinHandle};

/// The file a `web.open` names, resolved: canonical, inside the workspace, and a regular file.
pub(crate) struct PageFile {
    /// The file, canonical.
    file: PathBuf,
    /// Its directory: what the page may read.
    root: PathBuf,
    /// Its name, which the address ends with.
    name: String,
    /// Its path below the workspace folder, with `/` between the parts: the pane's `projectPath`.
    relative: String,
}

impl PageFile {
    /// The file `requested` names inside `workspace_root`, by the rule the editor opens a file
    /// with (an absolute path, a part that climbs and a link that leads out are refused), and
    /// by the names `soyeht_web::name_allowed` accepts, on the path as written and as resolved.
    pub(crate) fn resolve(workspace_root: &Path, requested: &str) -> Result<Self> {
        let outside = || errors::invalid("Project file path must stay within the workspace");
        let named = Path::new(requested).components().all(|part| match part {
            Component::Normal(name) => name.to_str().is_some_and(name_allowed),
            Component::CurDir => true,
            _ => false,
        });
        if !named {
            return Err(outside());
        }
        let resolved =
            crate::editor::resolve_in(&crate::editor::SystemFiles, workspace_root, requested)
                .map_err(|error| match error {
                    errors::Error::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                        errors::not_found("Project file not found")
                    }
                    other => other,
                })?;
        if !resolved.relative.split('/').all(name_allowed) {
            return Err(outside());
        }
        let metadata = std::fs::metadata(&resolved.canonical).map_err(errors::io)?;
        if !metadata.is_file() {
            return Err(errors::invalid("Project path is not a file"));
        }
        let name = resolved
            .canonical
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| errors::invalid("Project file name must be text"))?
            .to_owned();
        let root = resolved
            .canonical
            .parent()
            .ok_or_else(|| errors::invalid("Project file has no folder"))?
            .to_path_buf();
        Ok(Self {
            file: resolved.canonical,
            root,
            name,
            relative: resolved.relative,
        })
    }
}

#[cfg(test)]
impl PageFile {
    /// The file and the directory the page serves, for the containment fuzz test.
    pub(crate) fn paths(&self) -> (&Path, &Path) {
        (&self.file, &self.root)
    }
}

/// How many files a page is followed for.
const SERVED_FILES_MAX: usize = 1_000;

/// The files a page's server gave since the page last loaded: the ones a change reloads it for.
#[derive(Default)]
pub(super) struct Served {
    files: Mutex<BTreeSet<PathBuf>>,
}

impl Served {
    /// The page's own document was asked for: the page loads again, and what it needs is asked
    /// for again.
    fn loaded(&self, document: &Path) {
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        files.clear();
        files.insert(document.to_path_buf());
    }

    /// A file was given, or was asked for and is not there yet: when it changes, or appears, the
    /// page is reloaded. A page that asks for ever more names is not followed past the bound.
    fn gave(&self, file: &Path) {
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        if files.len() < SERVED_FILES_MAX {
            files.insert(file.to_path_buf());
        }
    }

    fn any_of(&self, paths: &[PathBuf]) -> bool {
        let files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        paths.iter().any(|path| files.contains(path))
    }
}

/// The page of one pane: the server that answers for it and the watch that reloads it.
pub(crate) struct ProjectPage {
    workspace_id: String,
    file: PathBuf,
    /// `http://127.0.0.1:<port>/<token>/`: what the pane's address starts with while it shows
    /// this page.
    prefix: String,
    /// The address of the file.
    entry: String,
    /// The path the pane holds as its `projectPath`.
    relative: String,
    /// Raised once per reload the watch asks for.
    changes: Arc<AtomicU64>,
    seen: u64,
    /// What keeps the watch going; dropping it ends the watch's thread.
    _watch: Arc<Mutex<Option<crate::file_events::Hold>>>,
    server: JoinHandle<()>,
}

impl ProjectPage {
    /// The address that shows the file.
    pub(crate) fn entry_url(&self) -> &str {
        &self.entry
    }

    /// The pane's `projectPath`.
    pub(crate) fn project_path(&self) -> &str {
        &self.relative
    }
}

impl Drop for ProjectPage {
    fn drop(&mut self) {
        self.server.abort();
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

    /// The pane of `workspace_id` that shows `file` still, when it was asked for and `url` is the
    /// address the pane holds now.
    pub(crate) fn pane_showing<'a>(
        &'a self,
        workspace_id: &str,
        file: &PageFile,
        current_url: impl Fn(&str) -> Option<String>,
    ) -> Option<&'a str> {
        self.pages
            .iter()
            .find(|(pane_id, page)| {
                page.workspace_id == workspace_id
                    && page.file == file.file
                    && current_url(pane_id).is_some_and(|url| url.starts_with(&page.prefix))
            })
            .map(|(pane_id, _)| pane_id.as_str())
    }

    /// Starts serving `file` for the workspace: the listener, the token and the watch.
    pub(crate) fn start(&self, workspace_id: &str, file: PageFile) -> Result<ProjectPage> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| errors::internal("Serving a project page needs the engine's runtime"))?;
        let listener = crate::network::listen(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
            .map_err(errors::io)?;
        let port = listener.local_addr().map_err(errors::io)?.port();
        let token = auth::token().map_err(|_| errors::internal("No random token is available"))?;
        let page_error = |_| errors::internal("The project page address is not valid");
        let entry = soyeht_web::page_url(port, &token, std::slice::from_ref(&file.name))
            .map_err(page_error)?;
        let prefix = format!(
            "{}/",
            soyeht_web::page_url(port, &token, &[]).map_err(page_error)?
        );
        let served = Arc::new(Served::default());
        served.loaded(&file.file);
        let changes = Arc::new(AtomicU64::new(0));
        let watch = watch::follow(
            file.root.clone(),
            Arc::clone(&served),
            Arc::clone(&self.kick),
            Arc::clone(&changes),
        );
        let site = serve::Site::new(port, token, file.root, file.file.clone(), served);
        let relative = file.relative;
        let server = runtime.spawn(serve::run(listener, site));
        Ok(ProjectPage {
            workspace_id: workspace_id.into(),
            file: file.file,
            prefix,
            entry,
            relative,
            changes,
            seen: 0,
            _watch: watch,
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

    /// The panes whose page was changed since this was last asked, with the address their page
    /// starts with.
    pub(crate) fn take_changed(&mut self) -> Vec<(String, String)> {
        self.pages
            .iter_mut()
            .filter_map(|(pane_id, page)| {
                let now = page.changes.load(Ordering::Relaxed);
                let changed = now != page.seen;
                page.seen = now;
                changed.then(|| (pane_id.clone(), page.prefix.clone()))
            })
            .collect()
    }
}

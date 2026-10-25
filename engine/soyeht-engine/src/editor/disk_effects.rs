//! The editor's autosave and disk watch, run off the Core lock (ADR 0079, ADR 0097).
//!
//! A save reads the file, writes a temporary file, flushes it, renames it over the target and
//! flushes the directory; the watch reads every watched file again. On a busy disk each of
//! those takes milliseconds to seconds, and the tick used to do them with the lock held, so
//! every request and the terminal output waited. Now the tick copies the inputs, a background
//! job (`Core::spawn_background` on a thread of its own) does the disk work, and the result is applied under the
//! lock on a later tick, against whatever the file has become meanwhile.
//!
//! One job per file runs at a time (the save and the watch share it), so two writes of one
//! file never overlap and a watch never reads half of a save. Requests that move or delete a
//! file wait for its job first, and a file closed while its job runs is finished from its
//! stored row.

use super::save_failure;
use super::{
    Effect, Failure,
    persistence::{next_revision, stored_version},
    service::{FileSystemPort, Service, read_disk_from, resolve_in},
};
use crate::{core::Core, errors, errors::Result, model, offload::Done};
use soyeht_platform::log::Line;
use soyeht_protocol::{EditorSaveFailure, FileAuthor, FileSaveFailedEvent, event};
use soyeht_store::editor::EditorFile;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// How long a request that moves or deletes a file waits for the job working on it.
pub(super) const SETTLE_WAIT: Duration = Duration::from_millis(1500);

/// What a background disk job found.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DiskResult {
    /// This draft is on the disk.
    Written(String),
    /// The disk holds this text, which the editor did not know.
    Changed(String),
    /// The disk holds what the editor knew, and there was nothing to write.
    Unchanged,
}

/// What a result is judged by: the parts of a file that say it is still the one the job saw.
/// The disk text changes only with `disk_revision`, or when a conflict is resolved, which
/// `conflicted` shows.
#[derive(Debug, PartialEq, Eq)]
struct Basis {
    relative_path: String,
    disk_revision: u64,
    conflicted: bool,
}

impl Basis {
    fn of(file: &EditorFile) -> Self {
        Self {
            relative_path: file.relative_path.clone(),
            disk_revision: file.disk_revision,
            conflicted: file.conflict_text.is_some(),
        }
    }
}

/// The file as a job was given it, without its texts.
pub(super) struct Snapshot {
    revision: u64,
    basis: Basis,
    /// Who wrote the draft a save carries.
    author: Option<FileAuthor>,
}

/// The inputs of one disk job, copied under the lock. It reaches the disk and nothing else.
pub(super) struct DiskJob {
    files: Arc<dyn FileSystemPort>,
    workspace_root: PathBuf,
    relative_path: String,
    /// The text the editor believes the disk holds.
    known: String,
    /// The text to write once the disk still holds `known`; a watch has none.
    draft: Option<String>,
    /// The person asked to save: a file gone from the disk is created again, empty, and
    /// then written.
    recreate: bool,
}

impl DiskJob {
    /// The job for `effect` on `file`, with what it will be judged by. `None` when there is
    /// nothing to do: a save needs a draft that is not in conflict.
    fn new(
        files: Arc<dyn FileSystemPort>,
        workspace_root: PathBuf,
        file: &EditorFile,
        effect: Effect,
        recreate: bool,
    ) -> Option<(Self, Snapshot)> {
        let draft = match effect {
            Effect::Save if file.conflict_text.is_none() => Some(file.draft_text.clone()?),
            Effect::Save => return None,
            Effect::Watch => None,
        };
        let known = file.conflict_text.as_ref().unwrap_or(&file.disk_text);
        let job = Self {
            files,
            workspace_root,
            relative_path: file.relative_path.clone(),
            known: known.clone(),
            draft,
            recreate,
        };
        let snapshot = Snapshot {
            revision: file.revision,
            basis: Basis::of(file),
            author: file.draft_author.clone(),
        };
        Some((job, snapshot))
    }

    pub(super) fn run(self) -> Result<DiskResult> {
        let found =
            resolve_in(&*self.files, &self.workspace_root, &self.relative_path).and_then(|path| {
                read_disk_from(&*self.files, &path.canonical).map(|disk| (path, disk))
            });
        let (path, disk) = match found {
            Ok(found) => found,
            // The person asked to save a file that is gone: it is created again with the draft.
            Err(error)
                if self.recreate && self.draft.is_some() && save_failure::is_missing(&error) =>
            {
                return self.recreated();
            }
            Err(error) => return Err(error),
        };
        if disk != self.known {
            return Ok(DiskResult::Changed(disk));
        }
        let Some(draft) = self.draft else {
            return Ok(DiskResult::Unchanged);
        };
        save_failure::refuse_read_only(&path.canonical).map_err(errors::io)?;
        let written = self
            .files
            .write_text_if_unchanged(&path.canonical, &self.known, &draft)
            .map_err(errors::io)?;
        if written {
            Ok(DiskResult::Written(draft))
        } else {
            // The file changed between the read and the write: report what it holds now.
            read_disk_from(&*self.files, &path.canonical).map(DiskResult::Changed)
        }
    }

    /// The file is gone and the person asked to save it: it is created again holding the draft,
    /// in one step, so a write that fails or a crash leaves no file, never an empty one that
    /// the next save would take for a change made by someone else.
    fn recreated(self) -> Result<DiskResult> {
        let Some(draft) = self.draft.clone() else {
            return Ok(DiskResult::Unchanged);
        };
        let relative = Path::new(&self.relative_path);
        match self
            .files
            .create_text_if_absent(&self.workspace_root, relative, &draft)
        {
            Ok(_) => Ok(DiskResult::Written(draft)),
            // Another program made the file meanwhile: it is judged like any other save.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Self {
                recreate: false,
                ..self
            }
            .run(),
            Err(error) => Err(errors::io(error)),
        }
    }
}

impl Service {
    /// The log lines the applied jobs produced since the last call.
    pub(super) fn take_records(&mut self) -> Vec<Line> {
        std::mem::take(&mut self.records)
    }
}

const JOB_PREFIX: &str = "editor.disk:";
const CLOSE_PREFIX: &str = "editor.close:";

fn job_key(file_id: &str) -> String {
    format!("{JOB_PREFIX}{file_id}")
}

/// The key of the job that saves what a closed tab had left.
fn close_key(file_id: &str) -> String {
    format!("{CLOSE_PREFIX}{file_id}")
}

impl Core {
    pub fn advance_editor(&mut self) -> Result<()> {
        for line in self.advance_editor_effects() {
            line.emit();
        }
        Ok(())
    }

    /// Starts the autosaves and disk checks that are due and returns the log records of the ones
    /// that finished. A failing effect backs off (see `schedule`), so a file that stays
    /// unreadable is retried less and less often and logged only at attempts 1, 2, 4, 8 and so
    /// on. A file whose job is still running stays due and is looked at again on the next tick.
    pub(super) fn advance_editor_effects(&mut self) -> Vec<Line> {
        let now = self.now_ms();
        if !self.hooks.pause_editor_autosave() {
            for file_id in self.editor.due_saves(now) {
                self.start_disk_job(&file_id, Effect::Save, now);
            }
        }
        for file_id in self.editor.due_watches(now) {
            self.start_disk_job(&file_id, Effect::Watch, now);
        }
        self.editor.take_records()
    }

    fn start_disk_job(&mut self, file_id: &str, effect: Effect, now: i64) {
        let key = job_key(file_id);
        if !self.background.accepts(&key) {
            return;
        }
        let prepared = self.prepare_disk_job(file_id, effect);
        if effect == Effect::Watch {
            self.editor.checked_watch(file_id, now);
        }
        let (job, snapshot) = match prepared {
            Ok(Some(prepared)) => prepared,
            Ok(None) => return self.settle_disk_job(file_id, effect, Ok(())),
            Err(error) => return self.settle_disk_job(file_id, effect, Err(error)),
        };
        let applied = file_id.to_owned();
        self.background.run_alone(&key);
        let started = self.spawn_background(
            key,
            move || job.run(),
            move |core, result| {
                core.finish_disk_job(&applied, effect, &snapshot, result);
                Ok(())
            },
        );
        if let Err(error) = started {
            self.settle_disk_job(file_id, effect, Err(error));
        }
    }

    fn prepare_disk_job(
        &mut self,
        file_id: &str,
        effect: Effect,
    ) -> Result<Option<(DiskJob, Snapshot)>> {
        let recreate = effect == Effect::Save && self.editor.take_recreate(file_id);
        self.disk_job_for(self.editor.stored(file_id)?, effect, recreate)
    }

    fn disk_job_for(
        &self,
        file: &EditorFile,
        effect: Effect,
        recreate: bool,
    ) -> Result<Option<(DiskJob, Snapshot)>> {
        let workspace = model::workspace(&self.store, &file.workspace_id)?;
        Ok(DiskJob::new(
            Arc::clone(&self.editor.files),
            PathBuf::from(workspace.path.as_str()),
            file,
            effect,
            recreate,
        ))
    }

    /// Runs a pending autosave now instead of waiting for its debounce; used before a file is
    /// deleted. A conflict is left untouched. This is a person's request that must know the
    /// outcome, so it waits for the disk. A file whose job is running is left to that job: its
    /// result is applied by the tick, and a request that moves or deletes the file has already
    /// waited for it (`project_entry_request`).
    pub(super) fn save_editor_now(&mut self, file_id: &str) -> Result<()> {
        let pending = self
            .editor
            .stored(file_id)
            .is_ok_and(|file| file.draft_text.is_some() && file.conflict_text.is_none());
        if !pending || self.hooks.pause_editor_autosave() || self.disk_job_running(file_id) {
            return Ok(());
        }
        let Some((job, snapshot)) = self.prepare_disk_job(file_id, Effect::Save)? else {
            return Ok(());
        };
        let result = job.run()?;
        self.apply_disk_result(file_id, &snapshot, result, self.now_ms())
    }

    /// The signals of the jobs working on a file at `path` or below it, for a request to wait on
    /// off the lock. The jobs are found by their keys, not through the open files: the job that
    /// saves a closed tab outlives its file's tab.
    pub(super) fn disk_job_signals_under(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<Vec<Arc<Done>>> {
        let prefix = format!("{path}/");
        let mut signals = Vec::new();
        for (key, done) in self.background.alone_jobs() {
            let Some(file_id) = key
                .strip_prefix(JOB_PREFIX)
                .or_else(|| key.strip_prefix(CLOSE_PREFIX))
            else {
                continue;
            };
            let place = if let Ok(file) = self.editor.stored(file_id) {
                Some((file.workspace_id.clone(), file.relative_path.clone()))
            } else {
                // Its tab is closed: the stored row has the place.
                self.store
                    .editor_file(file_id)
                    .map_err(errors::storage)?
                    .map(|file| (file.workspace_id, file.relative_path))
            };
            if place.is_some_and(|(workspace, relative)| {
                workspace == workspace_id && (relative == path || relative.starts_with(&prefix))
            }) {
                signals.push(Arc::clone(done));
            }
        }
        Ok(signals)
    }

    /// Refuses a request that moves or deletes the files at `path` while a job is working on
    /// one of them: a write that is renaming a file into the old place would recreate it.
    pub(super) fn refuse_while_saving(&self, workspace_id: &str, path: &str) -> Result<()> {
        if self.disk_job_signals_under(workspace_id, path)?.is_empty() {
            return Ok(());
        }
        Err(errors::stale_version("The file is being saved"))
    }

    /// Whether a disk job is working on the file, including the one that saves a closed tab.
    pub(super) fn disk_job_running(&self, file_id: &str) -> bool {
        self.background.is_active(&job_key(file_id))
            || self.background.is_active(&close_key(file_id))
    }

    fn finish_disk_job(
        &mut self,
        file_id: &str,
        effect: Effect,
        snapshot: &Snapshot,
        result: Result<DiskResult>,
    ) {
        let now = self.now_ms();
        let mut outcome =
            result.and_then(|result| self.apply_disk_result(file_id, snapshot, result, now));
        if !self.editor.is_open(file_id) {
            // The tab closed while the job ran: whatever it found, finish the file from its row.
            outcome = outcome.and(self.finish_closed_file(file_id));
        }
        self.settle_disk_job(file_id, effect, outcome);
    }

    fn settle_disk_job(&mut self, file_id: &str, effect: Effect, outcome: Result<()>) {
        let line = self.settle_editor_effect(file_id, effect, self.now_ms(), outcome);
        self.editor.records.extend(line);
    }

    fn settle_editor_effect(
        &mut self,
        file_id: &str,
        effect: Effect,
        now: i64,
        outcome: Result<()>,
    ) -> Option<Line> {
        match outcome {
            Ok(()) => self
                .editor
                .effect_succeeded(file_id, effect)
                .map(|attempts| {
                    Line::new("engine.editor_effect_recovered")
                        .field("action", effect.name())
                        .field("fileId", file_id)
                        .field("attempts", attempts)
                }),
            Err(error) => {
                // Only the class of the failure: an error's text can carry paths.
                let kind = soyeht_platform::log::Failure::failure_kind(&error);
                let reason = save_failure::classify(&error);
                let failure =
                    self.editor
                        .effect_failed(file_id, effect, &kind, reason.clone(), now)?;
                if effect == Effect::Save {
                    self.announce_save_failure(file_id, reason);
                }
                failure
                    .report
                    .then(|| failure_line(effect, file_id, kind, failure))
            }
        }
    }

    /// Tells the clients why the file's save failed. The state is also on the file's document
    /// (`file.read`), so a client that connects later learns it.
    fn announce_save_failure(&self, file_id: &str, reason: EditorSaveFailure) {
        let params = FileSaveFailedEvent {
            file_id: file_id.to_owned(),
            reason,
        };
        let sent = serde_json::to_value(params)
            .map_err(|_| errors::internal("File save failure serialization failed"))
            .and_then(|params| self.transient_event(event::FILE_SAVE_FAILED, params));
        if let Err(error) = sent {
            soyeht_platform::log::failure("engine.editor_save_failure_not_sent", &error);
        }
    }

    /// Applies what a disk job found to the file as it is now. `snapshot` is the file as the
    /// job was given it; the person may have typed since, the file may have moved, and its tab
    /// may have closed.
    pub(super) fn apply_disk_result(
        &mut self,
        file_id: &str,
        snapshot: &Snapshot,
        result: DiskResult,
        now: i64,
    ) -> Result<()> {
        if result == DiskResult::Unchanged {
            return Ok(());
        }
        let current = if self.editor.is_open(file_id) {
            self.editor.stored(file_id)?.clone()
        } else {
            // The tab closed while the job ran: the stored row has the draft as it was left.
            match self.store.editor_file(file_id).map_err(errors::storage)? {
                Some(file) => file,
                None => return Ok(()),
            }
        };
        self.judge(snapshot, current, result, now)
    }

    /// A tab that closed while its job ran could not save the text typed after the job began.
    /// Now that the job is applied, save what is left in a job of its own, as closing a tab
    /// promises, and apply it like any other result.
    fn finish_closed_file(&mut self, file_id: &str) -> Result<()> {
        let Some(file) = self.store.editor_file(file_id).map_err(errors::storage)? else {
            return Ok(());
        };
        if self.hooks.pause_editor_autosave() {
            return Ok(());
        }
        let Some((job, snapshot)) = self.disk_job_for(&file, Effect::Save, false)? else {
            return Ok(());
        };
        // Not the key of the job that just ended: that one is released after this apply.
        let key = close_key(file_id);
        self.background.run_alone(&key);
        let applied = file_id.to_owned();
        self.spawn_background(
            key,
            move || job.run(),
            move |core, result| {
                let now = core.now_ms();
                core.apply_disk_result(&applied, &snapshot, result?, now)
            },
        )
    }

    fn judge(
        &mut self,
        snapshot: &Snapshot,
        current: EditorFile,
        result: DiskResult,
        now: i64,
    ) -> Result<()> {
        match result {
            DiskResult::Unchanged => Ok(()),
            DiskResult::Written(text) => {
                if current.relative_path != snapshot.basis.relative_path {
                    // Moved while the write ran. The requests that move a file wait for its
                    // job, so this does not happen; the draft stays dirty and is saved again
                    // at the new place, never marked as saved.
                    return Ok(());
                }
                self.record_written(snapshot, &text, current, now)
            }
            DiskResult::Changed(disk) => {
                if snapshot.basis != Basis::of(&current) {
                    // The file moved on since the job read it (a revert, a reopen): what the
                    // job saw is no longer a change to judge, and the next attempt reads again.
                    return Ok(());
                }
                self.apply_disk_change(current, disk, now)
            }
        }
    }

    /// `written` is on the disk. The version is that text at the revision written. A draft the
    /// person typed while the write ran stays dirty and is saved next.
    fn record_written(
        &mut self,
        snapshot: &Snapshot,
        written: &str,
        mut file: EditorFile,
        now: i64,
    ) -> Result<()> {
        let author = snapshot
            .author
            .clone()
            .ok_or_else(|| errors::internal("Written editor save has no author"))?;
        let current_revision = std::mem::replace(&mut file.revision, snapshot.revision);
        let version = stored_version(&file, written.into(), author.clone(), self.timestamp());
        file.revision = current_revision;
        file.disk_revision = next_revision(file.disk_revision)?;
        file.disk_text = written.into();
        file.conflict_text = None;
        file.updated_ms = now;
        if file.draft_text.as_deref() == Some(written) {
            file.draft_text = None;
            file.draft_author = None;
        }
        self.persist_saved(file, version, author)
    }
}

fn failure_line(effect: Effect, file_id: &str, kind: String, failure: Failure) -> Line {
    Line::new("engine.editor_effect_failed")
        .field("action", effect.name())
        .field("fileId", file_id)
        .field("kind", kind)
        .field("attempts", failure.attempts)
        .field("retryInMs", failure.retry_in_ms)
}

#[cfg(test)]
mod tests;

use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    model,
    offload::Offload,
};
use serde_json::Value;
use soyeht_protocol::{
    DiffHunk, DiffLine, DiffLineKind, FileBlame, FileDiffParams, FileDiffResult,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const RESULT_HEADROOM: usize = 512;
/// Three git commands, each with its own deadline in soyeht-vcs.
const DIFF_TIMEOUT: Duration = Duration::from_secs(120);

impl Core {
    /// Reads the buffer under the Core lock and runs git off it (ADR 0079).
    pub fn file_diff(&mut self, params: &Value) -> Result<Outcome> {
        let request: FileDiffParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid file diff parameters"))?;
        let revision = request
            .revision
            .parse::<u64>()
            .map_err(|_| errors::invalid("File revision is invalid"))?;
        let file = self.editor.stored(&request.file_id)?.clone();
        if file.revision != revision {
            return Err(errors::stale_version("File revision changed"));
        }
        let workspace = model::workspace(&self.store, &file.workspace_id)?;
        let resolved = self
            .editor
            .resolve(Path::new(&workspace.path), &file.relative_path)?;
        let current = file.draft_text.clone().unwrap_or(file.disk_text);
        let vcs = self.vcs.clone();
        let workspace = PathBuf::from(workspace.path);
        let relative = PathBuf::from(resolved.relative);
        let file_id = file.file_id;
        let revision = file.revision.to_string();
        Ok(Outcome::Offload(Offload::new(
            "File diff timed out",
            DIFF_TIMEOUT,
            move || Ok(vcs.inspect(&workspace, &relative, &current)?),
            move |_, diff| diff_result(file_id, revision, diff?).map(Outcome::Reply),
        )))
    }
}

fn diff_result(file_id: String, revision: String, diff: soyeht_vcs::Diff) -> Result<Value> {
    let result = FileDiffResult {
        file_id,
        revision,
        hunks: diff
            .hunks
            .into_iter()
            .map(|hunk| wire_hunk(hunk, None))
            .collect(),
        blame: diff.blame.into_iter().map(wire_blame).collect(),
    };
    let value = serde_json::to_value(result)
        .map_err(|_| errors::internal("File diff serialization failed"))?;
    if errors::json_bytes(&value)?.len()
        > soyeht_protocol::limits::JSON_BYTES.saturating_sub(RESULT_HEADROOM)
    {
        return Err(errors::internal(
            "File diff exceeds the response byte budget",
        ));
    }
    Ok(value)
}

pub(crate) fn wire_hunk(
    hunk: soyeht_vcs::Hunk,
    author: Option<&soyeht_protocol::FileAuthor>,
) -> DiffHunk {
    DiffHunk {
        hunk_id: hunk.hunk_id,
        old_start: hunk.old_start,
        old_lines: hunk.old_lines,
        new_start: hunk.new_start,
        new_lines: hunk.new_lines,
        lines: hunk
            .lines
            .into_iter()
            .map(|line| wire_line(line, author))
            .collect(),
    }
}

fn wire_line(line: soyeht_vcs::Line, author: Option<&soyeht_protocol::FileAuthor>) -> DiffLine {
    let changed = line.kind != soyeht_vcs::LineKind::Context;
    DiffLine {
        kind: match line.kind {
            soyeht_vcs::LineKind::Context => DiffLineKind::Context,
            soyeht_vcs::LineKind::Added => DiffLineKind::Added,
            soyeht_vcs::LineKind::Removed => DiffLineKind::Removed,
        },
        old_line: line.old_line,
        new_line: line.new_line,
        text: line.text,
        author: changed.then(|| author.cloned()).flatten(),
    }
}

fn wire_blame(blame: soyeht_vcs::Blame) -> FileBlame {
    FileBlame {
        line: blame.line,
        commit: blame.commit,
        author: blame.author,
        summary: blame.summary,
    }
}

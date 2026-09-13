use crate::{
    core::Core,
    errors::{self, Result},
    model,
};
use serde_json::Value;
use soyeht_protocol::{
    DiffHunk, DiffLine, DiffLineKind, FileBlame, FileDiffParams, FileDiffResult,
};
use std::path::Path;

const RESULT_HEADROOM: usize = 512;

impl Core {
    pub fn file_inspection(&mut self, method: &str, params: &Value) -> Result<Value> {
        match method {
            "file.diagnostics" => self.file_diagnostics(params),
            "file.diff" => self.file_diff(params),
            _ => Err(errors::internal("Unknown file inspection method")),
        }
    }

    pub fn file_diff(&mut self, params: &Value) -> Result<Value> {
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
        let current = file.draft_text.as_ref().unwrap_or(&file.disk_text);
        let diff = self.vcs.inspect(
            Path::new(&workspace.path),
            Path::new(&resolved.relative),
            current,
        )?;
        let result = FileDiffResult {
            file_id: file.file_id,
            revision: file.revision.to_string(),
            hunks: diff.hunks.into_iter().map(wire_hunk).collect(),
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
}

fn wire_hunk(hunk: soyeht_vcs::Hunk) -> DiffHunk {
    DiffHunk {
        hunk_id: hunk.hunk_id,
        old_start: hunk.old_start,
        old_lines: hunk.old_lines,
        new_start: hunk.new_start,
        new_lines: hunk.new_lines,
        lines: hunk.lines.into_iter().map(wire_line).collect(),
    }
}

fn wire_line(line: soyeht_vcs::Line) -> DiffLine {
    DiffLine {
        kind: match line.kind {
            soyeht_vcs::LineKind::Context => DiffLineKind::Context,
            soyeht_vcs::LineKind::Added => DiffLineKind::Added,
            soyeht_vcs::LineKind::Removed => DiffLineKind::Removed,
        },
        old_line: line.old_line,
        new_line: line.new_line,
        text: line.text,
        author: None,
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

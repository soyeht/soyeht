use super::documents::{public_document, require_revision};
use crate::{core::Core, dispatch::Outcome, errors, errors::Result, offload::Offload};
use serde_json::Value;
use soyeht_protocol::{
    ScreenplayScene, WritingKind, WritingSectionListParams, WritingSectionListResult,
    WritingStructureParams, WritingStructureResult, limits,
};
use soyeht_writing::SectionEntryInput;
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

const SECTION_LIST_TIMEOUT: Duration = Duration::from_secs(5);

struct PreparedWritingSectionList {
    request: WritingSectionListParams,
    workspace_root: PathBuf,
}

struct ScannedWritingSectionList {
    request: WritingSectionListParams,
    files: Vec<crate::editor::ProjectFile>,
}

impl PreparedWritingSectionList {
    fn scan(self) -> Result<ScannedWritingSectionList> {
        let workspace_root =
            soyeht_platform::fs::canonicalize(&self.workspace_root).map_err(errors::io)?;
        let folder = soyeht_writing::folder_for(&self.request.section);
        let section_root = match soyeht_platform::fs::canonicalize(&workspace_root.join(folder)) {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(ScannedWritingSectionList {
                    request: self.request,
                    files: Vec::new(),
                });
            }
            Err(error) => return Err(errors::io(error)),
        };
        if !section_root.starts_with(&workspace_root) {
            return Err(errors::invalid(
                "Writing section folder must stay within the workspace",
            ));
        }
        let mut files =
            crate::editor::enumerate_project_files(&section_root).map_err(errors::io)?;
        for file in &mut files {
            file.relative = format!("{folder}/{}", file.relative);
        }
        Ok(ScannedWritingSectionList {
            request: self.request,
            files,
        })
    }
}

impl ScannedWritingSectionList {
    fn finish(self, core: &Core) -> Result<Value> {
        let mut inputs = Vec::new();
        for file in self.files {
            let opened = core
                .store
                .editor_file_for_path(&self.request.workspace_id, &file.relative)
                .map_err(errors::storage)?;
            inputs.push(SectionEntryInput {
                path: file.relative.clone(),
                title: title(&file.relative),
                file_id: opened.map(|item| item.file_id),
                modified_at: timestamp(file.modified_ms),
                byte_count: file.byte_count,
            });
            if inputs.len() > limits::WRITING_SECTION_ENTRIES {
                return Err(errors::Error::Writing(soyeht_writing::Error::SectionLimit));
            }
        }
        let entries = soyeht_writing::section_entries(self.request.section, inputs)?;
        let start = cursor_start(&entries, self.request.cursor.as_deref())?;
        let limit = self.request.limit as usize;
        let end = start.saturating_add(limit).min(entries.len());
        let page = entries[start..end].to_vec();
        let next_cursor = (end < entries.len())
            .then(|| page.last().map(|entry| entry.path.clone()))
            .flatten();
        serde_json::to_value(WritingSectionListResult {
            entries: page,
            next_cursor,
        })
        .map_err(|_| errors::internal("Writing section list cannot be encoded"))
    }
}

impl Core {
    /// Walks the section folder off the Core lock, then pages it under it.
    pub(crate) fn writing_section_list(&self, params: &Value) -> Result<Outcome> {
        let request: WritingSectionListParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid writing section list parameters"))?;
        if request.limit == 0 || request.limit as usize > limits::WRITING_SECTION_ENTRIES {
            return Err(errors::invalid("Invalid writing section page limit"));
        }
        let workspace = crate::model::workspace(&self.store, &request.workspace_id)?;
        let prepared = PreparedWritingSectionList {
            request,
            workspace_root: workspace.path.into(),
        };
        Ok(Outcome::Offload(Offload::new(
            "Writing section listing timed out",
            SECTION_LIST_TIMEOUT,
            move || prepared.scan(),
            |core, scanned| scanned?.finish(core).map(Outcome::Reply),
        )))
    }

    pub(super) fn writing_structure(&self, connection: &str, params: &Value) -> Result<Value> {
        let request: WritingStructureParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid writing structure parameters"))?;
        let file = self.editor.stored(&request.file_id)?.clone();
        let author = self.file_author(connection)?;
        super::variants::authorize_agent_workspace(self, &author, &file.workspace_id)?;
        require_revision(&request.revision, file.revision)?;
        let record = self.writing_record(&file.file_id)?;
        let text = file.draft_text.as_deref().unwrap_or(&file.disk_text);
        let (slides, mut scenes, elements) = structure(&record.kind, text)?;
        decorate_scenes(&mut scenes, &record.sidecar);
        let result = WritingStructureResult {
            document: public_document(&file, &record)?,
            slides,
            scenes,
            elements,
            variants: record.sidecar.line_variants,
            beats: record.sidecar.beat_cards,
            word_count: word_count(text),
            truncated: false,
        };
        serde_json::to_value(result)
            .map_err(|_| errors::internal("Writing structure cannot be encoded"))
    }
}

fn word_count(text: &str) -> u32 {
    u32::try_from(text.split_whitespace().count()).unwrap_or(u32::MAX)
}

fn structure(
    kind: &WritingKind,
    text: &str,
) -> Result<(
    Vec<soyeht_protocol::WritingSlide>,
    Vec<ScreenplayScene>,
    Vec<soyeht_protocol::ScreenplayElement>,
)> {
    match kind {
        WritingKind::Text => Ok((Vec::new(), Vec::new(), Vec::new())),
        WritingKind::Presentation => Ok((
            soyeht_writing::segment_slides(text)?,
            Vec::new(),
            Vec::new(),
        )),
        WritingKind::Screenplay => {
            let screenplay = soyeht_writing::parse_screenplay(text)?;
            Ok((Vec::new(), screenplay.scenes, screenplay.elements))
        }
    }
}

fn decorate_scenes(
    scenes: &mut [ScreenplayScene],
    sidecar: &soyeht_store::writing::WritingSidecar,
) {
    for scene in scenes {
        if let Some(annotation) = sidecar
            .scene_annotations
            .iter()
            .find(|item| item.scene_id == scene.scene_id)
        {
            scene
                .reference_image
                .clone_from(&annotation.reference_image);
            scene.caption.clone_from(&annotation.caption);
            scene.prompt_idea.clone_from(&annotation.prompt_idea);
        }
        scene.linked_beat_ids = sidecar
            .beat_cards
            .iter()
            .filter(|beat| beat.scene_id.as_deref() == Some(scene.scene_id.as_str()))
            .map(|beat| beat.beat_id.clone())
            .collect();
    }
}

fn cursor_start(
    entries: &[soyeht_protocol::WritingSectionEntry],
    cursor: Option<&str>,
) -> Result<usize> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    entries
        .iter()
        .position(|entry| entry.path == cursor)
        .map(|index| index + 1)
        .ok_or_else(|| errors::invalid("Writing section cursor is invalid"))
}

fn title(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .replace(['-', '_'], " ")
}

fn timestamp(value: i64) -> String {
    chrono::DateTime::from_timestamp_millis(value)
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_platform::testing::{allow_owner, deny_everyone};
    use soyeht_protocol::WritingSection;
    use std::fs;

    struct Fixture {
        root: PathBuf,
        protected: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "soyeht-writing-section-scope-{}",
                uuid::Uuid::new_v4()
            ));
            let protected = root.join("Pictures/Photos Library.photoslibrary");
            fs::create_dir_all(root.join("Writing/Texts/Research")).unwrap();
            fs::create_dir_all(&protected).unwrap();
            fs::write(root.join("Writing/Texts/Research/notes.md"), "# Notes\n").unwrap();
            fs::write(protected.join("private.md"), "private\n").unwrap();
            deny_everyone(&protected).unwrap();
            Self { root, protected }
        }

        fn prepared(&self, section: WritingSection) -> PreparedWritingSectionList {
            PreparedWritingSectionList {
                request: WritingSectionListParams {
                    workspace_id: "workspace".into(),
                    section,
                    cursor: None,
                    limit: 50,
                },
                workspace_root: self.root.clone(),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            allow_owner(&self.protected).unwrap();
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn section_scan_never_visits_a_protected_workspace_sibling() {
        let fixture = Fixture::new();
        let scanned = fixture.prepared(WritingSection::Texts).scan().unwrap();
        assert_eq!(scanned.files.len(), 1);
        assert_eq!(scanned.files[0].relative, "Writing/Texts/Research/notes.md");
    }

    #[test]
    fn missing_section_folder_is_an_empty_page() {
        let fixture = Fixture::new();
        let scanned = fixture
            .prepared(WritingSection::Screenplays)
            .scan()
            .unwrap();
        assert!(scanned.files.is_empty());
    }
}

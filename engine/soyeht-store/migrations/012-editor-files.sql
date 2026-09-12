CREATE TABLE editor_files (
  file_id TEXT PRIMARY KEY,
  pane_id TEXT NOT NULL UNIQUE REFERENCES panes(id) ON DELETE CASCADE,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  relative_path TEXT NOT NULL CHECK(length(relative_path) BETWEEN 1 AND 4096),
  language TEXT NOT NULL CHECK(language IN ('rust','swift','typescript','python','json','markdown','shell','plain_text')),
  revision INTEGER NOT NULL CHECK(revision >= 0),
  disk_revision INTEGER NOT NULL CHECK(disk_revision >= 0),
  disk_text BLOB NOT NULL,
  draft_text BLOB,
  draft_author TEXT CHECK(draft_author IS NULL OR json_valid(draft_author)),
  conflict_text BLOB,
  watching INTEGER NOT NULL CHECK(watching IN (0,1)),
  updated_ms INTEGER NOT NULL CHECK(updated_ms >= 0),
  UNIQUE(workspace_id, relative_path),
  CHECK((draft_text IS NULL) = (draft_author IS NULL)),
  CHECK(conflict_text IS NULL OR draft_text IS NOT NULL)
);

CREATE TABLE editor_file_versions (
  ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
  version_id TEXT NOT NULL UNIQUE,
  file_id TEXT NOT NULL REFERENCES editor_files(file_id) ON DELETE CASCADE,
  revision INTEGER NOT NULL CHECK(revision >= 0),
  created_at TEXT NOT NULL,
  author TEXT NOT NULL CHECK(json_valid(author)),
  content BLOB NOT NULL,
  byte_count INTEGER NOT NULL CHECK(byte_count >= 0),
  UNIQUE(file_id, revision)
);
CREATE INDEX editor_file_history ON editor_file_versions(file_id, ordinal DESC);

PRAGMA user_version = 12;

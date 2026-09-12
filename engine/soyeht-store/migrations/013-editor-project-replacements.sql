CREATE TABLE editor_files_v13 (
  file_id TEXT PRIMARY KEY,
  pane_id TEXT UNIQUE REFERENCES panes(id) ON DELETE SET NULL,
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
  CHECK(conflict_text IS NULL OR draft_text IS NOT NULL),
  CHECK(pane_id IS NOT NULL OR (draft_text IS NULL AND conflict_text IS NULL AND watching=0))
);

CREATE TABLE editor_file_versions_v13 (
  ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
  version_id TEXT NOT NULL UNIQUE,
  file_id TEXT NOT NULL REFERENCES editor_files_v13(file_id) ON DELETE CASCADE,
  revision INTEGER NOT NULL CHECK(revision >= 0),
  created_at TEXT NOT NULL,
  author TEXT NOT NULL CHECK(json_valid(author)),
  content BLOB NOT NULL,
  byte_count INTEGER NOT NULL CHECK(byte_count >= 0),
  UNIQUE(file_id, revision)
);

INSERT INTO editor_files_v13
SELECT * FROM editor_files;
INSERT INTO editor_file_versions_v13
SELECT * FROM editor_file_versions;
DROP TABLE editor_file_versions;
DROP TABLE editor_files;
ALTER TABLE editor_files_v13 RENAME TO editor_files;
ALTER TABLE editor_file_versions_v13 RENAME TO editor_file_versions;
CREATE INDEX editor_file_history ON editor_file_versions(file_id, ordinal DESC);

CREATE TABLE editor_replace_operations (
  operation_id TEXT PRIMARY KEY,
  scope TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  query TEXT NOT NULL CHECK(length(query) BETWEEN 1 AND 4096),
  replacement TEXT NOT NULL CHECK(length(replacement) <= 262144),
  case_sensitive INTEGER NOT NULL CHECK(case_sensitive IN (0,1)),
  author TEXT NOT NULL CHECK(json_valid(author)),
  matches TEXT NOT NULL CHECK(json_valid(matches)),
  total_replaced INTEGER NOT NULL CHECK(total_replaced BETWEEN 1 AND 4294967295),
  created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
  UNIQUE(scope, idempotency_key),
  UNIQUE(workspace_id)
);

CREATE TABLE editor_replace_items (
  operation_id TEXT NOT NULL REFERENCES editor_replace_operations(operation_id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
  file_id TEXT NOT NULL REFERENCES editor_files(file_id) ON DELETE CASCADE,
  relative_path TEXT NOT NULL CHECK(length(relative_path) BETWEEN 1 AND 4096),
  before_hash TEXT NOT NULL CHECK(length(before_hash)=64),
  after_hash TEXT NOT NULL CHECK(length(after_hash)=64),
  replaced INTEGER NOT NULL CHECK(replaced BETWEEN 1 AND 500),
  applied INTEGER NOT NULL CHECK(applied IN (0,1)),
  PRIMARY KEY(operation_id, ordinal),
  UNIQUE(operation_id, file_id),
  UNIQUE(operation_id, relative_path)
);

PRAGMA user_version = 13;

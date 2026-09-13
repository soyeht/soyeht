CREATE TABLE editor_agent_reviews (
  file_id TEXT PRIMARY KEY REFERENCES editor_files(file_id) ON DELETE CASCADE,
  revision INTEGER NOT NULL CHECK(revision >= 0),
  pane_id TEXT NOT NULL REFERENCES panes(id) ON DELETE CASCADE,
  author TEXT NOT NULL CHECK(json_valid(author)),
  baseline_text BLOB NOT NULL CHECK(length(baseline_text) <= 52428800),
  updated_ms INTEGER NOT NULL CHECK(updated_ms >= 0)
);

PRAGMA user_version = 15;

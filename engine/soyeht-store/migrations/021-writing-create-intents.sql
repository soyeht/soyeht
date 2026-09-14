CREATE TABLE writing_create_intents (
  scope TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  operation_id TEXT NOT NULL UNIQUE,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  relative_path TEXT NOT NULL,
  file_id TEXT NOT NULL UNIQUE,
  pane_id TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL CHECK(kind IN ('text','presentation','screenplay')),
  section TEXT NOT NULL CHECK(section IN ('texts','presentations','screenplays')),
  created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
  PRIMARY KEY(scope, idempotency_key),
  CHECK((kind='text' AND section='texts') OR
        (kind='presentation' AND section='presentations') OR
        (kind='screenplay' AND section='screenplays'))
);

PRAGMA user_version = 21;

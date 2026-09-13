CREATE TABLE language_server_checkpoints (
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  language TEXT NOT NULL CHECK(language IN ('rust','swift','typescript','python')),
  payload TEXT NOT NULL CHECK(json_valid(payload) AND length(payload) BETWEEN 2 AND 1048576),
  PRIMARY KEY(workspace_id, language)
);

PRAGMA user_version = 14;

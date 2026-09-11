ALTER TABLE workspaces ADD COLUMN closed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE idempotency ADD COLUMN actor TEXT;
CREATE TABLE client_focus (
  scope TEXT PRIMARY KEY,
  workspace_id TEXT,
  pane_id TEXT
);
CREATE TABLE input_intents (
  scope TEXT NOT NULL,
  key TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  operation_key TEXT NOT NULL,
  supervisor_id TEXT NOT NULL,
  instance_id TEXT NOT NULL,
  PRIMARY KEY(scope,key)
);
CREATE TABLE pair_windows (code TEXT PRIMARY KEY, expires_unix INTEGER NOT NULL);
CREATE TABLE pair_requests (id TEXT PRIMARY KEY, payload TEXT NOT NULL, expires_unix INTEGER NOT NULL);
PRAGMA user_version = 2;

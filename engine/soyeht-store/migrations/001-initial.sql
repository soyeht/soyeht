CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE workspaces (id TEXT PRIMARY KEY, payload TEXT NOT NULL);
CREATE TABLE panes (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id),
  payload TEXT NOT NULL,
  closed INTEGER NOT NULL DEFAULT 0,
  instance_key TEXT NOT NULL UNIQUE,
  instance_id TEXT,
  supervisor_id TEXT,
  sequence_base TEXT NOT NULL DEFAULT '0'
);
CREATE TABLE devices (
  id TEXT PRIMARY KEY,
  payload TEXT NOT NULL,
  public_key BLOB NOT NULL,
  token_hash BLOB NOT NULL UNIQUE,
  revoked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  method TEXT NOT NULL,
  params TEXT NOT NULL,
  created_unix INTEGER NOT NULL
);
CREATE TABLE idempotency (
  scope TEXT NOT NULL,
  key TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  result TEXT NOT NULL,
  created_unix INTEGER NOT NULL,
  PRIMARY KEY (scope, key)
);
CREATE INDEX events_created ON events(created_unix);
CREATE INDEX idempotency_created ON idempotency(created_unix);
PRAGMA user_version = 1;

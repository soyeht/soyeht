-- The avatar library and what binds an avatar to a workspace (ADR 0166, contract 19).
-- An avatar is global to the person; its files live in `<data>/avatars/<id>/`. A binding is the
-- workspace's own: the harness, the speech settings and the persona, and the clients that have
-- voice on (a paired device's id, or `local`). Only identifiers and settings are kept here: a
-- provider key is never stored by the engine. Kinds and states are text the engine validates, so
-- a later value needs no migration. The statements are repeatable, as a database whose version
-- was set back (as the tests of earlier migrations do) runs them again.
CREATE TABLE IF NOT EXISTS avatars (
  id TEXT PRIMARY KEY CHECK(length(id) BETWEEN 1 AND 128),
  name TEXT NOT NULL CHECK(length(name) >= 1),
  kind TEXT NOT NULL,
  source_hash TEXT NOT NULL CHECK(length(source_hash) = 64),
  build_state TEXT NOT NULL DEFAULT 'none',
  failure_kind TEXT,
  package_hash TEXT CHECK(package_hash IS NULL OR length(package_hash) = 64),
  voice_provider TEXT,
  voice_id TEXT,
  created_at TEXT NOT NULL,
  created_seq INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workspace_avatars (
  workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id),
  avatar_id TEXT NOT NULL REFERENCES avatars(id),
  harness TEXT NOT NULL,
  model TEXT,
  effort TEXT,
  hearing_provider TEXT,
  hearing_language TEXT,
  voice_provider TEXT,
  voice_id TEXT,
  persona TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS workspace_avatars_by_avatar ON workspace_avatars(avatar_id);

CREATE TABLE IF NOT EXISTS workspace_avatar_voice (
  workspace_id TEXT NOT NULL REFERENCES workspace_avatars(workspace_id) ON DELETE CASCADE,
  viewer TEXT NOT NULL CHECK(length(viewer) BETWEEN 1 AND 128),
  PRIMARY KEY (workspace_id, viewer)
) WITHOUT ROWID;

PRAGMA user_version = 38;

-- The avatar library and what binds an avatar to a workspace (ADR 0166, contract 19).
-- An avatar is global to the person; its files live in `<data>/avatars/<id>/`. A binding is the
-- workspace's own: the harness, the speech settings and the persona, with a revision that a writer
-- must name, so a client holding an old copy cannot overwrite a newer one. Only identifiers and
-- settings are kept here: a provider key is never stored by the engine, and neither is whether a
-- client has voice on (that is the client's own preference). The kind is text the engine does not
-- constrain, so a later value needs no migration. A binding goes with its workspace when the
-- workspace is closed. The statements are repeatable, as a database whose version was set back
-- (as the tests of earlier migrations do) runs them again.
CREATE TABLE IF NOT EXISTS avatars (
  id TEXT PRIMARY KEY CHECK(length(id) BETWEEN 1 AND 128),
  name TEXT NOT NULL CHECK(length(name) >= 1),
  kind TEXT NOT NULL,
  source_hash TEXT NOT NULL CHECK(length(source_hash) = 64),
  image_provider TEXT,
  image_model TEXT,
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
  persona TEXT NOT NULL DEFAULT '',
  revision INTEGER NOT NULL CHECK(revision >= 1)
);
CREATE INDEX IF NOT EXISTS workspace_avatars_by_avatar ON workspace_avatars(avatar_id);

PRAGMA user_version = 38;

CREATE TABLE writing_documents (
  file_id TEXT PRIMARY KEY REFERENCES editor_files(file_id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK(kind IN ('text','presentation','screenplay')),
  section TEXT NOT NULL CHECK(section IN ('texts','presentations','screenplays')),
  sidecar_revision INTEGER NOT NULL CHECK(sidecar_revision >= 0),
  font TEXT NOT NULL CHECK(font IN ('reading_serif','classic_serif','sans')),
  focus_mode INTEGER NOT NULL CHECK(focus_mode IN (0,1)),
  sidecar_payload TEXT NOT NULL CHECK(length(CAST(sidecar_payload AS BLOB)) <= 1048576),
  updated_ms INTEGER NOT NULL CHECK(updated_ms >= 0),
  CHECK((kind='text' AND section='texts') OR
        (kind='presentation' AND section='presentations') OR
        (kind='screenplay' AND section='screenplays'))
);

CREATE TABLE writing_sidecar_versions (
  file_id TEXT NOT NULL REFERENCES writing_documents(file_id) ON DELETE CASCADE,
  revision INTEGER NOT NULL CHECK(revision > 0),
  author TEXT NOT NULL,
  payload TEXT NOT NULL CHECK(length(CAST(payload AS BLOB)) <= 1048576),
  created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
  PRIMARY KEY(file_id, revision)
);

PRAGMA user_version = 20;

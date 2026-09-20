ALTER TABLE workspaces ADD COLUMN created_seq INTEGER;

UPDATE workspaces
SET created_seq = rowid
WHERE created_seq IS NULL;

CREATE UNIQUE INDEX workspaces_created_sequence
ON workspaces(created_seq);

PRAGMA user_version = 24;

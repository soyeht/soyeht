ALTER TABLE panes ADD COLUMN created_seq INTEGER;

UPDATE panes
SET created_seq = rowid
WHERE created_seq IS NULL;

CREATE UNIQUE INDEX panes_created_sequence
ON panes(created_seq);

PRAGMA user_version = 27;

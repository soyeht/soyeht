-- Who a pane was made for, and what each viewer has been told about devices added (ADR 0164).
-- `panes.created_by` is the paired device whose connection made the pane (the column is added by
-- the migration, which checks it is not there yet); it is null for a pane made by the person at
-- the computer, an agent or a task, and for every pane made before this version. A viewer is the
-- id of a paired device or `local` for the person at the computer; `seen_unix` is the `addedAt`
-- of the newest device that viewer dismissed the notice for. Only an engine that asks for the
-- person's code writes marks; every other engine leaves the table empty.
CREATE INDEX IF NOT EXISTS panes_by_creator ON panes(created_by) WHERE created_by IS NOT NULL;
CREATE TABLE IF NOT EXISTS device_notice_marks (
  viewer TEXT PRIMARY KEY CHECK(length(viewer) BETWEEN 1 AND 128),
  seen_unix INTEGER NOT NULL
) WITHOUT ROWID;

PRAGMA user_version = 37;

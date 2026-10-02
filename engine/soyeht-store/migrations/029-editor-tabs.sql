-- Engine-owned order of a workspace's editor tabs. A tab is a live editor
-- pane; a row only records its position and whether it is the workspace's
-- preview tab. The preview stays a preview while the file keeps the revision
-- it had when it was previewed, so typing never has to update this table.
CREATE TABLE editor_tabs (
  pane_id TEXT PRIMARY KEY REFERENCES panes(id) ON DELETE CASCADE,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
  preview INTEGER NOT NULL CHECK(preview IN (0,1)),
  preview_revision INTEGER CHECK(preview_revision IS NULL OR preview_revision >= 0),
  CHECK(preview = 0 OR preview_revision IS NOT NULL)
);
CREATE INDEX editor_tabs_workspace ON editor_tabs(workspace_id, ordinal);

PRAGMA user_version = 29;

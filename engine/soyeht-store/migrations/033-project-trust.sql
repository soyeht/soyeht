-- Whether the person trusts a project folder (ADR 0150). One row per canonical folder, written
-- only by a decision of the person: a folder without a row is undecided and is treated as
-- restricted. The folder, not the workspace, is the key: two workspaces over one folder share it,
-- and a folder keeps its answer when its workspace is closed and opened again.
CREATE TABLE project_trust (
  root TEXT PRIMARY KEY CHECK(length(root) BETWEEN 1 AND 4096),
  state TEXT NOT NULL CHECK(state IN ('restricted','trusted')),
  decided_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

PRAGMA user_version = 33;

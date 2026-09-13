CREATE TABLE web_session_grants (
  grant_id INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  domain TEXT NOT NULL CHECK(length(domain) BETWEEN 1 AND 255),
  scope TEXT NOT NULL CHECK(scope IN ('task','workspace')),
  task_id TEXT,
  granted_by TEXT NOT NULL CHECK(length(granted_by) BETWEEN 1 AND 128),
  created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
  CHECK(
    (scope = 'task' AND task_id IS NOT NULL) OR
    (scope = 'workspace' AND task_id IS NULL)
  ),
  FOREIGN KEY(workspace_id, task_id)
    REFERENCES tasks(workspace_id, task_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX web_session_grants_scope
  ON web_session_grants(workspace_id, domain, scope, ifnull(task_id, ''));

PRAGMA user_version = 16;

CREATE TABLE web_permission_requests (
  request_id TEXT PRIMARY KEY CHECK(length(request_id) BETWEEN 1 AND 128),
  pane_id TEXT NOT NULL REFERENCES panes(id) ON DELETE CASCADE,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 4294967295),
  task_id TEXT,
  kind TEXT NOT NULL CHECK(kind IN ('web_session','web_evaluate')),
  domain TEXT NOT NULL CHECK(length(domain) BETWEEN 1 AND 255),
  public TEXT NOT NULL CHECK(json_valid(public)),
  expires_ms INTEGER NOT NULL CHECK(expires_ms >= 0),
  result TEXT CHECK(result IS NULL OR json_valid(result)),
  FOREIGN KEY(workspace_id, task_id)
    REFERENCES tasks(workspace_id, task_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX web_permission_requests_open
  ON web_permission_requests(pane_id, kind, domain)
  WHERE result IS NULL;

PRAGMA user_version = 17;

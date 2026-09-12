CREATE TABLE workspace_task_counters (
  workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id),
  next_number INTEGER NOT NULL CHECK(next_number > 0)
);

CREATE TABLE tasks (
  workspace_id TEXT NOT NULL REFERENCES workspaces(id),
  task_id TEXT NOT NULL,
  ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
  state TEXT NOT NULL CHECK(state IN ('todo','in_progress','in_review','completed','cancelled')),
  priority_rank INTEGER NOT NULL CHECK(priority_rank BETWEEN 0 AND 3),
  owner_pane_id TEXT REFERENCES panes(id),
  planned_owner TEXT,
  executor_harness TEXT,
  blocking_kind TEXT CHECK(blocking_kind IS NULL OR blocking_kind IN ('dependency','missing_information','external_error')),
  version INTEGER NOT NULL CHECK(version >= 1),
  payload TEXT NOT NULL CHECK(json_valid(payload)),
  UNIQUE(workspace_id, task_id)
);
CREATE INDEX tasks_claimable ON tasks(workspace_id,state,blocking_kind,priority_rank,ordinal);
CREATE INDEX tasks_owner ON tasks(owner_pane_id,state);

CREATE TABLE task_dependencies (
  workspace_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  dependency_task_id TEXT NOT NULL,
  PRIMARY KEY(workspace_id,task_id,dependency_task_id),
  FOREIGN KEY(workspace_id,task_id) REFERENCES tasks(workspace_id,task_id) ON DELETE CASCADE,
  FOREIGN KEY(workspace_id,dependency_task_id) REFERENCES tasks(workspace_id,task_id)
);

CREATE TABLE task_subitems (
  workspace_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  subitem_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  payload TEXT NOT NULL CHECK(json_valid(payload)),
  PRIMARY KEY(workspace_id,task_id,subitem_id),
  UNIQUE(workspace_id,task_id,ordinal),
  FOREIGN KEY(workspace_id,task_id) REFERENCES tasks(workspace_id,task_id) ON DELETE CASCADE
);

CREATE TABLE task_comments (
  ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  payload TEXT NOT NULL CHECK(json_valid(payload)),
  FOREIGN KEY(workspace_id,task_id) REFERENCES tasks(workspace_id,task_id) ON DELETE CASCADE
);
CREATE INDEX task_comments_page ON task_comments(workspace_id,task_id,ordinal);

CREATE TABLE task_attempts (
  workspace_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  attempt INTEGER NOT NULL CHECK(attempt > 0),
  payload TEXT NOT NULL CHECK(json_valid(payload)),
  PRIMARY KEY(workspace_id,task_id,attempt),
  FOREIGN KEY(workspace_id,task_id) REFERENCES tasks(workspace_id,task_id) ON DELETE CASCADE
);

CREATE TABLE task_moves (
  ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  payload TEXT NOT NULL CHECK(json_valid(payload)),
  FOREIGN KEY(workspace_id,task_id) REFERENCES tasks(workspace_id,task_id) ON DELETE CASCADE
);
CREATE INDEX task_moves_history ON task_moves(workspace_id,task_id,ordinal);

CREATE TABLE task_loops (
  pane_id TEXT PRIMARY KEY REFERENCES panes(id) ON DELETE CASCADE,
  payload TEXT NOT NULL CHECK(json_valid(payload)),
  next_tick_ms INTEGER NOT NULL CHECK(next_tick_ms >= 0)
);

CREATE TABLE task_feedback (
  workspace_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  delivery_count INTEGER NOT NULL CHECK(delivery_count BETWEEN 0 AND 3),
  armed INTEGER NOT NULL CHECK(armed IN (0,1)),
  PRIMARY KEY(workspace_id,task_id),
  FOREIGN KEY(workspace_id,task_id) REFERENCES tasks(workspace_id,task_id) ON DELETE CASCADE
);

CREATE TABLE task_dispatch (
  workspace_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  stage TEXT NOT NULL CHECK(stage IN ('reserved','worktree_created','pane_opened','prompt_accepted')),
  worktree_path TEXT,
  branch TEXT,
  pane_id TEXT,
  PRIMARY KEY(workspace_id,task_id),
  FOREIGN KEY(workspace_id,task_id) REFERENCES tasks(workspace_id,task_id) ON DELETE CASCADE
);

PRAGMA user_version = 7;

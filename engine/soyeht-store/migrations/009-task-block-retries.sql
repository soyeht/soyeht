CREATE TABLE task_block_retries (
  workspace_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  due_ms INTEGER NOT NULL CHECK(due_ms >= 0),
  PRIMARY KEY(workspace_id, task_id),
  FOREIGN KEY(workspace_id, task_id) REFERENCES tasks(workspace_id, task_id) ON DELETE CASCADE
);

CREATE INDEX task_block_retries_due ON task_block_retries(due_ms);

PRAGMA user_version = 9;

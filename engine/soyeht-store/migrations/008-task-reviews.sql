CREATE TABLE task_reviews (
  workspace_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  attempt INTEGER NOT NULL CHECK(attempt > 0),
  version INTEGER NOT NULL CHECK(version >= 1),
  stage INTEGER NOT NULL CHECK(stage BETWEEN 0 AND 4),
  reviewer_pane_id TEXT NOT NULL,
  payload TEXT NOT NULL CHECK(json_valid(payload)),
  PRIMARY KEY(workspace_id,task_id,attempt),
  FOREIGN KEY(workspace_id,task_id,attempt)
    REFERENCES task_attempts(workspace_id,task_id,attempt) ON DELETE CASCADE
);
CREATE INDEX task_reviews_unresolved ON task_reviews(stage,workspace_id,task_id,attempt);

PRAGMA user_version = 8;

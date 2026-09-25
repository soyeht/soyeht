CREATE TABLE harness_readiness (
  harness_id TEXT PRIMARY KEY,
  state TEXT NOT NULL,
  reason TEXT,
  last_ready_ms INTEGER,
  updated_ms INTEGER NOT NULL
);
PRAGMA user_version = 26;

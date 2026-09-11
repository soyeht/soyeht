CREATE TABLE agent_runtime (
  pane_id TEXT PRIMARY KEY REFERENCES panes(id),
  generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 4294967295),
  payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE TABLE agent_outbox (
  ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
  operation_key TEXT NOT NULL UNIQUE,
  pane_id TEXT NOT NULL REFERENCES agent_runtime(pane_id),
  generation INTEGER NOT NULL,
  instance_id TEXT NOT NULL,
  supervisor_id TEXT NOT NULL,
  bytes BLOB NOT NULL CHECK(length(bytes) BETWEEN 1 AND 1048577),
  accepted_bytes INTEGER NOT NULL DEFAULT 0 CHECK(accepted_bytes BETWEEN 0 AND length(bytes)),
  attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt BETWEEN 0 AND 4294967295)
);
CREATE INDEX agent_outbox_pane ON agent_outbox(pane_id, ordinal);
CREATE TABLE agent_requests (
  request_id TEXT PRIMARY KEY,
  pane_id TEXT NOT NULL REFERENCES agent_runtime(pane_id),
  generation INTEGER NOT NULL,
  source_seq BLOB NOT NULL CHECK(length(source_seq)=8),
  native_id TEXT NOT NULL CHECK(json_valid(native_id)),
  method TEXT NOT NULL,
  params TEXT NOT NULL CHECK(json_valid(params)),
  public TEXT NOT NULL CHECK(json_valid(public)),
  expires_ms INTEGER NOT NULL,
  result TEXT CHECK(result IS NULL OR json_valid(result)),
  UNIQUE(pane_id, generation, source_seq)
);
CREATE INDEX agent_requests_open ON agent_requests(pane_id, result);
CREATE TABLE agent_transcript (
  pane_id TEXT NOT NULL REFERENCES agent_runtime(pane_id),
  seq BLOB NOT NULL CHECK(length(seq)=8),
  local_id TEXT NOT NULL,
  generation INTEGER NOT NULL,
  payload TEXT NOT NULL CHECK(json_valid(payload)),
  bytes INTEGER NOT NULL CHECK(bytes BETWEEN 1 AND 262144),
  PRIMARY KEY(pane_id, seq),
  UNIQUE(pane_id, local_id)
);
CREATE TABLE agent_probes (
  fingerprint TEXT NOT NULL,
  method TEXT NOT NULL,
  payload TEXT NOT NULL CHECK(json_valid(payload)),
  PRIMARY KEY(fingerprint, method)
);
CREATE TABLE agent_prompt_queue (
  ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
  prompt_id TEXT NOT NULL UNIQUE,
  pane_id TEXT NOT NULL REFERENCES agent_runtime(pane_id),
  payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE INDEX agent_prompt_queue_pane ON agent_prompt_queue(pane_id,ordinal);
PRAGMA user_version = 4;

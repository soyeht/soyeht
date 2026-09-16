CREATE TABLE agent_model_refusals (
  harness_id TEXT NOT NULL CHECK(length(harness_id) BETWEEN 1 AND 128),
  model_id TEXT NOT NULL CHECK(length(model_id) BETWEEN 1 AND 128),
  reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB)) BETWEEN 1 AND 4096),
  observed_at TEXT NOT NULL CHECK(length(observed_at) BETWEEN 20 AND 35),
  PRIMARY KEY(harness_id, model_id)
);

CREATE INDEX agent_model_refusals_observed
ON agent_model_refusals(observed_at, harness_id, model_id);

PRAGMA user_version = 22;

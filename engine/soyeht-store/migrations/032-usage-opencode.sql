-- The usage readings table accepts the OpenCode provider: SQLite cannot change a CHECK, so the
-- table is rebuilt with the rows it holds. A database from before the table (a test's, or one
-- that never ran version 11) gets the old table first, so the copy has something to read.
CREATE TABLE IF NOT EXISTS usage_readings (
  provider TEXT PRIMARY KEY CHECK(provider IN ('claude','codex','cursor')),
  observed_at_unix_sec INTEGER NOT NULL CHECK(observed_at_unix_sec >= 0),
  payload TEXT NOT NULL CHECK(json_valid(payload))
);

CREATE TABLE usage_readings_new (
  provider TEXT PRIMARY KEY CHECK(provider IN ('claude','codex','cursor','opencode')),
  observed_at_unix_sec INTEGER NOT NULL CHECK(observed_at_unix_sec >= 0),
  payload TEXT NOT NULL CHECK(json_valid(payload))
);

INSERT INTO usage_readings_new SELECT provider, observed_at_unix_sec, payload FROM usage_readings;
DROP TABLE usage_readings;
ALTER TABLE usage_readings_new RENAME TO usage_readings;

PRAGMA user_version = 32;

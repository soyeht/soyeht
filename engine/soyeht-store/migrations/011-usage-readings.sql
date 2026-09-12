CREATE TABLE usage_readings (
  provider TEXT PRIMARY KEY CHECK(provider IN ('claude','codex','cursor')),
  observed_at_unix_sec INTEGER NOT NULL CHECK(observed_at_unix_sec >= 0),
  payload TEXT NOT NULL CHECK(json_valid(payload))
);

PRAGMA user_version = 11;

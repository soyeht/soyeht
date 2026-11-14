-- The tailnet nodes that each device's authenticated `hello` came from, and the tailnet identity
-- the engine pinned when it first served a pairing (ADR 0164). Only an engine that asks for the
-- person's code writes them; every other engine leaves both tables empty. The tables are created
-- if they are missing, so a database that is migrated again does not fail.
CREATE TABLE IF NOT EXISTS device_nodes (
  device_id TEXT NOT NULL REFERENCES devices(id),
  node_id TEXT NOT NULL CHECK(length(node_id) BETWEEN 1 AND 256),
  first_seen_unix INTEGER NOT NULL,
  last_seen_unix INTEGER NOT NULL,
  revoked_unix INTEGER,
  PRIMARY KEY(device_id, node_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS device_nodes_by_node ON device_nodes(node_id);

CREATE TABLE IF NOT EXISTS pairing_tailnet (
  singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
  tailnet TEXT NOT NULL CHECK(length(tailnet) BETWEEN 1 AND 256),
  node_key TEXT NOT NULL CHECK(length(node_key) BETWEEN 1 AND 256),
  pinned_unix INTEGER NOT NULL
);

PRAGMA user_version = 36;

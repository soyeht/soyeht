-- The packages a person acknowledged a built-in adapter downloads and runs unchecked (ADR 0143).
-- One row per `name@version`: a new version of the adapter has no row and asks again. The
-- table is created if it is missing, so a database that is migrated again does not fail.
CREATE TABLE IF NOT EXISTS adapter_acknowledgements (
  package TEXT PRIMARY KEY CHECK(length(package) BETWEEN 1 AND 256),
  acknowledged_ms INTEGER NOT NULL
);

PRAGMA user_version = 33;

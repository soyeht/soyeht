-- One APNs registration per paired device. Revoking the device removes its
-- registration. A token is unique: when a phone pairs again it keeps its APNs
-- token under a new device id, and the newer registration wins.
CREATE TABLE push_registrations (
  device_id TEXT PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
  token TEXT NOT NULL UNIQUE,
  environment TEXT NOT NULL CHECK (environment IN ('sandbox', 'production')),
  updated_unix INTEGER NOT NULL
);

PRAGMA user_version = 28;

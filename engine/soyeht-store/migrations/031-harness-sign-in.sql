-- The protocol sign-in method the person chose for a harness. A new agent process of that
-- harness is authenticated with it before it opens a session (ADR 0100). The method id is
-- the only thing kept: never a credential.
CREATE TABLE harness_sign_in (
  harness_id TEXT PRIMARY KEY CHECK(length(harness_id) BETWEEN 1 AND 128),
  method_id TEXT NOT NULL CHECK(length(method_id) BETWEEN 1 AND 256)
);

PRAGMA user_version = 31;

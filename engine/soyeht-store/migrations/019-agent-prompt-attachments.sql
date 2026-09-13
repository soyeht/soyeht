CREATE TABLE agent_prompt_attachments (
  prompt_id TEXT NOT NULL REFERENCES agent_prompt_queue(prompt_id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL CHECK(ordinal >= 0 AND ordinal < 20),
  attachment_id TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  name TEXT NOT NULL,
  bytes BLOB NOT NULL CHECK(length(bytes) > 0 AND length(bytes) <= 2097152),
  PRIMARY KEY(prompt_id, ordinal),
  UNIQUE(prompt_id, attachment_id)
);
PRAGMA user_version = 19;

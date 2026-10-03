-- Prompt attachments are engine-owned files named in the queued prompt
-- payload; their bytes no longer live in the database.
DROP TABLE IF EXISTS agent_prompt_attachments;
PRAGMA user_version = 30;

-- When an input's intent was recorded: an intent that nobody retries is forgotten a minute after
-- it was recorded, not a minute after an engine first saw it. The intents a database holds when
-- it is migrated count from now.
ALTER TABLE input_intents ADD COLUMN created_unix INTEGER NOT NULL DEFAULT 0;
UPDATE input_intents SET created_unix = CAST(strftime('%s', 'now') AS INTEGER);

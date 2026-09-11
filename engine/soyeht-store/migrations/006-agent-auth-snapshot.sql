-- Agent authentication choices are part of the reconnectable pane snapshot.
UPDATE panes
SET payload = json_set(payload, '$.agent.authMethods', json('[]'))
WHERE json_type(payload, '$.agent') = 'object'
  AND json_type(payload, '$.agent.authMethods') IS NULL;
PRAGMA user_version = 6;

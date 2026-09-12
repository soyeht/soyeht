UPDATE panes
SET payload = json_set(payload, '$.agent.role', 'worker')
WHERE json_type(payload, '$.agent') = 'object'
  AND json_type(payload, '$.agent.role') IS NULL;

PRAGMA user_version = 10;

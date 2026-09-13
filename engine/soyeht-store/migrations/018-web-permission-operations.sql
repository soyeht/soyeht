CREATE UNIQUE INDEX web_permission_requests_operation
  ON web_permission_requests(
    pane_id,
    kind,
    json_extract(public, '$.promptId')
  );

PRAGMA user_version = 18;

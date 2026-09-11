-- Native load output remains private until the load response confirms success.
CREATE TABLE agent_replay_transcript (
    pane_id TEXT NOT NULL REFERENCES agent_runtime(pane_id),
    seq BLOB NOT NULL CHECK(length(seq) = 8),
    local_id TEXT NOT NULL UNIQUE,
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 4294967295),
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    bytes INTEGER NOT NULL CHECK(bytes BETWEEN 1 AND 262144),
    PRIMARY KEY(pane_id, seq)
);
PRAGMA user_version = 5;
-- Interactive sign-in uses a private launch command, never public pane JSON.
CREATE TABLE pane_launches (
    pane_id TEXT PRIMARY KEY REFERENCES panes(id),
    payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE TABLE agent_attention (
    ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
    attention_id TEXT NOT NULL UNIQUE,
    pane_id TEXT NOT NULL REFERENCES panes(id),
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE INDEX agent_attention_workspace ON agent_attention(workspace_id, ordinal);

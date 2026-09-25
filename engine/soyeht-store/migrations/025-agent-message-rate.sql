CREATE TABLE agent_message_rate (
  sender_pane_id TEXT NOT NULL,
  target_pane_id TEXT NOT NULL,
  sent_at_ms INTEGER NOT NULL
);
CREATE INDEX agent_message_rate_pair_time
ON agent_message_rate(sender_pane_id, target_pane_id, sent_at_ms);
PRAGMA user_version = 25;

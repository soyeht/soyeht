-- The size of a pane's transcript, kept as a running total (ADR 0020). Saving a message used to
-- read `SUM(bytes)` over every row of the pane to apply the retention limit, which grew with the
-- pane's history. The triggers keep the total in the same transaction as every insert, update and
-- delete of a transcript row, whichever statement makes it (a save, a retention trim, a replay
-- publication), so no writer can forget it. `staged` is 0 for `agent_transcript` and 1 for
-- `agent_replay_transcript`. The statements are repeatable, as a database whose version was set
-- back (as the tests of earlier migrations do) runs them again.
CREATE TABLE IF NOT EXISTS agent_transcript_total (
  pane_id TEXT NOT NULL,
  staged INTEGER NOT NULL CHECK(staged IN (0, 1)),
  bytes INTEGER NOT NULL CHECK(bytes >= 0),
  PRIMARY KEY(pane_id, staged)
) WITHOUT ROWID;

DELETE FROM agent_transcript_total;
INSERT INTO agent_transcript_total(pane_id, staged, bytes)
  SELECT pane_id, 0, SUM(bytes) FROM agent_transcript GROUP BY pane_id;
INSERT INTO agent_transcript_total(pane_id, staged, bytes)
  SELECT pane_id, 1, SUM(bytes) FROM agent_replay_transcript GROUP BY pane_id;

CREATE TRIGGER IF NOT EXISTS agent_transcript_total_insert AFTER INSERT ON agent_transcript BEGIN
  INSERT INTO agent_transcript_total(pane_id, staged, bytes) VALUES (NEW.pane_id, 0, NEW.bytes)
    ON CONFLICT(pane_id, staged) DO UPDATE SET bytes = bytes + excluded.bytes;
END;
CREATE TRIGGER IF NOT EXISTS agent_transcript_total_update AFTER UPDATE ON agent_transcript BEGIN
  UPDATE agent_transcript_total SET bytes = bytes - OLD.bytes
    WHERE pane_id = OLD.pane_id AND staged = 0;
  INSERT INTO agent_transcript_total(pane_id, staged, bytes) VALUES (NEW.pane_id, 0, NEW.bytes)
    ON CONFLICT(pane_id, staged) DO UPDATE SET bytes = bytes + excluded.bytes;
END;
CREATE TRIGGER IF NOT EXISTS agent_transcript_total_delete AFTER DELETE ON agent_transcript BEGIN
  UPDATE agent_transcript_total SET bytes = bytes - OLD.bytes
    WHERE pane_id = OLD.pane_id AND staged = 0;
END;

CREATE TRIGGER IF NOT EXISTS agent_replay_total_insert AFTER INSERT ON agent_replay_transcript BEGIN
  INSERT INTO agent_transcript_total(pane_id, staged, bytes) VALUES (NEW.pane_id, 1, NEW.bytes)
    ON CONFLICT(pane_id, staged) DO UPDATE SET bytes = bytes + excluded.bytes;
END;
CREATE TRIGGER IF NOT EXISTS agent_replay_total_update AFTER UPDATE ON agent_replay_transcript BEGIN
  UPDATE agent_transcript_total SET bytes = bytes - OLD.bytes
    WHERE pane_id = OLD.pane_id AND staged = 1;
  INSERT INTO agent_transcript_total(pane_id, staged, bytes) VALUES (NEW.pane_id, 1, NEW.bytes)
    ON CONFLICT(pane_id, staged) DO UPDATE SET bytes = bytes + excluded.bytes;
END;
CREATE TRIGGER IF NOT EXISTS agent_replay_total_delete AFTER DELETE ON agent_replay_transcript BEGIN
  UPDATE agent_transcript_total SET bytes = bytes - OLD.bytes
    WHERE pane_id = OLD.pane_id AND staged = 1;
END;

PRAGMA user_version = 39;

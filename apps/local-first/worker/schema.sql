-- Relay log. `position` is the only valid cross-device order: the writer's `seq`
-- is per device, so two writers both produce 1, 2, 3.
CREATE TABLE IF NOT EXISTS events (
  position      INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id    TEXT    NOT NULL,
  event_id      TEXT    NOT NULL,
  seq           INTEGER NOT NULL,
  device_id     TEXT    NOT NULL,
  kind          TEXT    NOT NULL,
  payload       TEXT    NOT NULL,
  created_at_ms INTEGER NOT NULL,
  fence         INTEGER NOT NULL DEFAULT 0,
  UNIQUE (session_id, event_id)
);

-- One row per session: the newest fencing token the relay has accepted.
CREATE TABLE IF NOT EXISTS sessions (
  session_id    TEXT PRIMARY KEY,
  current_fence INTEGER NOT NULL DEFAULT 0
);

-- The fence advances in the same transaction as the insert that carried it, so
-- two racing pushes cannot both be accepted on the strength of a stale read.
CREATE TRIGGER IF NOT EXISTS events_advance_fence
AFTER INSERT ON events
BEGIN
  INSERT INTO sessions (session_id, current_fence)
  VALUES (NEW.session_id, NEW.fence)
  ON CONFLICT (session_id)
  DO UPDATE SET current_fence = MAX(current_fence, excluded.current_fence);
END;

CREATE INDEX IF NOT EXISTS idx_events_session_position
  ON events (session_id, position);

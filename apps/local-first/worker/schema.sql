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
  UNIQUE (session_id, event_id)
);

CREATE INDEX IF NOT EXISTS idx_events_session_position
  ON events (session_id, position);

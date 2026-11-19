// The DB owner. OPFS sync access handles only exist in a dedicated worker, so
// the page never touches SQLite directly (same shape as Android's Tauri commands).
import sqlite3InitModule from "../node_modules/@sqlite.org/sqlite-wasm/dist/index.mjs";

let db;

function send(message) {
  self.postMessage(message);
}

function load() {
  const events = [];
  db.exec({
    sql: "SELECT session_id, event_id, seq, device_id, kind, payload, created_at_ms, fence FROM events ORDER BY rowid",
    rowMode: "object",
    callback: (row) => {
      events.push({
        session_id: row.session_id,
        event_id: row.event_id,
        seq: row.seq,
        device_id: row.device_id,
        kind: row.kind,
        payload: JSON.parse(row.payload),
        created_at_ms: row.created_at_ms,
        fence: row.fence ?? null,
      });
    },
  });
  return events;
}

self.onmessage = (event) => {
  const message = event.data;
  try {
    if (message.type === "load") {
      send({ id: message.id, events: load() });
      return;
    }
    if (message.type === "append") {
      db.exec({
        sql: `INSERT OR IGNORE INTO events
              (session_id, event_id, seq, device_id, kind, payload, created_at_ms, fence)
              VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
        bind: [
          message.event.session_id,
          message.event.event_id,
          message.event.seq,
          message.event.device_id,
          message.event.kind,
          JSON.stringify(message.event.payload ?? null),
          message.event.created_at_ms,
          message.event.fence ?? null,
        ],
      });
      send({ id: message.id, events: load() });
    }
  } catch (error) {
    send({ id: message.id, error: String(error?.message ?? error) });
  }
};

const sqlite3 = await sqlite3InitModule({ print: () => {}, printErr: (m) => console.error(m) });
db = new sqlite3.oo1.OpfsDb("infinity-app.db");
db.exec(`CREATE TABLE IF NOT EXISTS events(
  session_id TEXT NOT NULL,
  event_id TEXT PRIMARY KEY,
  seq INTEGER NOT NULL,
  device_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  payload TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  fence INTEGER
)`);
send({ type: "ready" });

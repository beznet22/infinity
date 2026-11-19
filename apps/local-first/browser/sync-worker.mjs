// Browser device: wa-sqlite (sync build) over OPFS, syncing with the same
// Cloudflare Worker + D1 the CLI and Android use.
//
// OPFS sync access handles only exist in a dedicated worker, which is also where
// wa-sqlite's sync build must run.
import SQLiteESMFactory from "./node_modules/wa-sqlite/dist/wa-sqlite.mjs";
import * as SQLite from "./node_modules/wa-sqlite/src/sqlite-api.js";
// wa-sqlite 1.0.0 ships AccessHandlePoolVFS (sync, one connection) and an
// async OPFS VFS; the sync build must use the former.
import { AccessHandlePoolVFS } from "./node_modules/wa-sqlite/src/examples/AccessHandlePoolVFS.js";

const RELAY = "https://local-first-sync.beznet22.workers.dev";
const SESSION = "cloud1";
const DEVICE = "browser";

const post = (payload) => self.postMessage(payload);

try {
  const module = await SQLiteESMFactory();
  const sqlite3 = SQLite.Factory(module);
  // 1.0.0 exposes a class, not a `create()` factory: construct and register.
const vfs = new AccessHandlePoolVFS("local-first-vfs");
await vfs.isReady;
  sqlite3.vfs_register(vfs, true);

  const db = await sqlite3.open_v2("local-first.db");
  await sqlite3.exec(db, `
    CREATE TABLE IF NOT EXISTS events (
      event_id   TEXT PRIMARY KEY,
      session_id TEXT NOT NULL,
      seq        INTEGER NOT NULL,
      device_id  TEXT NOT NULL,
      kind       TEXT NOT NULL,
      payload    TEXT NOT NULL,
      created_at INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS sync_state (
      session_id TEXT PRIMARY KEY,
      position   INTEGER NOT NULL
    );
  `);

  const scalar = async (sql) => {
    let value = null;
    await sqlite3.exec(db, sql, (row) => { value = row[0]; });
    return value;
  };

  const before = await scalar("SELECT COUNT(*) FROM events");

  // 1. Local write, like an agent turn would produce.
  const seq = Number(await scalar("SELECT COALESCE(MAX(seq), 0) FROM events")) + 1;
  const event = {
    session_id: SESSION,
    event_id: crypto.randomUUID(),
    seq,
    device_id: DEVICE,
    kind: "user_message",
    payload: { text: "hello from the browser", at: Date.now() },
    created_at_ms: Date.now(),
  };
  await sqlite3.run(db, 
    `INSERT OR IGNORE INTO events (event_id, session_id, seq, device_id, kind, payload, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?)`,
    [event.event_id, SESSION, seq, DEVICE, event.kind, JSON.stringify(event.payload), event.created_at_ms],
  );

  // 2. Push it to the Worker (D1 dedupes by event id).
  const pushResponse = await fetch(`${RELAY}/v1/sync/push`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ events: [event] }),
  });
  const pushed = await pushResponse.json();

  // 3. Pull everything after our stored position and apply it.
  const position = Number(
    (await scalar(`SELECT position FROM sync_state WHERE session_id = '${SESSION}'`)) ?? 0,
  );
  const pulled = await (await fetch(`${RELAY}/v1/sync/pull?session=${SESSION}&after=${position}`)).json();

  let highest = position;
  for (const row of pulled) {
    await sqlite3.run(db, 
      `INSERT OR IGNORE INTO events (event_id, session_id, seq, device_id, kind, payload, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?)`,
      [row.event_id, row.session_id, row.seq, row.device_id, row.kind, JSON.stringify(row.payload), row.created_at_ms],
    );
    highest = Math.max(highest, row.position);
  }
  await sqlite3.run(db, 
    `INSERT INTO sync_state (session_id, position) VALUES (?, ?)
     ON CONFLICT(session_id) DO UPDATE SET position = excluded.position`,
    [SESSION, highest],
  );

  const after = await scalar("SELECT COUNT(*) FROM events");
  const devices = await scalar("SELECT GROUP_CONCAT(DISTINCT device_id) FROM events");

  await sqlite3.close(db);

  post({
    // `libversion` is a function in wa-sqlite 1.0.0; functions cannot be
    // structured-cloned to the page.
    sqlite: typeof sqlite3.libversion === "function" ? sqlite3.libversion() : String(sqlite3.libversion),
    opfs: true,
    before,
    after,
    devices,
    pushed: pushed.accepted,
    fetched: pulled.length,
    position: highest,
  });
} catch (error) {
  post({ error: String(error && error.message ? error.message : error) });
}

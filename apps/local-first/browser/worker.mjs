// Dedicated worker: OPFS sync access handles only exist here.
// The package exports `default`, not a named `sqlite3InitModule`.
import sqlite3InitModule from "./node_modules/@sqlite.org/sqlite-wasm/dist/index.mjs";

const ROWS_PER_RUN = 100;

function report(payload) {
  self.postMessage(payload);
}

try {
  const sqlite3 = await sqlite3InitModule({
    print: () => {},
    printErr: (msg) => console.error(msg),
  });

  const version = sqlite3.version.libVersion;
  const opfsAvailable = !!sqlite3.capi.sqlite3_vfs_find("opfs");

  // OpfsDb opens the OPFS VFS; it only works in a dedicated worker.
  const db = new sqlite3.oo1.OpfsDb("spike.db");

  db.exec(`CREATE TABLE IF NOT EXISTS rows(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL
  )`);

  const before = db.selectValue("SELECT count(*) FROM rows");
  db.exec("BEGIN");
  db.exec({
    sql: "INSERT INTO rows(payload, created_at) VALUES(?, ?)",
    bind: [`run-${Date.now()}`, Date.now()],
    // 100 inserts per run: cheap, but enough to see persistence.
  });
  for (let i = 1; i < ROWS_PER_RUN; i++) {
    db.exec({
      sql: "INSERT INTO rows(payload, created_at) VALUES(?, ?)",
      bind: [`payload-${i}`, Date.now()],
    });
  }
  db.exec("COMMIT");

  const after = db.selectValue("SELECT count(*) FROM rows");
  const newest = db.selectValue("SELECT payload FROM rows ORDER BY id DESC LIMIT 1");
  db.close();

  report({
    sqlite: version,
    opfs: opfsAvailable,
    rowsBefore: before,
    rowsAfter: after,
    newest,
    persisted: before > 0,
  });
} catch (error) {
  report({ error: String(error && error.message ? error.message : error) });
}

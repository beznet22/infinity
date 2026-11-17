# Browser storage spike: SQLite-Wasm + OPFS

A throwaway-free proof that the storage direction for the consuming browser app
works today: **SQLite compiled to WebAssembly, persisted in OPFS**, running in a
dedicated worker.

```bash
pnpm install          # @sqlite.org/sqlite-wasm 3.54.1-build1
python3 serve.py      # http://127.0.0.1:8123 (sets COOP/COEP)
google-chrome --headless=new --remote-debugging-port=9400 \
  --user-data-dir=/tmp/chrome-sqlite-spike http://127.0.0.1:8123/
```

Each page load inserts 100 rows and reports the row count via `window.__spikeResult`.
Reload the page: the count grows, which is the persistence check.

## Verified (headless Chrome, 2026-11-17)

```json
{ "sqlite": "3.54.1", "opfs": true, "rowsBefore": 0,   "rowsAfter": 100 }
{ "sqlite": "3.54.1", "opfs": true, "rowsBefore": 100, "rowsAfter": 200 }
```

`persistence: PASS (100 -> 200 rows)`

## Cost

| Asset | Raw | gzip |
|---|---|---|
| `sqlite3.wasm` | 879,084 B (858 KB) | 407,552 B (398 KB) |
| `index.mjs` (JS API) | 656,872 B (641 KB) | — |

## Constraints this proves out

- A **dedicated worker** is mandatory: `createSyncAccessHandle()` is exposed
  only there (not window, shared worker, or service worker).
- One writer per file; multi-tab use needs coordination (SharedWorker electing
  one active tab).
- Durability across process death needs `navigator.storage.persist()` from the
  main thread; OPFS data is still quota-managed and clearable by the user.
- Browser floor: Chrome 102+, Firefox 111+, Safari 15.2+ (sync handles) /
  16.4+ (full OPFS).

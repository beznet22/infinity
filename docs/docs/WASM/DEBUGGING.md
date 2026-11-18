# Debugging the hybrid runtime: Android, desktop and browser

**Why this document exists:** most of the time lost on this branch came from
failures that look like hangs, not errors. The fixes are usually small once the
right instrument is attached. This is the field guide for attaching it.

**Evidence labels used below:**

- ✅ **exercised on this branch** — the command or technique produced the stated
  evidence in these sessions.
- 🔶 **partially exercised** — works, but only in a narrower configuration than a
  product needs.
- ⬜ **runbook, not yet verified here** — plausible next step, explicitly not
  claimed as tested.

Read [`HANDOFF.md`](./HANDOFF.md) first for what is production-ready and what is
not. This document is about **seeing what the code is doing**, not about
declaring it finished.

---

## 1. Pick the instrument, not the guess

| Layer | Instrument | What it can answer |
|---|---|---|
| Rust logic, storage folds | `cargo test`, temp-root CLI, SQLite inspection | Did the event append/lease/view math change? |
| Native desktop shell | whole-app build, `RUST_LOG`, app-data artifacts | Did the host wire the runtime at all? |
| Android process | `adb logcat`, artifacts via `run-as`, `screencap` | Did the app start, panic, or stall? |
| Android UI | `uiautomator dump`, screenshots, trusted `MotionEvent` taps | Is the DOM/WebView visible and interactive? |
| Android WebView JS | CDP over `adb forward` | What does the page think, and what input does it accept? |
| Browser page | headless Chrome + CDP | Does the real page load, render, and persist? |
| Browser workers | CDP target list / worker console | Is this an OPFS/worker failure rather than UI logic? |
| Local relay | `wrangler dev` + `curl` | Is it auth, schema, ordering, or the client? |
| Size/build | CI gates and scripts | Did the change silently exceed budget? |

The recurring rule: **attach an observer before changing code**. Rebuilding to
add a `println!` is slower than reading an artifact the app already writes.

---

## 2. First five checks (before any deep debugging)

Run these in order; each one eliminates a whole class of confusion.

```bash
# 1. Compile the exact target that failed.
cargo check --manifest-path apps/local-first/android/Cargo.toml --lib

# 2. Run the reference tests, not just the one that seemed relevant.
cd apps/local-first && cargo test --workspace

# 3. Prove the JS/Rust view contract has not drifted.
node browser/view/test.mjs

# 4. Typecheck the Worker for real (a missing tsconfig makes this a no-op).
cd worker && pnpm exec tsc --noEmit -p .

# 5. Confirm the device/browser you think you are using.
adb devices -l
```

Two process traps, both hit here:

- **Do not hide exit codes.** `cargo check ... 2>&1 | tail -40` reports
  `tail`'s status. Use `pipefail`, `set -o pipefail`, or check `${PIPESTATUS[0]}`.
- **A green gate is not a product test.** `check.bash` exits 0 while skipping
  optional `jj`, `bwrap`, Playwright, or `cargo-about`. Read what ran.

---

## 3. Desktop: debug the seam before the window

### What was actually used ✅

The desktop GUI was **not** run to completion on this branch. Desktop work was
verified at the crate/CLI layer, and the native Tauri path is blocked (see
below). Do not describe `cargo check` as “the desktop app works.”

The reliable desktop loop here is the CLI, because it exercises the same event
log, lease and view code the app uses:

```bash
cd apps/local-first
R=$(mktemp -d)
B=./target/debug/local-sync

$B --root "$R" --device laptop write   --session dbg --text first
$B --root "$R" --device laptop enqueue --session dbg --text queued
$B --root "$R" --device laptop view    --session dbg
```

Observed output (trimmed) confirms the important distinction:

```json
{"messages":[{"text":"first",...}],"queue":[{"text":"queued",...}], "read_only":false}
```

Use a **fresh temp root per scenario**. Reusing a root makes a previous test's
lease or queue look like the current bug.

### Inspect the log without a DB browser ✅

No `sqlite3` binary is required; Python's standard library can read the same
file:

```bash
python3 - <<'PY'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1] if len(sys.argv) > 1 else "events.sqlite")
for row in db.execute("select seq, kind, device_id, fence from events order by seq"):
    print(row)
print("journal:", db.execute("pragma journal_mode").fetchone())
PY
```

Observed: `journal_mode=wal`, kinds serialize as `"user_message"` and
`"message_queued"` (quoted JSON strings). If you compare kinds in SQL, expect the
quotes.

### The confirmed desktop compile failure ✅

Reproduce it with:

```bash
cargo check \
  --manifest-path apps/local-first/crates/tauri-plugin-infinity-android/Cargo.toml \
  --lib
```

Observed diagnostics (four `E0107`): `InfinityAndroid<R>` is supplied a generic
argument, but the desktop struct is non-generic.

- `src/desktop.rs` — `pub struct InfinityAndroid;` plus
  `init(...) -> Result<InfinityAndroid<R>>`
- `src/lib.rs` — `InfinityAndroidExt<R>` returns `&InfinityAndroid<R>`

Fix it in the consumer app by either gating the Android plugin out of the
desktop build, or making the desktop stand-in consistent with the runtime
parameter while keeping its honest `Unsupported` errors. See HANDOFF §6.

### Once the gate is cleared ⬜

The conventional Tauri desktop loop is `pnpm tauri dev` from
`apps/local-first/android`; it was **not** run on this branch. When you do:

1. Keep a terminal attached: Rust logs and WebView-process errors appear there.
2. Set `RUST_LOG` for the crates you care about, e.g.
   `RUST_LOG=infinity_agent_core=debug,infinity_sync=debug`.
3. Still write a JSON artifact from the runtime (as Android does) so a hang can
   be inspected without the window. A GUI that only shows a spinner is not
   debuggable headlessly.
4. Verify the model path against a **mock provider first**. A provider key and
   network failure look identical to a UI hang from the outside.

WebKit/GTK inspector tooling differs per OS and distribution and was not used
here; do not copy a flag from another platform without checking it on the target.

---

## 4. Android: artifacts first, CDP second

### The device contract ✅

The archetype package is `run.hydro.infinityhybrid`; its private data dir is
`/data/user/0/run.hydro.infinityhybrid`. Debug builds are signed and, importantly,
enable WebView remote debugging (wry compiles
`setWebContentsDebuggingEnabled` under `debug_assertions`).

Never assume a serial. `adb devices -l` first. A phone used earlier in this
work went offline; a replacement tablet was used only after explicit approval.
**Do not silently switch devices** — model turns, WhatsApp state and permissions
belong to one device.

### Golden headless loop ✅

```bash
S=<serial>
PKG=run.hydro.infinityhybrid
DIR=/data/user/0/$PKG

adb -s "$S" install -r \
  apps/local-first/android/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
adb -s "$S" shell pm grant "$PKG" android.permission.READ_CONTACTS
adb -s "$S" shell pm grant "$PKG" android.permission.POST_NOTIFICATIONS
adb -s "$S" shell am force-stop "$PKG"
adb -s "$S" shell am start -n "$PKG/.MainActivity"

# Probes write files; wait for the one you need instead of sleeping blindly.
adb -s "$S" shell run-as "$PKG" cat "$DIR/probe.json"
adb -s "$S" shell run-as "$PKG" cat "$DIR/native.json"
```

`apps/local-first/android/scripts/device-probe.sh <serial>` wraps install,
permission grants, launch and artifact printing. It removes only the files it
prints; it does **not** remove secret-bearing test configs.

### Artifact map ✅

| File | Produced by | Reads it to answer |
|---|---|---|
| `probe.json` | sandbox host probe | Did `HostCtx` + executor work? |
| `native.json` | Kotlin bridge + notification | Contacts granted? Shizuku present? Notification shown? |
| `chat.json` | optional model probe | Did the provider return, time out, or get denied? |
| `sandbox.json` | wasmi self-test | Benign=42? WASI denied? Spin out of fuel? |
| `sync.json` | optional relay probe | Push/pull counts, cursor position |
| `whatsapp.json` / `whatsapp-*.json` | opt-in WebView probes | Link stage, chat rows, read/send outcome |

Every artifact is written by the app, so a **missing file is itself evidence**:
the probe never reached that point. That is how the old WhatsApp read path was
found hanging when no conversation row existed.

### Logcat without drowning ✅

```bash
PID=$(adb -s "$S" shell pidof -s "$PKG")
adb -s "$S" logcat --pid="$PID"
```

Filter for the signatures that matter:

```bash
adb -s "$S" logcat --pid="$PID" | grep -E "AndroidRuntime|panicked|E/|infinity"
```

Common Android-native signatures and meaning:

- `FATAL EXCEPTION` + `AndroidRuntime` — Kotlin/JVM crash, look at the top frame.
- `panicked at ...` — Rust panic. The message appears in logcat; whether a
  symbolised backtrace is available depends on how the process was launched and
  how the build was configured, so verify that path rather than assuming it.
- A turn that never progresses, ending only at the probe's 45–60 s timeout —
  suspect a **TLS stall**, not a slow model. That exact symptom produced the
  portable-TLS fix (HANDOFF §5).
- `ENOENT` from the sandbox — the executor's cwd did not exist; it now creates it.

Do not paste unfiltered logcat into a bug report: it can contain message bodies,
phone numbers and tokens.

### Drive the WebView over CDP ✅

There is no need to tap a screen to inspect JS.

```bash
PID=$(adb -s "$S" shell pidof -s "$PKG")
# Discover the socket name. It is per-process, not fixed.
adb -s "$S" shell cat /proc/net/unix | grep webview_devtools
# Usually: @webview_devtools_remote_<pid>

adb -s "$S" forward tcp:9301 localabstract:webview_devtools_remote_$PID
curl -s http://127.0.0.1:9301/json
```

The target list matters. On this app it contained **both**
`http://tauri.localhost/` and `https://web.whatsapp.com/` when the hidden
WhatsApp view was alive. Always select by URL; grabbing the first target inspects
the wrong page.

Minimal Python CDP driver (used for the real send):

```python
import asyncio, json, urllib.request, websockets

async def main():
    targets = json.load(urllib.request.urlopen("http://127.0.0.1:9301/json"))
    page = next(t for t in targets if "web.whatsapp.com" in t["url"])
    async with websockets.connect(page["webSocketDebuggerUrl"], max_size=2**22) as ws:
        await ws.send(json.dumps({"id": 1, "method": "Runtime.enable"}))
        await ws.recv()
        await ws.send(json.dumps({
            "id": 2,
            "method": "Runtime.evaluate",
            "params": {
                "expression": "document.querySelector('#main header')?.innerText",
                "awaitPromise": True,
                "returnByValue": True,
            },
        }))
        while True:
            msg = json.loads(await ws.recv())
            if msg.get("id") == 2:
                print(msg["result"]["result"].get("value"))
                break

asyncio.run(main())
```

**Input needs to be trusted.** This was the single most misleading Android
lesson:

- `element.click()` and synthetic `MouseEvent`s do **not** open WhatsApp chat
  rows. Use CDP `Input.dispatchMouseEvent`, or `WebView.dispatchTouchEvent` with
  real `MotionEvent`s at `CSS px × displayMetrics.density`.
- Typing via `document.execCommand('insertText')` works only after focusing and
  selecting the editor. Without focus it silently left a single `"\n"`, which
  looked like a one-character draft.
- Prefer CDP `Input.insertText` when driving a page from outside; it targets the
  focused element and delivered the full multi-paragraph message in testing.
- After typing, **assert the character count and a preview**. Sending a partial
  message is worse than failing.

### Verify the notification/foreground service ✅

```bash
adb -s "$S" shell dumpsys notification --noredact | grep -A 2 "pkg=$PKG"
```

Observed during a turn: notification `id=4711`, channel `infinity-session`.
Short turns produced `claim → release`; an 8 s turn produced
`claim → renew → release`. A missing renewal in a **long** turn is a heartbeat
bug, not a lease bug — check the runtime loop, not the store.

### Screenshots and UI when there is no better instrument ✅

```bash
adb -s "$S" exec-out screencap -p > /tmp/shot.png
# Standard companion when you need element bounds; not required for the
# screenshots that verified the UI on this branch.
adb -s "$S" exec-out uiautomator dump /dev/tty
```

Screenshots contain real conversations and contact names. Review before sharing,
and never commit them.

### Pull the app database ✅

```bash
adb -s "$S" exec-out run-as "$PKG" cat sync/events.sqlite > /tmp/events.sqlite
python3 - <<'PY'
import sqlite3
db = sqlite3.connect("/tmp/events.sqlite")
for row in db.execute("select seq, kind, device_id, fence from events order by seq"):
    print(row)
PY
```

If the UI and a probe disagree, check the **path** first. A real bug on this
branch opened `sync/sync/events.sqlite` in the probe while the UI used
`sync/events.sqlite`. Same app, two databases, both “valid.”

---

## 5. Browser: one page, one profile, one CDP session

### Serve with the headers the storage APIs need ✅

```bash
cd apps/local-first/browser
python3 serve.py     # http://127.0.0.1:8123, COOP/COEP set
```

`serve.py` sets `allow_reuse_address`; without it, a killed server leaves
`TIME_WAIT` and the next run fails to bind. If the port is busy, find the actual
listener before assuming the script is broken.

### Launch a disposable Chrome ✅

```bash
# Use `chromium` instead of `google-chrome` if that is what is installed.
google-chrome \
  --headless=new \
  --no-sandbox \
  --disable-gpu \
  --disable-dev-shm-usage \
  --remote-debugging-port=9405 \
  --user-data-dir="$(mktemp -d /tmp/chrome-debug-XXXX)" \
  about:blank
```

- `--user-data-dir` is mandatory for reproducible storage tests; without a fresh
  profile, OPFS/IndexedDB from an earlier run masquerades as current success.
- `--no-sandbox` is needed in containers; it is not a production recommendation.
- The scripts use port **9405** for Chrome and **8123** for the static server.

### Open the page and inspect results ✅

```python
import asyncio, json, urllib.request, websockets

URL = "http://127.0.0.1:8123/app/"

async def main():
    req = urllib.request.Request(f"http://127.0.0.1:9405/json/new?{URL}", method="PUT")
    target = json.load(urllib.request.urlopen(req))
    async with websockets.connect(target["webSocketDebuggerUrl"], max_size=2**22) as ws:
        await ws.send(json.dumps({"id": 1, "method": "Runtime.enable"}))
        await ws.recv()
        await ws.send(json.dumps({
            "id": 2, "method": "Runtime.evaluate",
            "params": {
                "expression": "document.querySelector('#status')?.textContent",
                "awaitPromise": True, "returnByValue": True,
            },
        }))
        while True:
            msg = json.loads(await ws.recv())
            if msg.get("id") == 2:
                print(msg["result"]["result"].get("value"))
                break

asyncio.run(main())
```

### Probe hooks already present ✅

The demo pages publish their outcome on `window`, so no DOM scraping is needed:

| Page | Global |
|---|---|
| `browser/wasm-turn/index.html` | `window.__turnResult`, `__stateResult`, `__historyResult` |
| `browser/index.html` (relay experiment) | `window.__syncResult` |
| `browser/renderer/index.html` | `window.__commands`, `window.__render` |
| `browser/app/` | `#status` text plus rendered `data-testid` nodes |

Use them, then assert **values**. “The page did not throw” is not a test.

### Reload is the persistence test ✅

```python
await ws.send(json.dumps({"id": 3, "method": "Page.reload", "params": {"ignoreCache": True}}))
```

Then re-evaluate the status/queue count after the app reinitializes. Observed
success in this branch: `1 events in OPFS · 1 queued` before and after reload.
The same technique proved IndexedDB state/history counts
(`before=0 after=1` → reload → `before=1 after=2`).

For model context, a row count is **not** enough: inspect the second provider
request and confirm it actually carries the first turn. That test is required by
HANDOFF §4.1.

### Workers are a separate debugging surface 🔶

`/json` lists `type: "worker"` targets as well as pages. When OPFS fails:

1. Check the error came from the worker, not the page. `worker.onerror` is the
   only signal if you do not inspect the worker target.
2. Remember `createSyncAccessHandle()` exists only in a **dedicated worker** —
   not in the window, a shared worker, or a service worker.
3. Open the worker target and evaluate; a page-only console will look clean while
   the worker is dead.
4. Two stores opening the same IndexedDB at the same version can hang: only the
   first upgrade creates object stores. This branch uses separate databases
   (`infinity-agent-state`, `infinity-agent-history`) for exactly that reason.

SharedWorker coordination is proposed in HANDOFF §7.3 but **not implemented or
verified** here. Do not assume it behaves like a dedicated worker for OPFS.

### CORS is a network fact, not a code bug ✅

Providers fall into three buckets:

- Browser-callable (OpenRouter allowed it in testing).
- Not browser-callable (Kimchi returned `TypeError: Failed to fetch`; no CORS
  headers).
- Cloudflare-fronted endpoints that reject Python's default user agent with
  error `1010`.

`browser/dev-cors-proxy.py` is a **local test stand-in**: it injects the key
server-side and adds CORS headers. It is not a production component and must not
be deployed as an open proxy. Before blaming Rust, check the browser's network
failure and the provider's preflight behavior.

### Browser failure signatures ✅

| Symptom | Likely cause |
|---|---|
| `time not implemented on this platform` | `tokio::time` on wasm; use `SliceTimer`/`wasmtimer` or the cfg-gated retry delay |
| `RefCell already borrowed` in js-sys | reentrancy through a `spawn_local` bridge; this was fallout from the timer abort and disappeared with the fix |
| `createSyncAccessHandle is not a function` | OPFS used outside a dedicated worker |
| page waits forever on an IndexedDB open | two stores sharing one DB version |
| `TypeError: Failed to fetch` | provider CORS or offline; not a Rust panic |
| `1010` from Cloudflare | default Python user agent rejected; set a real UA (in the proxy) |
| blank page after reload | check the console for a failed import, then the origin/storage partition and the user-data-dir |

---

## 6. Local relay: debug the contract with curl first

✅ exercised locally. A public deployment existed earlier and was **deleted**;
local testing is the only supported mode until the auth/tenancy work in
HANDOFF §9 is done.

```bash
cd apps/local-first/worker
pnpm install --frozen-lockfile
pnpm exec wrangler d1 execute local-first-sync --local \
  --persist-to .wrangler/state --file schema.sql
printf 'RELAY_TOKEN=test-token\n' > .dev.vars     # git-ignored, never commit
pnpm exec wrangler dev --ip 127.0.0.1 --port 8790 --persist-to .wrangler/state
```

Then test each layer explicitly:

```bash
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8790/health
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://127.0.0.1:8790/v1/sync/push \
  -H 'content-type: application/json' -d '{"events":[]}'          # 401 or 503
curl -s -X POST http://127.0.0.1:8790/v1/sync/push \
  -H 'authorization: Bearer test-token' -H 'content-type: application/json' \
  -d '{"events":[]}'                                             # 200
```

Rules learned:

- No token configured → **503**, not an open relay.
- Wrong/missing token → **401**.
- A successful insert is decided by **row presence**, not `meta.changes`; the
  fence trigger inflates the change count.
- `tsconfig.json` must exist and include `src/`; without it `tsc --noEmit` can
  pass while checking nothing.
- Reset local state deliberately (`.wrangler/state`, D1 local data) when a test
  result looks impossible.

The public deployment from earlier work was deleted. Do not “fix” a client by
pointing it at a new URL; implement the auth/tenancy work in HANDOFF §9 first.

---

## 7. Compile-time signatures worth memorising ✅

These exact failures cost build cycles on this branch:

| Error | Cause | Fix |
|---|---|---|
| `This wasm target is unsupported by mio` | `sandbox-core` default `native` feature | `default-features = false` |
| `E0046 ... is_valid_executable` from `which` | native-only server module compiled for wasm | same feature switch |
| `The "wasm_js" backend requires the wasm_js feature for getrandom` | backend selected but feature missing | enable `getrandom/wasm_js` in the crate |
| `to use uuid on wasm32-unknown-unknown, specify ...` | uuid randomness source | enable uuid `js` on wasm |
| `E0053 ... incompatible type for trait` | native `Send` impl compiled for wasm | mirror the cfg-gated `async_trait` pair |
| `E0107 ... struct takes 0 generic arguments` | desktop plugin type mismatch | gate the plugin or make the stub generic |

When a Rust wasm error points into a dependency (`mio`, `which`, `socket2`), do
not patch the dependency. Find which **feature** of your crate pulled it in.

Other things that look like compile debugging but are not:

- **Stale APK.** A build can succeed while the device runs an older install.
  Reinstall, force-stop and relaunch; check the APK timestamp. A debug APK is
  large because it carries symbols and every ABI.
- **Missing desktop WebKit development libraries.** This branch compiled the
  `webkit2gtk` Rust wrapper, so it did not hit a missing system library. If you
  do, install the distribution's WebKitGTK/GTK dev packages; do not assume the
  exact package names carry over between distributions.

---

## 8. Secrets, personal data and device hygiene

The debugging loop must not leak what it inspects.

- `apps/.env`, `worker/.dev.vars`, `chat-config.json`, `sync-config.json` and
  WhatsApp pairing files are **runtime-only**. Never commit, print, screenshot,
  bundle, or include them in documentation.
- Load shell secrets without echoing them:

  ```bash
  set -a; . apps/.env; set +a
  ```

- Push device-only config with a temporary file, then remove both copies:

  ```bash
  DIR=/data/user/0/run.hydro.infinityhybrid
  adb -s "$S" push /tmp/chat-config.json /data/local/tmp/chat-config.json
  adb -s "$S" shell run-as "$PKG" cp /data/local/tmp/chat-config.json "$DIR/chat-config.json"
  adb -s "$S" shell rm -f /data/local/tmp/chat-config.json
  ```

- Delete configs after the test; a leftover key is a live credential.
- Redact phone numbers and chat bodies from logs. `bodySnippet` can contain the
  user's own number even when the command itself did not print it.
- Browser API keys belong in memory, injected per session (or held by a
  server-side proxy). Do not put them in URLs, `localStorage`, OPFS, or logs.
- Screenshots and pulled databases are personal data. Keep them in temp paths and
  delete them when the investigation ends.

---

## 9. One-command end-to-end loop

✅ `E2E PASS` was produced by this script on this branch.

```bash
cd apps/local-first
# Requires a built local-sync CLI, Chrome/Chromium, Python websockets,
# browser/node_modules, and a running or startable static server.
python3 scripts/e2e.py

# Optional Android leg, only with an approved serial:
ADB_DEVICE=<approved-serial> python3 scripts/e2e.py
```

It checks:

- desktop queue/send/truncate/restore via the CLI,
- browser queue persistence across a reload,
- Android probe availability and sandbox output.

Its scope is intentionally narrow: three **independent local logs**, not one
replicated conversation, and the Android leg does not drive the UI or a model
turn. When it passes but the product fails, the gap is in the integration, not
in this script.

---

## 10. What is still unverified

Do not let a successful command imply these are solved:

- Desktop GUI window, desktop model turn and desktop packaging.
- Browser SharedWorker/multi-tab authority.
- Stable-session model resume on native and browser (HANDOFF §4.1).
- Real relay deployment, per-user tenancy, authenticated browser CORS.
- Web Push or any closed-tab wake path.
- Shizuku execution (the app was not installed).
- WhatsApp plugin send path after the fixes; only a CDP-driven user-approved
  send is recorded.
- Windows/macOS desktop behavior.

If you add evidence for any of these, record **how it was observed** in
`decisions.md`, not just that it passed. The next agent needs the instrument, not
the reassurance.

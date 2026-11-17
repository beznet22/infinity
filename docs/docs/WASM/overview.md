# Infinity Hybrid: WASM + Android + Cloudflare Blueprint

> **Status legend.** **[today]** = exists in this repository now (evidence: `file:line`). **[plan]** = target architecture of the `feat/wasm-android-hybrid` fork — designed, not yet built. **[idea]** = proposed design (§10) — brainstormed, awaiting a decision before it enters the build plan. Every claim in this document was fact-checked against the codebase and primary sources on 2026-11-17; the full claim-by-claim audit with evidence is in [`audit.md`](./audit.md), and the reasoning behind each correction in [`decisions.md`](./decisions.md).

This is the project blueprint for forking [Infinity](https://github.com/hydro-project/infinity) (this repo) into a hybrid runtime that runs agents **fully on-device on Android (Tauri v2)** and **fully in the browser (`infinity-wasm`)** — including long-running tasks, with **no cloud dependency** (§2.5) — all from one Rust core. Cloudflare Workers / Agents is an **optional accelerator** for heavy-duty, very-long-running work — never a requirement. Sections 2, 3.1–3.6 and 6 describe what the upstream core actually is; sections 1, 3–5, 7–10 define what we build on top of it — starting from what the compiler says stands in the way (§9), then what the existing patterns make cheap (§10).

---

## 1. Repository & Git Worktree Strategy **[plan]**

**Goal.** The fork will build simultaneously for **Android NDK (`aarch64-linux-android`)**, **WebAssembly (`wasm32-unknown-unknown`)**, and **host tests** — three target triples that do not share build state. (Today the workspace has none of the cross-target configs; that is exactly what this plan adds.)

| Approach | How it works | Verdict for this stack |
|---|---|---|
| Single branch checkout | One working directory and one `target/` shared between upstream `main` and feature work. Switching branches to pull upstream invalidates Cargo's incremental state and shared build caches, and three-target builds amplify the rebuild cost. | **Avoid.** |
| Git worktree + feature branch | `git worktree add` creates a separate directory linked to the same `.git`, with its own working tree and independent `target/`. Keep `main` clean in the root for upstream syncs; build the multi-target fork in `../infinity-hybrid`. | **Recommended.** |

```bash
# Keep the root checkout on clean upstream `main`
git remote add upstream https://github.com/hydro-project/infinity.git

# Isolated worktree + branch for the cross-platform fork
git worktree add ../infinity-hybrid -b feat/wasm-android-hybrid
```

Sync cadence: `git -C ../infinity-hybrid fetch upstream && git merge upstream/main` — the worktree's `target/` stays untouched by root-checkout rebuilds.

---

## 2. System Overview & Core Pillars

The upstream Infinity core **[today]** is a Rust framework for massively concurrent agentic systems: agents yield while waiting on tools and "between turns, an agent is pure data, just its conversation history with no task, no stack, and no open connection, so a waiting agent hibernates for free and wakes on the next message" (`README.md`). The same code runs in a resident process and on serverless platforms (AWS Lambda), where scale-to-zero is the default. The fork's pillars:

### 2.1 Bounded-slice execution & hibernation **[today]** *(corrected from "stackless coroutine execution")*
Infinity executes agent workflows in short, bounded **slices**: a step-mode runtime runs a single slice, persists the thread state, and drops everything else (`crates/infinity-agent-core/src/system/builder.rs:399-424` — the step API exists precisely "to fit serverless platforms where nothing survives between slices"). There are no coroutines in the language sense: a waiting agent holds no task, no stack, and no open connection — that is what makes hibernation free and what the fork relies on in browsers and on the edge. *Design intent (unmeasured):* this should let many background agents coexist in one Android app or browser tab without OS memory kills or UI jank.

### 2.2 Reactive Agent Protocol (RAP) **[today]**
Instead of holding fragile open sockets for long-running tasks, RAP dispatches connectionless HTTP POST **invocations** carrying `callback_url` and `group_id`, and delivers results by POSTing typed callbacks back — `tool_result`, `user_choice`, `subscription_event`, `view_update` (`crates/rap-protocol/src/lib.rs:51-52, 241-243, 289-357`; the receiving runtime feeds them into the thread, which wakes on the next event). Existing MCP servers run unchanged through a compatibility layer, and RAP is an open spec ([spec overview](https://infinity.hydro.run/docs/rap/spec/overview)).

### 2.3 Dual-target compilation **[plan]** *(storage & TLS corrected)*
The fork compiles the shared crates natively into Tauri's Android `.so` library and into a `wasm32-unknown-unknown` WebAssembly module (`infinity-wasm`). Two facts to design around, verified today:
- **TLS:** the workspace HTTP client is `reqwest` 0.13, whose `default-tls` feature **is rustls** (`default-tls = ["rustls"]`, aws-lc-rs provider + `rustls-platform-verifier`) — there is **no OpenSSL** dependency. The Android implication is a C cross-toolchain for `aws-lc-sys` (NDK clang) plus `jni` for the platform certificate store, not a TLS switch *(corrected 2026-11-17; D11)*.
- **Persistence:** there is **no SQLite in this codebase**. The daemon persists per-thread JSON files (`crates/infinity-daemon/src/memory_store.rs:3-16`); the Lambda runtime uses Aurora DSQL (Postgres wire protocol) + DynamoDB (`crates/infinity-agent-lambda/src/conversation_history.rs:16`, `state_store.rs:9`). SQLite appears only at the Cloudflare edge (Durable Objects / Agents SDK local SQL). Android/browser persistence will be new `ConversationStore`/`StateStore` implementations (`crates/infinity-agent-core/src/traits.rs:27, 223`).

### 2.4 Swappable local vs. cloud adapters **[today = trait seams] / [plan = full adapter set]**
The core already exposes clean trait seams, and OS-dependent code lives in separate crates:
- **Stores:** `ConversationStore`, `StateStore`, `InputSender`, `DeferQueue`, `ModelSource`, `ThreadObserver` (`crates/infinity-agent-core/src/traits.rs:27, 223, 311`; `system/defer.rs:27`; `system/model.rs:32`; `system/observer.rs:30`) with in-memory, JSON-file (daemon), and DSQL/Dynamo (Lambda) implementations.
- **MCP:** `McpTransport` — one JSON-RPC `request(method, params)` method per initialized session (`crates/infinity-mcp-bridge/src/lib.rs:52-59`), currently **stdio only** (`StdioMcpTransport` spawns child processes, `lib.rs:66-100`).
- **Sandbox:** `ModeProvider`, `SandboxBackend`, `MetadataStore` (`crates/sandbox-core/src/sandbox.rs:57, 155`, `metadata.rs:10`) routing between local (`sandbox-local`) and remote (`sandbox-remote`, DynamoDB metadata, S3/clone-based backends).
- **Serverless slices:** `infinity-agent-lambda` runs one slice per invocation with `WakeupScheduler`/`SleepTool` timers (`crates/infinity-agent-lambda/src/tools/sleep.rs:46-115`) and a RAP receiver binary.

**[plan]** The fork adds browser and edge adapters behind new traits with these same roles (§4), so workloads can run 100% locally on Android or in-browser, or route heavy tasks to Cloudflare Workers / Containers.

### 2.5 Cloud-optional by design (verified) **[plan, on verified capabilities]**

Full-featured operation — **including long-running tasks — works with zero cloud dependency**; the edge (§4 remote column) is an accelerator, not a backbone. Verified basis:

| Capability | Android (Tauri v2) | Browser | Evidence |
|---|---|---|---|
| Long-running execution | Foreground service keeps the runtime resident — `startForegroundService` + `START_STICKY` (OS auto-restart), Tauri v2 background-service plugin pattern | SharedWorker runs slices while the tab is open; WASM compute unthrottled while visible | [tauri-plugin-background-service](https://docs.rs/tauri-plugin-background-service/); [Chrome M87+ throttling](https://blog.chromium.org/2020/11/tab-throttling-and-more-performance.html) — hidden-page wake-ups degrade to ~1/min: *delayed* slices, never broken ones, under the slice model |
| Surviving app/tab death | `START_STICKY` restart; threads persisted via the store traits | Hibernate → resume on reopen (native model; state in IndexedDB) | Step/slice design (`builder.rs:399-424`); store traits (`traits.rs:27,223`) |
| LLM calls | Direct device→provider over rustls reqwest | Direct browser→provider — CORS verified for Anthropic (`anthropic-dangerous-direct-browser-access: true`) and OpenAI (`ACAO: *`, streaming works) | [Anthropic CORS](https://simonwillison.net/2024/Aug/23/anthropic-dangerous-direct-browser-access/) (2024-08; empirically re-verified 2026-07). Caveats: Anthropic org settings can disable it; browser keys are a BYOK concern |
| RAP callbacks / events while active | The app hosts the callback endpoint locally (loopback HTTP) | In-page eventing (BroadcastChannel/WS to the SharedWorker) | `rap-client/src/callback_server.rs:16-18` (binds `127.0.0.1`); RAP callback URLs are host-agnostic (spec) |
| Events while closed (subscriptions, webhooks) | Foreground service receives them | **Needs an always-on endpoint of the user's own** — e.g. their own `infinity-daemon` on any machine they control (**not** a cloud worker) — or events defer to next open | RAP callback URLs accept any reachable host; `infinity-daemon` is a resident runtime **[today]** |
| Heavy tool execution | Native processes via app sandbox / Shizuku (§3) | wasmsh + OPFS sandbox (§5) — WASM-scale only | §3 table; §5 verified |

**What genuinely benefits from the optional edge (§4):** Linux-container-grade tooling (real toolchains, `npm test`, preview servers), tasks that must outlive device constraints (e.g. Android 15 foreground-service timeout policies), and event reception while a browser is closed when the user runs no always-on endpoint of their own. None of these is required for the core promise: **full-featured local operation.**

---

## 3. End-to-End Architecture on Android (Tauri v2) **[plan]**

> None of the Android surface exists in this repo — and most of it is **consumer-side**: the fork ships the Rust core and reusable crates; a consuming Tauri v2 app (the `apps/local-first/android` archetype, §6) supplies the Kotlin plugin layer. What is verified is every platform mechanism below: Tauri v2 mobile (`tauri android dev`), Kotlin plugins with `@Command` methods ([mobile plugin development](https://v2.tauri.app/develop/plugins/develop-mobile/)), Android webview debugging via `chrome://inspect` ([Tauri develop guide](https://v2.tauri.app/develop/)), and the Android framework APIs named.

| Capability | Android / Kotlin layer | Rust / Infinity bridge |
|---|---|---|
| Cloud LLMs | Standard `INTERNET` permission | Native `reqwest` client — already rustls via `default-tls`, no OpenSSL; Android builds need the NDK C toolchain for `aws-lc-sys`. Providers ride the existing `infinity-provider-rig` (OpenAI/Anthropic/Gemini via rig-core) and `infinity-provider-bedrock` seams |
| Notifications & Contacts | Android `NotificationListenerService` & `ContactsContract` | Tauri v2 Kotlin plugins (`app.tauri.plugin.Plugin` subclasses, `@Command` methods invoked from Rust via `run_mobile_plugin`); events pushed back through RAP `subscription_event` callbacks |
| Files & App storage | Scoped Storage / MediaStore; Shizuku file bridge where privileged | `tokio::fs` inside the app sandbox; Kotlin plugin calls for shared external storage |
| WhatsApp control | Overlay `WebView` with a desktop user agent (validated on device) | The client boots — QR canvas, service worker, IndexedDB and WebCrypto all present — but **only as a view**: pairing needs a human scan, and WhatsApp names unofficial clients and automation as ban triggers, so the fork ships no DOM injection or message automation *(validated 2026-11-17; D24)* |
| Shell, sandbox & app control | Wasm modules shipped as assets (no `exec` permission); Shizuku Binder IPC only for privileged escalation | Tools run in-process through `SandboxExecutor` by interpreting `.wasm` from app data (§3.1); Shizuku remains the optional path for privileged shell (`adb`/root via `app_process`), streaming stdout/stderr back. *Implementation-time checks:* `newProcess` / uiautomator / `am` surface in [Shizuku-API](https://github.com/RikkaApps/Shizuku-API); ADB permission limits and Android 9 hidden-API limits apply |
| MCP & async tools (RAP) | Android network stack or local loopback HTTP server | Native RAP (connectionless HTTP POST + webhook callbacks) for hibernating async tasks **[today]**; MCP over HTTP/SSE for remote servers **[plan]** — the existing `McpTransport` is stdio-only, which cannot work in a browser or against remote servers |

**Android runtime lifecycle** *(all steps are fork work; mechanics consistent with the verified slice/hibernation + RAP design)*:

1. **Trigger.** A user enters a prompt in the Tauri UI, or an Android event (incoming notification, WhatsApp Web message) arrives as a RAP callback.
2. **Wake & plan.** Infinity loads the thread from the thread store (JSON-file / DSQL today; Android store is a fork choice — see §2.3) and streams context to the provider over rustls.
3. **Dispatch & hibernate.** When the model calls a tool (Shizuku UI tap, contacts query, WhatsApp reply), Infinity dispatches the call to the Tauri Kotlin bridge or a RAP endpoint and hibernates the in-memory task.
4. **Resume & complete.** An Android Foreground Service keeps the process alive until the Kotlin plugin or RAP server posts the result back, waking the thread to finish streaming to the UI.

### 3.1 Running real tools on device: two options, one measured cost **[plan]**

The core needs to run commands (grep, git, python, formatters). On Android the obvious approach — ship a binary and execute it — is blocked by SELinux: since Android 10, an app targeting API 29+ cannot execute files in its writable data directory (`app_data_file:file execute` is denied). Two paths remain.

**A. Execute from the native library directory.** Rename the executable to `lib<name>.so`, put it in `jniLibs/<abi>/`, and set `android:extractNativeLibs="true"` (AGP ≥ 3.6 defaults to *not* extracting). The installer then places it in `/data/app/…/lib/<abi>/`, labelled `apk_data_file`, which the app's own domain may execute; the app runs it from `context.getApplicationInfo().nativeLibraryDir`. It accepts any ELF — including a full Python or Node build — but each binary ships separately, needs `patchelf --set-rpath '$ORIGIN'` plus its shared libraries, and multiplies per ABI. Treat it as a fragile platform detail, not a stable feature.

**B. Embed a Wasm engine and run `.wasm` from app data.** No `exec` permission is involved: the host reads the module bytes and interprets them — which is what a sandbox should be anyway. Measured here (aarch64-linux-android, release, `llvm-strip`) on an otherwise empty `cdylib`:

| Build | Stripped size | Delta |
|---|---|---|
| Baseline cdylib | 308 KB | — |
| + `wasmi` 1.x (`Engine` + `Module::new`) | 1.44 MB | **+1.16 MB** |

`wasmi` is an interpreter: no JIT, no executable memory, deterministic, with fuel metering — the same metering the slice model wants. A JIT (`wasmtime`) is much larger (the official minimal C API build is 2.1 MB on x86_64 Linux, and the Rust crate is bigger) and its JIT pages were not validated on this device, so the interpreter is the default recommendation. Toolchains that must run unmodified ELF binaries keep option A as a fallback.

**Decision:** option B is the default for on-device tools; option A is the escape hatch for binaries that cannot be compiled to Wasm (D23).

---

## 4. Unified Trait Architecture: Browser Local (`infinity-wasm`) vs. Cloud Offload **[plan, on verified foundations]**

The fork defines four adapter traits (roles preserved from the original blueprint; names marked *planned* since they don't exist yet) so the browser runtime can run 100% locally with zero backend, or selectively route heavy work to Cloudflare or the Android node.

| Subsystem | Core trait interface | Browser local adapter (`infinity-wasm`) | Swappable remote adapter (Cloudflare / Android) |
|---|---|---|---|
| Sandbox | `SandboxExecutor` *(planned; role grounded on the real `ModeProvider`/`SandboxBackend`/`MetadataStore` seams, `crates/sandbox-core`)* | **WASM bash/Python runtime + OPFS.** Sandboxed bash, JS, Python inside a Web Worker; OPFS (Origin Private File System) is Baseline, available in Workers, with sync access handles worker-side ([MDN](https://developer.mozilla.org/en-US/docs/Web/API/File_System_API/Origin_private_file_system)) | **Cloudflare Sandbox / Containers.** Real Linux containers: [Containers](https://developers.cloudflare.com/containers/) (public beta) and the [Sandbox SDK](https://developers.cloudflare.com/sandbox/) — "a sandbox is a Durable Object and the Container it starts"; code/commands arrive over HTTPS/RAP |
| MCP | `McpTransport` **[today]** (`request(method, params)`, `crates/infinity-mcp-bridge/src/lib.rs:52`) | **MessagePort + HTTP/SSE MCP.** Spawn WASM-compiled MCP servers in dedicated Web Workers (piping JSON-RPC over MessagePorts the way `StdioMcpTransport` pipes over stdio) or connect directly to remote HTTP/SSE MCP servers | **Cloudflare MCP relay.** Proxy native stdio CLI/Node/Python MCP servers over HTTPS/SSE; the [Agents SDK](https://developers.cloudflare.com/agents/) ships `MCPClientManager` for MCP connectivity |
| Coordinator | `AgentCoordinator` *(planned; role grounded on the daemon's session management + `PersistentConversationStore`/`PersistentStateStore`, `crates/infinity-daemon/src/session/mod.rs:11-17`)* | **SharedWorker + IndexedDB.** One background coordinator shared by all open tabs; threads persisted in IndexedDB; cross-tab messaging via `BroadcastChannel` (all standard, widely-supported APIs) | **Cloudflare Durable Objects.** The coordinator stays *reachable* 24/7 even after the browser closes: DOs are always-addressable, wake-on-event instances — clients/connections persist while the object sleeps, and it re-initializes automatically on the next event at near-zero idle cost ([hibernation docs](https://developers.cloudflare.com/durable-objects/best-practices/websockets/)) |
| Slice runner | `SliceRunner` *(planned; role grounded on the real serverless slice machinery in `crates/infinity-agent-lambda`: one slice per invocation, `WakeupScheduler` timers, RAP receiver)* | **Service Worker wake path.** Baseline: Web Push + server-initiated RAP callbacks; Web Locks for cross-tab exclusivity. Background Sync kept as a *Chromium-only, non-standard* enhancement (negative Firefox standards position — do not build on it as the primary path) | **Cloudflare Workers / Agents SDK.** Serverless slices at the edge with built-in SQLite storage, hibernation, alarms, and scheduling ([Agents SDK](https://developers.cloudflare.com/agents/)) |

---

## 5. Open-Source Browser Sandbox & Shell Repositories to Reference

All five repositories verified 2026-11-17 (evidence in `audit.md` §5). Original ordering preserved; descriptions corrected where the original overstated.

| Repository | Language / Stack | What it provides (verified) | Best use in `infinity-wasm` |
|---|---|---|---|
| [mayflower/wasmsh](https://github.com/mayflower/wasmsh) | Rust → WASM + Pyodide | Clean-room Bash-compatible shell with **88 utilities** (grep, sed, awk, jq, tar, curl, …), **Python 3.13** + pip/micropip, **capability-based VFS**, **step budgets** (bounded step counts; capped V8 heap in pool mode), network allowlist broker; standalone `wasm32-unknown-unknown` (browser Worker) and Pyodide (`wasm32-unknown-emscripten`) targets; Apache-2.0 | **Primary candidate** — drop-in sandbox executor for Rust-native shell + Python agent execution in a browser Worker. Note: built on Pyodide 0.28/0.29-era (Python 3.13); current Pyodide is 314.0.x (Python 3.14) — check at integration time |
| [sd2k/conch](https://github.com/sd2k/conch) | Rust (brush-core) → WASM + TS | Full bash compatibility via **brush** (bash-compatible shell in Rust) sandboxed with **wasmtime**; hybrid VFS (in-memory VFS + cap-std-secured real mounts); **`@bsull/conch`** JS/TS bindings for browsers and Node 19+; curated utility set (cat, grep, jq, sort, … — *not* uutils coreutils, contrary to earlier drafts); MCP server; Rust/Go/JS APIs; Apache-2.0 | Heavyweight bash compatibility when agents run complex pipelines in-browser |
| [Parassharmaa/agent-sandbox](https://github.com/Parassharmaa/agent-sandbox) | Rust + WASI (Boa JS) + TS/NAPI | Embeddable AI-agent sandbox: **80+ CLI tools**, full sh/bash interpreter, **Boa** JS runtime (ES2023+), **filesystem change tracking** (`diff()` snapshots), **SSRF-safe HTTP** (agent-fetch: allowlists, DNS-rebinding prevention, rate limiting; networking off by default), resource limits (fuel/timeout/memory), <13 ms cold start; MIT | Reference design for agent tool schemas and filesystem diff/change-tracking hooks |
| [hari-mohan-choudhary/web-container](https://github.com/hari-mohan-choudhary/web-container) | TypeScript + Rust WASM (QuickJS + esbuild-wasm) | Browser code container: esbuild-wasm bundling in a Worker, QuickJS (`quickjs-emscripten`) sandboxed execution, Rust→WASM runtime for process management/module resolution, **OPFS + IndexedDB** persistence surviving reloads (needs COOP/COEP headers). ⚠ **Reference-only: 1 commit, 0 stars — unmaintained hobby demo** | *Reading* reference for the OPFS persistence layer and JS/TS bundling pipeline inside a Worker |
| [jdrouet/browser-fs](https://github.com/jdrouet/browser-fs) | Rust → WASM | Async filesystem crate mirroring `std::fs` / `async-fs` on top of the browser **File System Access API** (⚠ *not* OPFS); `#![forbid(unsafe_code)]`; **must run in a Web Worker**; 4 GB file cap; MIT | Candidate FS adapter bridging Rust fs traits to browser storage inside `infinity-wasm` |

---

## 6. Fork Workspace Layout **[plan, reconciled with upstream]**

```text
infinity-hybrid/                        # git worktree (branch: feat/wasm-android-hybrid)
├── crates/                             # ── upstream members [today] ──
│   ├── infinity-protocol/              # pure serde types (compiles everywhere)
│   ├── rap-protocol/                   # RAP types: invocations, callbacks (compiles everywhere)
│   ├── infinity-agent-core/            # state machine + store/config/model traits, slice pipeline
│   ├── infinity-mcp-bridge/            # McpTransport (stdio today) + tool bridging
│   ├── infinity-daemon/                # resident runtime: session mgmt, JSON-file stores
│   ├── infinity-agent-lambda/          # serverless slices: DSQL/Dynamo stores, WakeupScheduler
│   ├── sandbox-core/                   # ModeProvider / SandboxBackend / MetadataStore seams
│   ├── sandbox-local/                  # native OS sandbox
│   ├── sandbox-remote/                 # remote sandbox backend
│   ├── infinity-provider-{protocol,rig,bedrock}/   # model providers
│   ├── rap-client/ rap-steering-server/ rap-github-event-poller/ rap-test-servers/
│   ├── infinity-rap-bridge/ infinity-agent-cli/ infinity-mdtests/ infinity-slack-{bot,dataflow}/
│   │                                   # ── fork additions [plan] ──
│   ├── infinity-wasm/                  # browser adapters: wasmsh/OPFS sandbox, Worker MCP, SharedWorker daemon
│   └── infinity-remote-adapters/       # HTTP/RAP adapters for Cloudflare Workers/Agents & Android Tauri peers
└── apps/                               # consumer archetypes — built in CONSUMER codebases, not fork members [consumer-side]
    ├── android-tauri/                  # example Tauri v2 app + Kotlin plugins (Shizuku shell, notifications, WebViewClient)
    └── cloudflare-worker/              # example edge SliceRunner & MCP/container offload worker over infinity-remote-adapters
```

**Scope note:** the fork ships crates; everything under `apps/` is a **consumer archetype** — an integration blueprint for any codebase that consumes the fork, deliberately not a workspace member of it (D16).

---

## 7. Build Order

When ready to start coding in `infinity-hybrid`, the modules in dependency order:

0. **WASM readiness (Phase 0)** — clear the compiler-verified blockers of §9 so `cargo check --target wasm32-unknown-unknown` passes for the core set (`infinity-provider-protocol` is already green; then the protocol crates, `infinity-agent-core`, wasm transports in `infinity-mcp-bridge`, sandbox traits). Everything else builds on this.
1. **Core traits** (`crates/infinity-agent-core`) — define `SandboxExecutor`, `AgentCoordinator`, `SliceRunner` with `wasm32` feature gates; keep `McpTransport` as the transport contract (extend with an HTTP/SSE impl later).
2. **Consumer archetype: Android Tauri app** (`apps/local-first/android`, consumer-side) — a reference consuming app scaffolding the Kotlin bridges (Shizuku shell, `NotificationListenerService`, WhatsApp `WebViewClient`) over the fork's crates; doubles as the integration test for the crate APIs.
3. **Browser runtime** (`crates/infinity-wasm`) — wire the sandbox (§5 candidates), OPFS, and IndexedDB into `wasm-bindgen` entrypoints; develop with the tooling in §8.
4. **Consumer archetype: Cloudflare worker** (`apps/cloudflare-worker`, consumer-side) — a reference edge slice runner and RAP callback router (Durable Objects + Agents SDK) built on `crates/infinity-remote-adapters`; doubles as the integration test for the edge crates.

---

## 8. WASM Development & Debugging Tooling

The general toolchain for building, inspecting, testing, and debugging `infinity-wasm` — all verified current on 2026-11-17 (evidence: `audit.md` Appendix C + research note §4) — plus the agent-integrated debugging layer that lets the coding agent drive the whole loop.

### 8.1 Build
| Tool | Install | Role |
|---|---|---|
| `wasm32-unknown-unknown` target | `rustup target add wasm32-unknown-unknown` | The compile target for all browser crates |
| [wasm-pack](https://crates.io/crates/wasm-pack) **0.15.1** | `cargo install wasm-pack` | One-stop build/test/package for Rust→WASM; now maintained under the wasm-bindgen org (0.14–0.15.1 released Jan–Oct 2026). The old rustwasm working group was sunset in 2025 — the tool is alive, its wiki/book links moved to [wasm-bindgen.github.io/wasm-pack](https://wasm-bindgen.github.io/wasm-pack/) |
| [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen) | workspace dep + matching `wasm-bindgen-cli` | JS interop boundary; emits the JS glue + `.wasm` consumed by the Worker/trunk |

### 8.2 Dev loop
| Tool | Install | Role |
|---|---|---|
| [trunk](https://trunk-rs.github.io/trunk/) | `cargo install trunk --locked` | App-style iteration: `trunk serve` builds (`wasm32` → `wasm-bindgen` → bundle), watches, hot-reloads the browser |
| `wasm-pack build --dev` / `--target web`/`--target bundler` | — | Library-style crates consumed by the Tauri webview or a bundler |

### 8.3 Inspect & profile
| Tool | Install | Role |
|---|---|---|
| [wabt](https://github.com/WebAssembly/wabt) | `npm i -g wabt` / `brew install wabt` / `apt install wabt` | `wasm2wat` (disassemble), `wat2wasm`, `wasm-objdump`, `wasm-validate` — see what the compiler actually emitted |
| [Twiggy](https://crates.io/crates/twiggy) **0.8.0** | `cargo install twiggy` | Code-size profiler (`twiggy top`) — find what's bloating the binary. ⚠ Upstream repo archived; crate works, but plan an exit to `wasm-tools`-based analysis if it stalls |

### 8.4 Debug
| Tool | Install | Role |
|---|---|---|
| [console_error_panic_hook](https://github.com/rustwasm/console_error_panic_hook) | cargo dep; `console_error_panic_hook::set_once()` | Rust panic messages (with stack) in the DevTools console instead of `RuntimeError: Unreachable executed` |
| **DWARF debugging in Chrome** (Chrome 114+) | Build with debug info — `wasm-pack` profile `dwarf-debug-info = true` / `rustc -g`; install the **"C/C++ DevTools Support (DWARF)"** Chrome extension ([official guide](https://developer.chrome.com/docs/devtools/wasm)) | Source-level Rust debugging in DevTools: breakpoints in `.rs`, variable inspection, call stacks with Rust names |

### 8.5 Test
| Tool | Install | Role |
|---|---|---|
| [wasm-bindgen-test](https://rustwasm.github.io/wasm-bindgen/) | ships with wasm-bindgen | In-browser tests: `wasm-pack test --headless chrome` (also firefox/geckodriver) |

### 8.6 Agent-integrated debugging: chrome-devtools-mcp
[chrome-devtools-mcp](https://github.com/ChromeDevTools/chrome-devtools-mcp) (official ChromeDevTools project, Apache-2.0, very active) exposes a live Chrome to the coding agent as an MCP server — performance traces, network inspection, screenshots, and console messages **with source-mapped stack traces**, driven through puppeteer. This closes the loop: the agent builds `infinity-wasm`, serves it, opens the page, reads the Rust panic traces, inspects the RAP/MCP network calls, records a performance trace, and iterates without a human at the keyboard.

**Install (coding agent config):**
```json
{
  "mcpServers": {
    "chrome-devtools": {
      "command": "npx",
      "args": ["-y", "chrome-devtools-mcp@latest"]
    }
  }
}
```
Requirements: Node LTS, current Chrome, npm. Useful flags: `--headless`, `--isolated`, `--slim` (reduced toolset). It ships skills (page debugging, CLI, memory-leak debugging, …) and one-command plugin installs for Claude Code, Copilot/VS Code, and Gemini CLI; the repo also carries a Kimi Code plugin directory.

**WASM debug loop with the agent:**
1. `trunk serve` (or `wasm-pack build --dev` + a static server) with `dwarf-debug-info = true`.
2. Agent calls `navigate_page` → `list_console_messages` — panics arrive source-mapped thanks to the panic hook + DWARF.
3. On failure: agent reads the mapped Rust stack, fixes code, rebuilds; DevTools breakpoints in `.rs` sources confirm behavior.
4. For performance: agent records a trace (`performance_start_trace`), checks long tasks, and iterates.
5. For Android later: the same server supports debugging Chrome on a device ([advanced usage](https://github.com/ChromeDevTools/chrome-devtools-mcp/blob/main/docs/advanced-usage.md)), and Tauri's Android webview is inspectable via `chrome://inspect`.

---

## 9. WASM Build Readiness — Compiler-Verified Gap Analysis **[today → plan]**

What actually stands between this workspace and a `wasm32-unknown-unknown` build — established by running `cargo check --target wasm32-unknown-unknown -p <crate>` per crate (2026-11-17; verbatim evidence and platform sources in `audit.md` Appendix C).

### 9.1 Where the workspace stands today

*Status after chunks 0.1–0.7 (2026-11-17): **7 green / 0 red — Phase 0 complete**. The gate below is enforced by `scripts/check-wasm.bash` (wired into `check.bash`); it now has an empty expected-red set, so any regression fails the suite.*

| Crate | `wasm32` check | Root cause |
|---|---|---|
| `infinity-provider-protocol` | ✅ **compiles clean** | heavy deps already behind optional `mock`/`remote` features — the gating pattern to replicate |
| `rap-protocol` | ✅ *(chunk 0.2)* | was ❌: reqwest's wasm fetch futures are **`!Send`** vs `Send`-bounded client traits → fixed with cfg-gated traits (`Send` native / `?Send` wasm) |
| `infinity-protocol` | ✅ *(chunk 0.2)* | was ❌ transitively via `rap-protocol`. `dirs` **does** compile for wasm; `home_dir()` just resolves to nothing at runtime, so path injection (W9) is a correctness item, not a blocker |
| `rap-client` | ✅ *(chunk 0.3)* | was ❌: mio rejected the target — tokio `net` + the hyper callback server. Both moved behind `cfg(not(target_arch = "wasm32"))`; the module is gated too |
| `infinity-agent-core` | ✅ *(chunk 0.5)* | was ❌: 15 `Send`-bound errors — `Tool<M>: Send + Sync` could not hold a `?Send` HTTP client. Fixed by cfg-gating the trait (native `Send + Sync` / wasm `?Send`) plus 11 impl sites. `tokio::time`/`std::fs` compile and fail only at runtime (D18 defers their replacements to Phase 3) |
| `sandbox-core` | ✅ *(chunk 0.6)* | was ❌: mio via tokio `process`/`fs` + axum, plus process plumbing. Fixed with a default-on `native` feature gating `server`/`git`/`jj`, `SpawnedCommand`/`spawn_command` and 8 native-only deps; browser builds use `--no-default-features` |
| `infinity-mcp-bridge` | ✅ *(chunk 0.7)* | was ❌: mio via the stdio transport, `McpTransport: Send`, a `Send`-bounded factory alias and `tokio::spawn`. Fixed with the `native` feature, cfg-gated trait/factory, and `spawn_local` in the browser (needs a `LocalSet` at runtime) |

Platform facts these rest on ([rustc platform doc](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html), [tokio #7510](https://github.com/tokio-rs/tokio/pull/7510), [tokio #7550](https://github.com/tokio-rs/tokio/issues/7550)): on `wasm32-unknown-unknown` tokio caps at `sync,macros,io-util,rt,time` (current-thread only): `time` compiles but **panics at runtime**, `net`/`process` don't compile (mio); `std::fs` compiles but always errors; `std::thread::spawn` panics.

### 9.2 Required work (Phase 0 backlog)

| # | Work item | Files / seams | Evidence → |
|---|---|---|---|
| W1 | **Per-target tokio feature gating**: move `net`/`process`/`fs` features into `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` (precedent exists: `cfg(unix)` nix in `sandbox-core`) | `rap-client`, `infinity-mcp-bridge`, `sandbox-core` Cargo.tomls | §9.1 mio errors |
| W2 | **HTTP transport seam for wasm**: reqwest-on-wasm futures are `!Send`; relax/audit `Send` bounds in `rap-protocol`'s client traits or introduce a transport abstraction with a browser (fetch/gloo) impl | `rap-protocol`, `rap-client` | §9.1 E0277 pair |
| W3 | **Browser callback ingestion**: the RAP callback endpoint is a native `TcpListener`+hyper server; a browser can't listen — in-page delivery for browser-hosted threads (SharedWorker/BroadcastChannel), or an always-on endpoint **the user controls** (their own `infinity-daemon`); the edge relay stays optional (D17, §2.5) | `rap-client/src/callback_server.rs` | §9.1 mio error |
| W4 | **getrandom/uuid config**: enable getrandom's `wasm_js` feature + `--cfg getrandom_backend="wasm_js"` build flag ([docs](https://docs.rs/getrandom/0.3.4/#webassembly-support)) — build-config item, not code surgery | workspace `.cargo/config.toml`, uuid consumers | §9.1 getrandom error |
| W5 | **Time without tokio::time in core**: gate `tokio::time` uses; browser slice runners use `wasmtimer`/gloo-timers or an injected scheduler (mirrors the existing `WakeupScheduler`/`SleepTool` abstraction in `infinity-agent-lambda`) | `infinity-agent-core` (`tools/sleep.rs`, `system/`) | tokio #7510 (time panics) |
| W6 | **No direct `std::fs` in core**: `tools/config.rs:125` reads config from disk — inject via the existing `ThreadConfigSource` trait instead; browser uses OPFS/virtual FS (§4) | `infinity-agent-core/src/tools/config.rs` | rustc doc: std::fs always errors |
| W7 | **Sandbox trait/impl split**: keep `ModeProvider`/`SandboxBackend`/`MetadataStore` portable; gate the native impls (git/jj/rg, `which`, `tempfile`, tokio::fs server surface) behind native; browser sandbox = wasmsh/OPFS adapters (§4, §5) | `sandbox-core` (traits vs `server.rs`, `git.rs`, `jj.rs`) | §9.1 + grep inventory |
| W8 | **MCP transports for wasm**: gate `StdioMcpTransport` (native-only); the `McpTransport` trait is already portable — add HTTP/SSE and Web-Worker MessagePort impls | `infinity-mcp-bridge/src/lib.rs:24-94` | §9.1 mio error |
| W9 | **Config-path injection**: replace `dirs::home_dir()` uses with injected paths (native keeps `dirs` behind a gate) | `infinity-protocol/src/lib.rs:194-217` | `dirs-sys` has no wasm target |
| W10 | **Single-threaded runtime discipline**: wasm build uses `Builder::new_current_thread()`; no `std::thread::spawn` in wasm-gated code paths | all wasm-targeted crates | rustc doc: thread::spawn panics |

### 9.3 What Phase 0 buys

After W1–W10, `cargo check --target wasm32-unknown-unknown` passes for the wasm foundation set — protocol crates, `infinity-agent-core`, wasm transports, sandbox traits — which is exactly the substrate `crates/infinity-wasm` (§6) compiles against. The resident/server crates (`infinity-daemon`, `infinity-agent-lambda`, RAP servers, slack bots) stay native and out of the wasm build; the browser and edge runtimes replace, not port, their native I/O surfaces (§4). The `apps/` archetypes (§6) are consumer-side builds entirely — they consume the wasm foundation, they are not part of it.

### 9.4 The HTTP seam: hosts bring their own client

The fork does not hard-code an HTTP stack — the seams already exist upstream and injection is the established pattern:

- **`HttpClient`** (`rap-client/src/http.rs:8-61`): `Send + Sync + Clone`, `async post(url, body) -> u16`, `async get(url) -> (u16, Vec<u8>)`, plus a pluggable **`ToolsetCache`**. A reqwest-based `SimpleHttpClient` ships today; the Lambda runtime injects its own (`RapHttpClient`, `infinity-agent-lambda/src/tools/rap_http.rs:90`); tests inject mocks (`rap_tool.rs:227`, `builder.rs:49`). Agent-core consumes `H: HttpClient` generically (`system/config.rs:38-40`). **[today]**
- **`CallbackClient`** (`rap-protocol/src/lib.rs:241-243`, `post_json`) is the second seam. **[today]**
- In-repo precedent for the wasm fix: agent-core's system traits already use `#[async_trait(?Send)]` (`system/config.rs:38`, `model.rs:32`, `observer.rs:30`) — matching the single-threaded slice model (W10).

Verified per-host clients:

| Host | HTTP the host brings | `HttpClient` impl |
|---|---|---|
| Native daemon / Lambda | reqwest (today) | `SimpleHttpClient` / `RapHttpClient` **[today]** |
| Android / Tauri v2 | Native reqwest inside the Tauri Rust core — [`tauri-plugin-http`](https://v2.tauri.app/plugin/http-client/) re-exports reqwest and adds a scope-enforced, Web-fetch-compliant API for frontend-initiated requests | reqwest(+rustls) impl; reuse the plugin's re-export to pin versions **[plan]** |
| Browser | `fetch` — reqwest's wasm build hooks into JS fetch automatically, or gloo-net | fetch impl; blocked on W2 (`!Send` fetch futures vs `Send` bounds — two candidates: a contained `SendWrapper`-style adapter, or `#[async_trait(?Send)]` bounds if no daemon path requires `Send`; decided in implementation-plan §0.2) **[plan]** |
| Cloudflare Workers | `fetch`-based runtime; reqwest-wasm is documented by Cloudflare; `worker::Fetch` native binding | reqwest-wasm or `worker::Fetch` impl **[plan]** |

Scope caveat: this seam covers RAP tool invocation + toolset discovery (status-code POST, GET bytes). LLM streaming rides the provider abstraction (rig), not `HttpClient`.

---

## 10. Design Ideas: Exploiting the Existing Patterns for WASM **[idea]**

The readiness audit (§9) verified the codebase's recurring patterns; each idea below is derived from one of them, anchored to evidence, and deliberately small because the pattern already exists. Ideas enter the build plan only after a decision (then: `decisions.md` D18+ and the implementation plan's chunk that starts them).

### 10.1 Runtime model

| Idea | Pattern exploited (evidence) | What it buys |
|---|---|---|
| **Browser-native step mode** — the browser is just another serverless host: one user message, push event, or `visibilitychange` = one slice; thread state persists in IndexedDB; a SharedWorker hosts the running slice; tabs are terminals | The step API exists precisely because "nothing survives between slices" (`crates/infinity-agent-core/src/system/builder.rs:399-424`); hibernation model (`README.md`: an agent between turns is "pure data … no task, no stack, no open connection") | No new runtime to invent — the Lambda-style step pipeline runs unchanged in the browser once stores + IO adapters (§4, W6) exist |
| **Optional edge-hosted RAP callback relay** (non-cloud alternative: the user's own `infinity-daemon` as the always-on endpoint, §2.5) — tools POST results to the relay's callback endpoint; it wakes on `tool_result` (DO hibernation API) and relays to the browser via WebSocket/SSE or Web Push | RAP's callback-URL indirection (`rap-protocol/src/lib.rs:51` — tools only need *a* URL); DO hibernation WebSocket API (§4 table; D7, verified) | Solves W3 (a browser cannot `TcpListener`) without changing any tool; the agent stays hibernated at near-zero cost between callbacks |
| **Offline-first threads via `DeferQueue`** — queue user input in an IndexedDB outbox while offline; redeliver on reconnect; subscription events buffer at the relay | `DeferQueue` exists to "hold back an input for later redelivery" (`crates/infinity-agent-core/src/system/defer.rs:27`) | Offline/reconnect becomes a core-semantics feature, not a browser hack |

### 10.2 Injection & assembly

| Idea | Pattern exploited (evidence) | What it buys |
|---|---|---|
| **One `HostCtx` capability bundle** — a single struct the host provides: `HttpClient`, `ToolsetCache`, stores, timer, config source, sandbox, MCP transport factory | Generic injection already everywhere: `H: HttpClient` (`system/config.rs:38-40`, `rap_tool.rs`); `infinity-provider-protocol`'s optional-feature design is the only wasm-green crate (§9.1) — the pattern to replicate | W1/W2/W5/W6/W9 collapse into one assembly point; `infinity-wasm` becomes thin generics over `HostCtx`, and a new host = implement one bundle |
| **Timer as a capability: generalize `WakeupScheduler`/`SleepTool` to core** — promote the Lambda runtime's timer abstraction into a core trait with tokio / wasmtimer / DO-alarm impls | `WakeupScheduler` + `SleepTool`/`SleepUntilTool` (`infinity-agent-lambda/src/tools/sleep.rs:46-115`) already abstract "wake me later" per host | Solves W5 by *promotion* (one trait), not per-target rewrites; the same sleep tool works in every runtime |
| **CI readiness gate** — `cargo check --target wasm32-unknown-unknown` for the core set added to `check.bash` | The §9 sweep methodology | Phase 0 progress becomes a green/red gate that cannot silently regress |
| **Headless wasm tests via the existing mock pattern** — wasm-bindgen-test (§8.5) with recorded-fetch fixtures | Test injection is established: `StubHttp` (`event_processor.rs:4325`), `MockHttp` (`rap_tool.rs:227`), `NoRapHttp` (`builder.rs:49`) | Wasm adapters tested headlessly in CI — no real network, no browser flake |

### 10.3 Tools & sandbox

| Idea | Pattern exploited (evidence) | What it buys |
|---|---|---|
| **One sandbox contract, three runtimes, hot failover** — identical tool schemas against wasmsh-in-Worker (browser), Cloudflare Sandbox/Containers (edge), `sandbox-local` (desktop); `MetadataStore` records which runtime holds the work | `ModeProvider` routes per-mode operations (`crates/sandbox-core/src/sandbox.rs:57-59`); verified edge runtimes (§4.4, §5) | An agent thread can start in-browser and hand execution to edge containers mid-thread — capability scaling without tool-schema churn |
| **Capability-based tool permissions in thread config** — tools declare required capabilities; each host enforces natively (browser fetch allowlist, Tauri URL scopes, wasmsh network broker, Sandbox SDK domain policies) | `ThreadConfigSource` (`system/config.rs:38-40`) is already the per-thread config seam; all four enforcement mechanisms verified this session (§5 wasmsh, §3.4 Tauri, §4.4 Sandbox SDK) | The browser agent gets real permission boundaries from one cross-platform config type — security posture scales with the runtime |
| **MCP-over-RAP for browser agents** — expose MCP toolsets as RAP toolsets via the edge relay; the browser agent reaches arbitrary remote MCP servers with zero local processes | RAP-MCP compatibility layer (`README.md`: "Existing MCP servers run unchanged through a compatibility layer"); `McpTransport` stays the one contract (§4.6, W8) | The browser agent's tool universe is no longer limited to what compiles to WASM |

### 10.4 Continuity & UX

| Idea | Pattern exploited (evidence) | What it buys |
|---|---|---|
| **Observer fan-out: tabs as pure renderers** — the SharedWorker runs slices and fans `ThreadObserver` events to N tabs (BroadcastChannel), the Tauri webview, or a remote browser via WS | `ThreadObserver` exists to "inspect the events after the slice completes" (`system/observer.rs:30-31`) | UIs become pure event renderers — one interface, three fan-outs; matches `infinity-ui`'s event-driven protocol |
| **Cross-host session migration** — export/import threads between hosts: daemon JSON files ⇄ browser IndexedDB ⇄ DO SQLite | "Session serialization for migration" already exists (`infinity-daemon/src/memory_store.rs:12`); store semantics live in `core_stores` (`memory_store.rs:3-5`) | "Start on the laptop, continue in a browser tab, resume at the edge" becomes an export/import — the store-trait design makes it cheap, and it is a genuine differentiator |

**Next step:** pick ideas to promote — each becomes a D18+ decision entry (options/rationale/rejected) and a chunk in [`implementation-plan.md`](./implementation-plan.md); `SliceTimer`/`HostCtx` (10.2) and the optional callback relay (10.1) are the three with the highest leverage, since several W-items collapse into them.

---

*Corrections applied relative to the original blueprint, with evidence, are catalogued in [`audit.md`](./audit.md); decision rationale in [`decisions.md`](./decisions.md).*

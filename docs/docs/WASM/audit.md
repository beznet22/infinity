# Audit Report — `docs/docs/WASM/overview.md`

Claim-by-claim fact-check of the original overview (backed up at `.kimchi/docs/overview-original-backup.md`).
External evidence: `.kimchi/docs/wasm-overview-external-verification.md` (all sources fetched 2026-11-17).

**Verdict legend:** ✅ Verified · ◐ Partial · ❌ False · ❓ Unverifiable · 🔮 Aspirational (plan, not current fact)

## §1 — Repository & Git Worktree Strategy

| # | Original claim | Verdict | Evidence | Correction |
|---|---|---|---|---|
| 1.1 | Workspace "builds simultaneously for Android NDK (aarch64-linux-android), WebAssembly (wasm32-unknown-unknown), and host tests" | ❌ today | `grep wasm32` over `crates/` → 0 hits; no `wasm-bindgen`; no Android target config | No cross-target build exists yet; this is the fork's goal, not current reality. State as plan. |
| 1.2 | Single-branch checkout "shares one working directory and one target/ folder" and "switching back to main … invalidates Cargo artifacts and slows Android/WASM rebuilds" | ◐ | Cargo separates artifacts per target triple (`target/<triple>/`); branch switches do invalidate incremental state and shared caches | Keep the recommendation (worktree), fix the rationale: branch-switch churn and incremental-cache thrash are the real costs; cross-target sharing is only partial (per-triple subdirs). |
| 1.3 | Git worktree = "separate directory linked to the same .git repository, with its own working tree and independent target/ directory" | ✅ | Standard git worktree semantics (`git worktree add`) | Keep as-is. |
| 1.4 | Commands: `git remote add upstream https://github.com` (truncated), `git worktree add ../infinity-hybrid -b feat/wasm-android-hybrid` | ◐ | Syntax valid; upstream URL truncated/incomplete | Fix to full URL `https://github.com/hydro-project/infinity` (workspace `repository` field, `Cargo.toml`). |

## §2 — System Overview & Core Pillars

| # | Original claim | Verdict | Evidence | Correction |
|---|---|---|---|---|
| 2.1 | Architecture "unifies on-device Android autonomy (Tauri v2), local-first browser execution (infinity-wasm), and serverless edge offloading (Cloudflare Workers / Agents) around a single forked Infinity Rust core" | 🔮 | Repo = fork of hydro-project/infinity (`Cargo.toml` workspace.repository); no Tauri/WASM/Cloudflare code exists | Keep as the fork's stated goal; label as target architecture. |
| 2.2 | "Stackless Coroutine Execution (infinity-agent-core) … short, bounded 'slices'" | ◐ | Slice-based step execution is real: `crates/infinity-agent-core/src/system/builder.rs:399-424` ("drops it, which is what makes the step API fit serverless platforms"); `system/local/mod.rs:3` ("shared slice pipeline") | "Stackless coroutine" is not the project's term. Correct to Infinity's verified model: agents yield between turns and run in bounded slices; between turns an agent is "pure data … no task, no stack, and no open connection" (`README.md`). |
| 2.3 | "drops in-memory state to near-zero RAM whenever waiting on an LLM or tool" | ◐ | `README.md`: "a waiting agent hibernates for free"; conversation history remains in the store | "Near-zero RAM" overstates; correct to: no task/stack/connection, conversation history retained in the store. |
| 2.4 | "Dozens of background threads can coexist inside an Android app or browser tab without triggering OS memory kills or UI freezes" | ❓ | Consistent with hibernation model + README's "75k agents in the memory of a Raspberry Pi", but no measurement for this project | Keep as design intent, explicitly labeled unmeasured. |
| 2.5 | RAP: "connectionless HTTP POST requests with callback URLs" | ✅ | `crates/rap-protocol/src/lib.rs:51-52` (`callback_url`, `group_id`), `lib.rs:241-243` (`CallbackClient`), `lib.rs:289-357` (`send_tool_result`, `send_subscription_event`, `send_view_update`) | Keep; verified against crate. |
| 2.6 | RAP "wakes hibernating agents via synthetic events whenever results or subscriptions arrive" | ◐ | RAP delivers callbacks (`tool_result`, `user_choice`, `subscription_event`, `view_update`) to a callback URL; "synthetic events" is not the protocol's vocabulary | Re-word: RAP posts results/subscription events to the callback URL; the receiving runtime feeds them into the thread as tool results/events. |
| 2.7 | "Dual-Target Compilation (aarch64-linux-android + wasm32-unknown-unknown) … Tauri's Android .so library (using rustls and bundled SQLite) and … a WebAssembly module (infinity-wasm)" | ❌ (as fact) | Zero `wasm32`/`wasm-bindgen`/android targets in workspace; no `infinity-wasm` crate; no SQLite anywhere (see 2.9). **Corrected 2026-11-17:** the TLS half was right — reqwest 0.13's `default-tls` *is* rustls (`default-tls = ["rustls"]`, aws-lc-rs provider), so there is no OpenSSL and no TLS switch to plan (see D11 correction) | Present as the fork's compile targets (the aarch64 target now compiles the foundation crates — verified). Drop "bundled SQLite" from the Android claim (see 2.9). |
| 2.8 | "Every OS-dependent crate (sandbox-local, infinity-mcp-bridge, infinity-daemon, infinity-agent-lambda) sits behind a clean Rust trait" | ◐ | Real seams: `McpTransport` (`crates/infinity-mcp-bridge/src/lib.rs:52`); `ConversationStore`/`StateStore` (`crates/infinity-agent-core/src/traits.rs:27,223`) with daemon JSON-file impls and Lambda DSQL/Dynamo impls; sandbox traits `ModeProvider`/`SandboxBackend`/`MetadataStore` (`crates/sandbox-core/src/sandbox.rs:57,155`, `metadata.rs:10`). But there is no single trait layering all four crates; `infinity-daemon` is a binary, not behind a trait | Correct the inventory: name the real traits and the planned adapter traits for the fork (`SandboxExecutor`, `AgentCoordinator`, `SliceRunner` as *to-be-defined* interfaces). |
| 2.9 | "local SQLite" (Android + daemon persistence) | ❌ | Daemon: per-thread JSON files (`crates/infinity-daemon/src/memory_store.rs:3-16`); Lambda: Aurora DSQL + DynamoDB (`crates/infinity-agent-lambda/src/conversation_history.rs:16`, `state_store.rs:9`); `grep -i sqlite crates/` → 0 hits | Replace with verified storage reality: per-thread JSON files (daemon), DSQL+DynamoDB (Lambda), SQLite only on the Cloudflare edge (Durable Objects / Agents SDK local SQL). |
| 2.10 | "run 100% locally on Android or in-browser, or route heavy tasks to Cloudflare Workers/Containers" | 🔮 | Platform capabilities verified (Cloudflare Containers beta 2025-06, Sandbox SDK) | Keep as the fork's capability goal; platform side is verified, the fork's adapter code does not exist yet. |

## §3 — End-to-End Architecture on Android (Tauri v2)

| # | Original claim | Verdict | Evidence | Correction |
|---|---|---|---|---|
| 3.1 | "Standard Android internet permission (INTERNET)" | ✅ | Standard Android manifest permission | Keep. |
| 3.2 | "Native Rust HTTP client using rustls (no OpenSSL) for Anthropic, OpenAI, and Gemini streaming APIs" | ✅ *(corrected 2026-11-17)* | reqwest 0.13's `default-tls` maps to `["rustls"]` with the aws-lc-rs provider (`reqwest-0.13.2/Cargo.toml:119,169`), so the HTTP stack is rustls-only — no OpenSSL. New Android caveat: `aws-lc-sys` needs a C cross-toolchain (NDK clang) and `rustls-platform-verifier` pulls `jni` for the platform cert store | Claim stands. The real work item is Android toolchain wiring, not a TLS switch (see D11 correction and README → "Building for Android"). |
| 3.3 | "Android NotificationListenerService & ContactsContract" | ✅ | Standard Android framework APIs (developer.android.com) | Keep. |
| 3.4 | "Custom Tauri v2 Kotlin plugins exposing @Command methods called asynchronously by Infinity tools" | ✅ | Tauri v2 mobile plugin development: Kotlin class extends `app.tauri.plugin.Plugin`, `@Command` methods, `Invoke` (https://v2.tauri.app/develop/plugins/develop-mobile/) | Keep; pattern verified. |
| 3.5 | "pushing events via RAP subscriptions" | ✅ | `send_subscription_event` verified (`rap-protocol/src/lib.rs:336-357`) | Keep. |
| 3.6 | "Android Scoped Storage / MediaStore or Shizuku file bridge"; "Native tokio::fs inside the app sandbox" | ✅ | Scoped Storage/MediaStore standard; tokio in workspace deps | Keep. |
| 3.7 | WhatsApp: "Hidden or embedded WebView with custom WebViewClient + JS injection; Rust tool sends DOM/action commands over Tauri IPC" | ⚠️ mechanism ✅ / automation ❌ | Verified on device 2026-11-17: an overlay `WebView` with a desktop UA loads `web.whatsapp.com` to `readyState: complete`, renders one `<canvas>` (the QR), and exposes `serviceWorker`, `indexedDB` and `subtleCrypto`; a `screencap` shows the real login screen. | Keep the *view*; drop the automation framing. Pairing needs a human scan, and WhatsApp's Help Center names "unofficial version of WhatsApp" and "scraping" as ban triggers while the Messaging Guidelines forbid "use of unofficial clients … or automation" — the ban lands on the user's number. Recorded as D24. |
| 3.8 | "Shizuku (rikka.shizuku) Binder IPC (newProcess / uiautomator / am)" | ◐ | Shizuku Binder IPC verified (https://github.com/RikkaApps/Shizuku — "Using system APIs directly with adb/root privileges … Binder"); named primitives (`newProcess`/uiautomator/`am`) not on README — plausible Shizuku-API surface, verify at implementation | Keep Binder-IPC claim (verified); mark the specific primitives as implementation-time checks; note ADB permission limits + Android 9 hidden-API limits (documented upstream). |
| 3.9 | "Tauri Kotlin plugin executes privileged shell commands via Shizuku and streams stdout/stderr back" | ❓ | Design; consistent with Shizuku-API process execution | Keep as plan. |
| 3.10 | "Infinity runs native RAP (connectionless HTTP POST + webhook callbacks) for hibernating async tasks" | ✅ | RAP verified (see 2.5); step-mode slices verified (`builder.rs:399-424`) | Keep. |
| 3.11 | "plus standard MCP over HTTP/SSE for remote servers" | ◐ | Only stdio transport exists: `StdioMcpTransport` spawns child processes (`infinity-mcp-bridge/src/lib.rs:66-100`); HTTP/SSE MCP transports are standard MCP but not implemented here | Label HTTP/SSE as a planned transport for the fork; current code is stdio-only (incompatible with browsers — motivation for the plan). |
| 3.12 | Lifecycle step 2: "loads the thread from local SQLite … streams context to Anthropic, OpenAI, or Gemini over rustls" | ❌/◐ | See 2.9 (no SQLite) and 3.2 (rustls) | Correct storage to "from the thread store (JSON-file/DSQL today; IndexedDB/OPFS planned on browser, SQLite-in-DO on edge)"; TLS correction as in 3.2. |
| 3.13 | Lifecycle steps 1, 3, 4 (prompt/trigger → dispatch & hibernate → Foreground Service resume) | ❓ | Internally consistent with verified slice/hibernation + RAP mechanics; Foreground Service is a standard Android API; no code exists | Keep as the fork's runtime plan, labeled as such. |

## §4 — Unified Trait Architecture: Browser Local vs Cloud Offload

| # | Original claim | Verdict | Evidence | Correction |
|---|---|---|---|---|
| 4.1 | "putting unified Rust traits inside infinity-agent-core" | ◐ | Store traits live in `infinity-agent-core/src/traits.rs`; `McpTransport` lives in `infinity-mcp-bridge`; sandbox traits in `sandbox-core` | Correct trait geography in the rewrite; the "define fork adapter traits in agent-core" part is the plan. |
| 4.2 | `SandboxExecutor` trait (run shell/code, read/write virtual FS) | ❌ (name) | No such trait; real sandbox seams: `ModeProvider`, `SandboxBackend` (`sandbox-core/src/sandbox.rs:57,155`), `MetadataStore` (`metadata.rs:10`) | Keep the *role*; rename to a planned trait built on the real sandbox-core seams. |
| 4.3 | Browser adapter: "WASM Bash/Python Runtime + OPFS … inside a Web Worker backed by OPFS" | 🔮 (design, platform ✅) | OPFS Baseline + Web Workers (MDN); Pyodide current (Python 3.14, 314.0.8); wasmsh verified candidate | Keep as plan; platform claims verified. |
| 4.4 | Remote adapter: "Cloudflare Sandbox / Container Worker … real Linux container" | ✅ | Cloudflare Containers public beta (2025-06-24); Sandbox SDK: "A sandbox is a Durable Object and the Container it starts" (developers.cloudflare.com/sandbox) | Keep; verified platform capability. |
| 4.5 | `McpTransport` "(initialize, list tools, call tool)" | ◐ | Trait exists (`infinity-mcp-bridge/src/lib.rs:52`) but shape is single `request(method, params)`; `initialize` is a helper fn; list/call are JSON-RPC methods sent via `request` | Correct signature; keep semantics (initialize/list/call are the JSON-RPC flows it carries). |
| 4.6 | Browser adapter: "Browser MessagePort + HTTP/SSE MCP. Spawns WASM-compiled MCP servers inside dedicated Web Workers (piping JSON-RPC like stdio) or connects directly to remote HTTP/SSE MCP servers" | 🔮 | Design; MessagePort/Worker standard APIs; HTTP/SSE MCP standard | Keep as plan; note stdio-only reality (3.11). |
| 4.7 | Remote adapter: "Cloudflare MCP Relay. Proxies native stdio CLI/Node/Python MCP servers over HTTPS/SSE" | 🔮 | Design; Agents SDK `MCPClientManager` verified as MCP-support capability | Keep as plan. |
| 4.8 | `AgentCoordinator` trait (thread registry, routing, persistence) | ❌ (name) | No such trait; real daemon has session management + `PersistentConversationStore`/`PersistentStateStore` (`infinity-daemon/src/session/mod.rs:11-17`, `memory_store.rs`) | Keep the *role* (registry/routing/persistence); define as planned trait over the real daemon seams. |
| 4.9 | Browser adapter: "Browser SharedWorker + IndexedDB … routing messages via BroadcastChannel" | 🔮 (design, APIs ✅) | SharedWorker/BroadcastChannel/IndexedDB standard MDN APIs; SharedWorker supported in all major engines | Keep as plan. |
| 4.10 | Remote adapter: "Cloudflare Durable Objects. Keeps the coordinator alive 24/7 even after the browser closes" | ◐ | DOs verified, but they are **wake-on-event**: "When an event arrives, the Durable Object wakes up automatically"; idle = evicted from memory, near-zero cost | Correct "alive 24/7" → "reachable 24/7 (always addressable), waking on events at near-zero idle cost." |
| 4.11 | `SliceRunner` trait (wake on event, run bounded slice, hibernate) | ❌ (name) | No such trait. Real serverless slice machinery: `infinity-agent-lambda` (main.rs, `bin/rap-receiver.rs`), `WakeupScheduler` + `SleepTool`/`SleepUntilTool` (`tools/sleep.rs:46,115,182`), DSQL/Dynamo stores | Keep the *role*; define as planned trait over the real Lambda-runtime seams. |
| 4.12 | Browser adapter: "Service Worker + Background Sync / Web Locks. Wakes up on push events or timer alarms to run a single Infinity slice" | ◐ | Web Locks: Baseline ✅. Background Sync: Chromium-only, non-standard, negative Firefox position, "very unlikely to ever become Baseline" (caniuse; web-features-explorer). Periodic Sync: Chrome-only, installed-PWA-only, experimental. No standard SW "timer alarm" API | Correct wake mechanisms: baseline = Web Push + server-initiated RAP callbacks; Background Sync flagged Chromium-only enhancement, not the primary path. |
| 4.13 | Remote adapter: "Cloudflare Workers / Agents SDK. Executes serverless slices at the edge with built-in SQLite hibernation" | ✅ | Durable Objects: SQLite storage classes (`new_sqlite_classes`), hibernation + alarms (developers.cloudflare.com/durable-objects); Agents SDK: durable identity, local SQL storage, scheduled work, recoverable execution | Keep; verified. |

## §5 — Open-Source Browser Sandbox & Shell Repositories

| # | Original claim | Verdict | Evidence | Correction |
|---|---|---|---|---|
| 5.1 | wasmsh: "Clean-room Bash-compatible shell with 88 built-in utilities (grep, sed, awk, jq, curl), Python 3.13 REPL, capability-based VFS, and step/memory budgets" | ✅ (≈) | README verified: "Bash with 88 utilities (grep, sed, awk, jq, tar, curl, …), Python 3.13 …", "Capability-based VFS", "Step budgets"; clean-room: "No GPL code in the core" | Keep; note "memory budgets" = bounded step counts + capped V8 heap in scalable mode; Pyodide has since moved to Python 3.14 (314.0.x). |
| 5.2 | wasmsh use: "Drop-in sandbox-local replacement for Rust-native shell + Python agent execution in a browser Web Worker" | 🔮 | Design; browser-Worker deployment mode verified (`wasm32-unknown-unknown` standalone target) | Keep as recommended candidate for `infinity-wasm`'s sandbox. |
| 5.3 | conch: "Full Bash shell via brush and Rust uutils coreutils compiled to WASM with a hybrid virtual filesystem and @bsull/conch JS/TS bindings" | ◐ | brush verified; hybrid VFS verified; `@bsull/conch` verified. **uutils: FALSE** — no uutils anywhere in README; utilities are brush builtins + cat/head/tail/wc/grep/sort/uniq/jq | Drop the uutils claim; describe conch as brush-based with a curated utility set; add verified facts: wasmtime sandbox, MCP server, Go FFI, ResourceLimits. |
| 5.4 | conch use: "Heavyweight POSIX shell compatibility when your agent runs complex bash pipelines in-browser" | 🔮 | Design; browser bindings verified | Keep as secondary candidate. |
| 5.5 | agent-sandbox: "80+ CLI tools, sh/bash interpreter, Boa JS runtime, filesystem change tracking, and SSRF-safe HTTP" | ✅ | All five verified (README: 80+ tools list; full shell interpreter; Boa engine; "Change tracking via filesystem snapshots"; agent-fetch SSRF protection) | Keep; add: WASI-based, <13ms cold start, Rust + NAPI bindings, resource limits. |
| 5.6 | agent-sandbox use: "Reference design for agent tool schemas and filesystem diff/change-tracking hooks" | 🔮 | Design; supported by repo (diff API) | Keep. |
| 5.7 | web-container: "Browser-native code container combining esbuild-wasm, QuickJS WASM sandboxing, Rust process management, and persistent OPFS storage" | ✅ (content) / ⚠ (repo) | All stack items verified (esbuild-wasm, quickjs-emscripten, Rust WASM runtime via wasm-pack, OPFS+IndexedDB). **But repo is 0★ / 0 forks / 1 commit** — unmaintained hobby demo | Keep as *reading reference only* (OPFS layer + bundling pipeline design); do not treat as dependency or active project. |
| 5.8 | web-container use: "Copying the OPFS persistence layer and JS/TS bundling pipeline inside a Web Worker" | 🔮 | Design | Keep, flagged reference-only. |
| 5.9 | browser-fs: "Async filesystem crate mirroring std::fs / async-fs on top of the browser File System / OPFS APIs" | ◐ | `std::fs`/`async-fs` mirroring ✅; "on top of the browser **File System Access API**" — **not OPFS** (README; requires Web Worker context) | Correct: File System Access API-based, not OPFS; must run in a Web Worker; 4GB file cap; `forbid(unsafe_code)`. |

## §6 — Fork Workspace Layout

| # | Original claim | Verdict | Evidence | Correction |
|---|---|---|---|---|
| 6.1 | Crates: infinity-protocol, rap-protocol, infinity-agent-core, sandbox-local exist | ✅ | All in `Cargo.toml` workspace members | Keep. |
| 6.2 | Crates: infinity-wasm, infinity-remote-adapters, apps/local-first/android, apps/cloudflare-worker | 🔮 | Not in workspace; planned — **user-corrected scope (post-audit):** the fork ships **crates**; `apps/` are integration archetypes for consumer codebases, deliberately not fork members (→ `decisions.md` D16) | Split the labels: `infinity-wasm`/`infinity-remote-adapters` = to-be-created fork crates; `apps/*` = consumer-side archetypes. |
| 6.3 | Layout omits real crates: sandbox-core, sandbox-remote, infinity-daemon, infinity-agent-lambda, infinity-mcp-bridge, infinity-agent-cli, infinity-provider-{protocol,rig,bedrock}, rap-client, rap-steering-server, rap-github-event-poller, rap-test-servers, infinity-rap-bridge, infinity-mdtests, infinity-slack-{bot,dataflow} | ◐ | Full member list in `Cargo.toml` | Correct the layout to reconcile planned additions with the actual upstream member list, **and mark `apps/` as consumer-side archetypes** (D16) — the original presented them as fork directories. |
| 6.4 | Closing build-order options 1–4 (core traits → Android plugins → browser runtime → Cloudflare offload) | 🔮 | Plan | Preserve intent in the rewrite's closing "build order" section — with items 2/4 reframed as **consumer archetypes** (D16). |

## Appendix A — Feature Preservation Map (original → rewrite)

| Original element | Where it lands in the rewritten overview |
|---|---|
| §1 worktree vs single-branch table + commands | §1 (rationale corrected per 1.2, upstream URL fixed per 1.4) |
| §2 pillar: bounded-slice/hibernation execution | §2.1 (terminology corrected per 2.2–2.4) |
| §2 pillar: RAP | §2.2 (verified mechanics kept) |
| §2 pillar: dual-target compilation | §2.3 (labeled plan; storage/TLS corrections applied) |
| §2 pillar: swappable local/cloud adapters | §2.4 (real trait inventory + planned traits) |
| §3 capability table (6 rows) | §3 table — all rows preserved with verdicts applied |
| §3 runtime lifecycle (4 steps) | §3 lifecycle — all steps preserved, labeled plan |
| §4 trait-architecture table (4 rows × 4 cells) | §4 table — all cells preserved; false trait names corrected to planned-adapter framing; real traits cited |
| §5 five reference repos | §5 — all five kept, corrected descriptions, verdicts noted |
| §6 workspace layout | §6 — reconciled with actual workspace + planned fork crates; `apps/` marked consumer-side archetypes (D16) |
| Closing "which module to build first" options 1–4 | §7 closing build-order recommendation |
| *(additive)* — | §8 WASM Development & Debugging Tooling (new, per approved plan) |

## Appendix B — Evidence spot-checks

`file:line` citations in this report resolve as follows (checked this session):

- `crates/infinity-agent-core/src/traits.rs:27` → `pub trait ConversationStore` ✅
- `crates/infinity-agent-core/src/traits.rs:223` → `pub trait StateStore` ✅
- `crates/infinity-agent-core/src/system/builder.rs:399` → step-mode/slice drop comment ✅
- `crates/infinity-mcp-bridge/src/lib.rs:52` → `pub trait McpTransport` ✅
- `crates/rap-protocol/src/lib.rs:51` → `callback_url` field ✅
- `crates/rap-protocol/src/lib.rs:241` → `CallbackClient` trait ✅
- `crates/infinity-daemon/src/memory_store.rs:3-16` → JSON-file persistence doc comment ✅
- `crates/infinity-agent-lambda/src/conversation_history.rs:16` → `DsqlConversationStore` ✅
- `crates/infinity-agent-lambda/src/state_store.rs:9` → `DynamoDbStateStore` ✅
- `crates/sandbox-core/src/sandbox.rs:57` → `pub trait ModeProvider` ✅
- `crates/sandbox-core/src/sandbox.rs:155` → `pub trait SandboxBackend` ✅
- `crates/sandbox-core/src/metadata.rs:10` → `pub trait MetadataStore` ✅
- `Cargo.toml:45` → reqwest `default-tls` ✅ · `Cargo.toml:59` → sqlx `tls-rustls-aws-lc-rs` ✅
- `grep wasm32 crates/` → 0 hits ✅ · `grep -i sqlite crates/` → 0 hits ✅

**Totals:** 53 claim rows audited across the 6 sections — 54 verdict assignments: 14 ✅, 17 ◐, 7 ❌, 4 ❓, 12 🔮. The extra assignment is row 3.12 (split ❌/◐); row 5.7's ⚠ repo-health note is not a verdict. Zero claims left without a verdict.

## Appendix C — Codebase WASM-readiness audit (compiler-verified, 2026-11-17)

Method: `rustup target add wasm32-unknown-unknown` (was already installed), then `cargo check --target wasm32-unknown-unknown -p <crate>` per crate (dev profile), supplemented by grep inventory of blocking APIs and primary-source platform docs. Full verbatim evidence: `.kimchi/docs/wasm-overview-external-verification.md` §6. This appendix feeds overview §9 (readiness) and decision D14.

| Crate | `wasm32` check | Blocker (verbatim / location) |
|---|---|---|
| `infinity-provider-protocol` | ✅ compiles clean (5.80s) | — heavy deps already behind optional `mock`/`remote` features (`Cargo.toml` `[features]`) |
| `rap-protocol` | ❌ | 2× `E0277` — "`Rc<RefCell<js_sys::futures::Inner>>` cannot be sent between threads safely", "`(dyn FnMut() + 'static)` cannot be sent between threads safely" (reqwest wasm fetch futures are `!Send`) |
| `infinity-protocol` | ❌ (transitive) | fails inside dep `rap-protocol`; own `dirs` dep (`lib.rs:194-217` `dirs::home_dir()`) is the expected next blocker (no `dirs-sys` wasm target) |
| `rap-client` | ❌ | mio: "This wasm target is unsupported by mio. If using Tokio, disable the net feature." — tokio `net` + hyper server (`Cargo.toml` `tokio` `net`; `callback_server.rs:4-18` `TcpListener::bind`) |
| `infinity-agent-core` | ❌ (transitive) | mio via dep `rap-client`. Post-fix runtime blockers (grep): `tokio::time::timeout` (`system/local/handle.rs:291`), `tokio::time::sleep` (`tools/sleep.rs:100`), `tokio::time::Instant` (`event_processor.rs:5065`), `std::fs::read_to_string` (`tools/config.rs:125`); desktop-only `system/local/` module |
| `sandbox-core` | ❌ | mio (tokio `process`/`fs` + axum server); native surface: `tokio::process::Child` in core type (`sandbox.rs:11`), `git`/`jj`/`rg` spawning (`git.rs:11`, `jj.rs:10,181,193`, `server.rs:1790`), `which::which("rg")` (`server.rs:1746`), `tempfile` (`server.rs:2083,2244`), `std::os::unix::fs::symlink` |
| `infinity-mcp-bridge` | ❌ | mio (tokio `process`, `lib.rs:24-94` `StdioMcpTransport`) + getrandom 0.3.4: "The wasm32-unknown-unknown targets are not supported by default; you may need to enable the \"wasm_js\" configuration flag" (via `uuid` v4) |

Platform support sources:
- [rustc: wasm32-unknown-unknown platform support](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html) — Tier 2; "`std::fs` always return errors, and `std::thread::spawn` will panic"; `core`/`alloc` full.
- [tokio PR #7510](https://github.com/tokio-rs/tokio/pull/7510) (merged 2025-09-09) — "`Instant::now()` panics on unknown WASM … using time APIs (sleep) panics on unknown WASM."
- [tokio issue #7550](https://github.com/tokio-rs/tokio/issues/7550) (open) — upstream `fs`/`threads`/`timer` on wasm32-unknown-unknown is an open feature request.
- [tokio issue #8194](https://github.com/tokio-rs/tokio/issues/8194) (2026-06) — wasm-not-wasi bucket caps tokio at `sync,macros,io-util,rt,time`, "no `net`/`fs". Note: `time` is inside the cap yet its APIs panic at runtime on this target (#7510) — compiling ≠ working.
- [getrandom 0.3 wasm support](https://docs.rs/getrandom/0.3.4/#webassembly-support) — documented `wasm_js` backend config.

Derived work items W1–W10 with file targets: overview §9.2. Not in wasm scope: `infinity-daemon`, `infinity-agent-lambda`, RAP servers, slack bots (resident/server binaries — the browser and edge runtimes replace their native I/O surfaces per overview §4), and the `apps/` archetypes (consumer-side builds entirely — D16).

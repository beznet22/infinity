# Decisions Log — WASM Overview Audit & Rewrite

The research → brainstorm → decide record behind the rewritten `overview.md`. Each decision cites audit rows (see `audit.md`) and external evidence (`.kimchi/docs/wasm-overview-external-verification.md`). Status markers: ✅ decided · 🔁 revisit trigger noted.

## D1 — Document framing: corrected fork blueprint ✅
- **Options:** (a) corrected fork blueprint, (b) current-state doc + roadmap, (c) research dossier.
- **Decision:** (a) — keep the original's explicit purpose ("complete project blueprint consolidating repository workflow, core runtime architecture, Android Tauri v2 integration, browser WASM adapters, Cloudflare edge offloading, and open-source reference repositories") with every hypothesis fact-checked.
- **Rationale:** The user requirement is to preserve "the exact purpose and intention of the original overview (we must not drop any feature)". (b)/(c) would change the document's genre, violating that requirement.
- **Rejected:** (b), (c) — both shift purpose away from a go-forward build plan.
- **Mechanism:** plan-vs-existing separation is structural — sections describing existing upstream code carry `file:line` evidence; sections describing fork work are labeled **Plan** and cite verified platform capabilities only. (Audit framing rows 2.1, 2.7, 2.10.)

## D2 — Correction policy: correct to reality, keep intent ✅
- **Options:** (a) correct to reality, (b) keep as labeled plan, (c) drop disproven content.
- **Decision:** (a), with nuance: where the original stated unbuilt work as current fact (SQLite, trait names, dual-target builds), the *factual error* is corrected while the *intent* (the fork plan) survives, explicitly labeled.
- **Rationale:** "Correct to reality" without the nuance would delete the blueprint's roadmap; (b) alone would leave false statements uncorrected; (c) violates the no-drop requirement.
- **Applied at:** storage corrections (2.9, 3.12), trait-name corrections (2.8, 4.2, 4.8, 4.11), dual-target compilation (2.7), TLS (3.2), conch uutils (5.3), browser-fs OPFS→File System Access API (5.9), DO "alive 24/7" → "reachable 24/7" (4.10).

## D3 — Storage story: JSON files / DSQL / Dynamo today; SQLite only at the edge ✅
- **Evidence:** daemon persists per-thread JSON files (`infinity-daemon/src/memory_store.rs:3-16`); Lambda uses Aurora DSQL + DynamoDB (`infinity-agent-lambda/src/conversation_history.rs:16`, `state_store.rs:9`); zero `sqlite` in `crates/`.
- **Decision:** the rewrite says exactly this. "Bundled SQLite on Android" is dropped as a claim about today; if the fork wants Android SQLite, that is a future store-trait implementation choice (`ConversationStore`/`StateStore`, `infinity-agent-core/src/traits.rs:27,223`). SQLite appears only where verified: Cloudflare Durable Objects / Agents SDK.
- **Rejected:** keeping "local SQLite" (false for daemon/Lambda); inventing a SQLite migration plan (out of scope).

## D4 — Trait naming: real traits stay real; fork traits are named as planned ✅
- **Evidence:** `McpTransport` exists but is `request(method, params)` (`infinity-mcp-bridge/src/lib.rs:52-59`); `SandboxExecutor`/`AgentCoordinator`/`SliceRunner` do not exist; real seams are `ConversationStore`, `StateStore`, `InputSender`, `DeferQueue`, `ModelSource`, `ThreadObserver`, `Tool` (`infinity-agent-core/src/traits.rs`, `tools/mod.rs:68`), `ModeProvider`/`SandboxBackend`/`MetadataStore` (`sandbox-core`).
- **Decision:** the rewrite's §4 table keeps the four original *roles* (sandbox executor / MCP transport / coordinator / slice runner) but grounds each cell: role → existing code seam (cited) → planned fork trait. Planned names (`SandboxExecutor`, `AgentCoordinator`, `SliceRunner`) are retained as the fork's proposed interfaces, explicitly marked "to be defined", since the original's vocabulary is reasonable and dropping it would be a needless rename debate now.
- **Rejected:** asserting the planned traits as existing (fabrication); renaming roles to invented terms (drops original intent).

## D5 — RAP description: keep verified mechanics, fix vocabulary ✅
- **Evidence:** callback URL, `group_id`, `CallbackClient`, four callback types (`rap-protocol/src/lib.rs:51-52,241-243,289-357`); RAP spec exists on the docs site (README).
- **Decision:** describe RAP as connectionless HTTP POST invocations with callback URLs and typed callbacks (`tool_result`, `user_choice`, `subscription_event`, `view_update`) — all verified. Replace "wakes hibernating agents via synthetic events" with the delivery-into-runtime phrasing (audit 2.6).
- **Rejected:** keeping "synthetic events" (not the protocol's vocabulary).

## D6 — Wake mechanisms for the browser slice runner: Push first, Background Sync demoted ✅
- **Evidence:** Background Sync is Chromium-only, non-standard, negative Firefox standards position ("very unlikely to ever become Baseline"); Periodic Sync is Chrome-only + installed-PWA-only + experimental; Web Push and Web Locks are Baseline.
- **Decision:** the rewrite's browser wake path = Web Push + server-initiated RAP callbacks (baseline), with Web Locks for cross-tab coordination; Background Sync kept as a Chromium-only enhancement with an explicit portability warning. No standard SW "timer alarms" claim — edge timer wakes are Cloudflare DO alarms (verified).
- **Rejected:** presenting Background Sync as the primary wake mechanism (audit 4.12).

## D7 — Cloudflare edge story: verified as-is, one correction ✅
- **Evidence:** Durable Objects hibernation (wake-on-event, SQLite-backed), Agents SDK (durable identity, SQL storage, Scheduler, MCPClientManager), Containers public beta, Sandbox SDK ("a sandbox is a Durable Object and the Container it starts").
- **Decision:** keep all four §4 remote-adapter cells; correct "keeps the coordinator alive 24/7" → "reachable 24/7, waking on events at near-zero idle cost" (audit 4.10) — this matches how DO hibernation actually bills and behaves.

## D8 — Reference repos: ranked with corrected descriptions ✅
- **Verdicts:** wasmsh all-but-verified (88 utilities, Python 3.13, capability VFS, step budgets); agent-sandbox fully verified (80+ tools, Boa, SSRF-safe fetch, snapshots, resource limits); conch brush-based but **no uutils** (claim dropped); browser-fs is File System Access API **not OPFS** (corrected); web-container content verified but **1-commit, 0-star** → reference-only.
- **Decision:** §5 keeps all five repos in the original order, each with a corrected description and a "fitness for infinity-wasm" line. wasmsh remains the primary sandbox candidate; agent-sandbox the design reference for tool schemas/change-tracking; conch the bash-compatibility fallback; web-container the OPFS/bundling *reading* reference (explicitly flagged unmaintained); browser-fs a candidate FS adapter (noting its API basis and Worker requirement).
- **Rejected:** removing any repo (violates no-drop); adding replacements not requested.

## D9 — Toolchain standardization for the fork (new §8) ✅
- **Evidence table:** research note §4 (all tools verified 2026-11-17).
- **Decision:** the fork's standard loop is:
  - **Build:** `rustup target add wasm32-unknown-unknown`; `wasm-pack build` (0.15.1, now under the wasm-bindgen org, actively maintained); `wasm-bindgen` for JS interop.
  - **Dev loop:** `trunk serve` (watch + auto-reload) for app-style iteration; `wasm-pack`/raw `wasm-bindgen` output for library-style crates consumed by the Tauri webview or a bundler.
  - **Inspect/profile:** `wabt` (`wasm2wat`, `wasm-objdump`, `wasm-validate`); `twiggy top` for size — with an archived-upstream warning (crates.io 0.8.0 still fine).
  - **Debug:** `console_error_panic_hook` for readable panics; DWARF path (`dwarf-debug-info = true` + `-C debuginfo` + Chrome "C/C++ DevTools Support (DWARF)" extension, Chrome 114+) for source-level Rust debugging.
  - **Test:** `wasm-bindgen-test` via `wasm-pack test --headless chrome`.
- **Rejected:** presenting twiggy/wabt without maintenance caveats; skipping the DWARF extension requirement (it's still needed despite Chrome 114+ defaults for wasm).

## D10 — chrome-devtools-mcp: adopt as the agent-facing debugging layer ✅
- **Evidence:** official ChromeDevTools project (57.7k★, Apache-2.0); MCP server + CLI; performance traces, network, screenshots, console with source-mapped stack traces; `npx -y chrome-devtools-mcp@latest`; skills + plugin installs (Claude Code, Copilot/VS Code, Gemini CLI, `.kimi-code-plugin` present); `--headless/--isolated/--slim` flags; Android debugging doc.
- **Decision:** adopt it as the coding-agent tool for the infinity-wasm build→load→debug loop: the agent builds, opens the served page, reads console/source-mapped Rust traces, inspects network (RAP/MCP calls), records performance traces, and iterates unattended. Wire into the section as install + config snippet + concrete WASM-debug workflow.
- **Scope note (user feedback):** it is one piece of the general tooling section (D9), not the whole section.
- **Rejected:** puppeteer-playwright-MCP alternatives (not requested; chrome-devtools-mcp is official and current).

## D11 — TLS: state the real config; rustls is a planned switch ✅
- **Evidence:** `reqwest` = `default-tls` (native-tls → OpenSSL on Linux) at `Cargo.toml:45`; rustls enters via `sqlx` `tls-rustls-aws-lc-rs` (`Cargo.toml:59`).
- **Decision:** the rewrite stated the configuration as understood at audit time and listed a rustls switch as a possible work item (audit 3.2).
- **Correction (2026-11-17, Phase 2 research):** the premise was wrong. reqwest **0.13** redefined `default-tls = ["rustls"]` (aws-lc-rs provider + `rustls-platform-verifier`), so the workspace is already rustls-only and there is **no OpenSSL**. The *original overview's* "rustls (no OpenSSL)" was accurate; audit rows 2.7/3.2 are corrected accordingly. The real Android work item is toolchain wiring — `aws-lc-sys` needs the NDK clang via `CC_<target>`/`AR_<target>`/linker env, and `rustls-platform-verifier` uses `jni`. **No TLS feature switch is planned.**

## D12 — WhatsApp / Android control rows: kept as labeled, unverified-by-nature plan ✅
- **Rationale:** these describe *intended capabilities* of a not-yet-written app layer. They cannot be verified against this repo and some (WhatsApp automation) are inherently implementation-dependent.
- **Decision:** keep the rows (no-drop), label the section "Android plan (not yet built)", and restrict verified statements to the underlying platform APIs (Tauri Kotlin plugins, Shizuku Binder IPC, WebView/WebViewClient, Foreground Services, NotificationListenerService, ContactsContract, Scoped Storage) — all confirmed via official sources. Shizuku's specific primitives (`newProcess`/uiautomator/`am`) are marked implementation-time checks (audit 3.8).

## D13 — Deliverables & placement ✅
- **Decision:** three artifacts in `docs/docs/WASM/`: `audit.md` (this repo, claim-by-claim + preservation map), `decisions.md` (this file), and the rewritten `overview.md` (in place). Original backed up at `.kimchi/docs/overview-original-backup.md`. None wired into `docs/sidebars.ts` (docusaurus autogenerated dirs cover only infinity-code/infinity-runtime/rap) — publishing to the site is a possible follow-up, out of scope here.
- **Rejected:** single-file output (loses the audit trail); putting working notes in the project tree (harness rule: Documents dir for transient research).
- **Updated (post-audit):** the artifact set has since grown to include `PROGRESS.md` (status tracker) and `implementation-plan.md` (Phases 0–4 chunk spec); D13's placement rationale applies to both.

## D14 — WASM build readiness: compiler-first audit; gate, don't rewrite ✅
- **Trigger (user):** the audit must also capture the work required for a successful `wasm32-unknown-unknown` build of this project — not just verify the overview's claims.
- **Method decision:** the definitive evidence is the compiler — `cargo check --target wasm32-unknown-unknown` per crate (7 crates swept 2026-11-17; 1 green, 6 red) — corroborated by API greps (`std::fs/process/thread`, `tokio::net/time/fs`) and primary sources (rustc platform doc, tokio #7510/#7550/#8194, getrandom docs). Dep-graph reasoning alone was rejected as insufficient (it cannot see `Send`-bound clashes like rap-protocol's E0277s).
- **Strategy decision — feature-gate per target, keep one core:** tokio `net`/`process`/`fs` and native-only impls (git/jj/rg, `which`, `tempfile`, `dirs`, `StdioMcpTransport`) move behind `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` / `#[cfg]` gates; browser equivalents live in `infinity-wasm` (§4/§5 of the overview). Precedent already in-repo: `[target.'cfg(unix)'.dependencies] nix` in `sandbox-core`. The proven-good pattern to replicate is `infinity-provider-protocol` (only wasm-green crate: everything heavy behind optional features).
- **Rejected alternatives:** (a) dropping tokio from the core for a wasm-native executor — unnecessary churn: tokio's `sync,macros,io-util,rt` (current-thread) are wasm-usable today; (b) pinning community forks (utooland/tokio, tokio-fs-ext, wasmtimer as tokio replacements) — acceptable as *point* solutions behind gates (e.g. wasmtimer for timers), not as workspace-wide dependency forks; (c) forcing the whole workspace (daemon/lambda/servers) to compile for wasm — out of scope: those are resident/server binaries; the browser/edge runtimes replace their native I/O surfaces.
- **Notable findings folded into overview §9:** reqwest-on-wasm futures are `!Send` (needs a transport seam or bound audit in `rap-protocol`); rap-client's RAP callback endpoint is a native TCP/hyper server and cannot exist in a browser (edge-hosted callback endpoint or SSE/WS/polling ingestion); getrandom/uuid v4 needs the documented `wasm_js` backend build config (cheap); `tokio::time` panics and `std::fs` errors at runtime on wasm-unknown even after compile fixes; only `infinity-provider-protocol` compiles green today.
- **Evidence:** audit.md Appendix C; research note §6; overview §9 (per-crate table + work items W1–W10 + Phase 0 in §7).
- **Resolution for `sandbox-core` (chunk 0.6, 2026-11-17):** gating uses a **default-on `native` feature** (not automatic target cfg, per user decision), covering `server`/`git`/`jj`, `SpawnedCommand`/`SandboxBackend::spawn_command`, and 8 native-only dependencies. Consequences recorded in `docs/docs/WASM/README.md`: browser consumers must pass `default-features = false`, and the readiness gate special-cases this crate with `--no-default-features` (otherwise the default build would keep the gate red and hide regressions). `which` 8.0.2 turned out not to compile for wasm at all (`E0046: missing is_valid_executable`), not merely to be unused.
- **Resolution for `infinity-mcp-bridge` (chunk 0.7, 2026-11-17):** the same default-on `native` feature gates the stdio transport. Gating it alone was insufficient: `McpTransport` needed the cfg-gated `Send`/`?Send` treatment, `McpTransportFactory` needed two aliases (bounds cannot be cfg'd inside a type alias), and `McpTool::execute` needs `tokio::task::spawn_local` on wasm. **Phase 3 prerequisite:** the browser runtime must execute inside a `LocalSet`, otherwise spawned tool-result delivery panics.

## D15 — Host-provided HTTP: keep the `HttpClient` seam; one impl per host ✅
- **Question (user):** can the host environment plug in its own HTTP — browser brings fetch, Tauri v2 brings …?
- **Answer: yes, and the architecture already does this.** `HttpClient` (`rap-client/src/http.rs:8`) + `ToolsetCache` are injected today (reqwest impl ships; Lambda injects its own `RapHttpClient`; tests inject mocks — `rap_tool.rs:227`, `builder.rs:49`); `CallbackClient` (`rap-protocol/src/lib.rs:241`) is the second seam; agent-core consumes `H: HttpClient` generically (`system/config.rs:38-40`).
- **Decision:** keep the seam as the single HTTP contract; add one impl per host — Tauri v2 = native reqwest(+rustls) in the app process (optionally `tauri_plugin_http::reqwest` re-export; the plugin's scope-enforced fetch covers frontend-initiated calls), browser = fetch via reqwest-wasm/gloo (unblocked by W2 — candidates: a contained `SendWrapper` adapter, or `#[async_trait(?Send)]` bounds if no daemon path requires `Send`, as in agent-core's system traits; implementation-plan §0.2 records the choice), Workers = reqwest-wasm (Cloudflare-documented) or `worker::Fetch`. Full matrix: overview §9.4.
- **Rejected:** hard-coupling all hosts to reqwest's native stack (breaks browser/Workers — audit Appendix C findings); moving LLM streaming onto `HttpClient` (streaming belongs to the provider seam; this trait is status-POST/GET-bytes for RAP).
- **Evidence:** research note §7 (Tauri plugin docs, Cloudflare crates doc, in-repo trait inventory).
- **Resolution (2026-11-17, chunks 0.2/0.3):** implemented as **cfg-gated trait definitions**, not a wrapper. Native keeps `#[async_trait]` with `Send + Sync`; wasm uses `#[async_trait(?Send)]` without those bounds. Request bodies live in shared inherent methods, so each request is implemented once.
  - **Why not the `SendWrapper` candidate:** the `!Send` value is *inside* reqwest's future (`JsFuture` holds thread-local JS objects) — wrapping the client cannot make the future `Send` without `unsafe`, which is unjustified on a single-threaded runtime.
  - **Why not unconditional `?Send`:** the axum-based servers (`sandbox-core` HTTP server, `rap-steering-server`) require handler futures to be `Send`; dropping the bound globally fails to compile them.
  - **Verified:** `rap-protocol` and `rap-client` green for `wasm32-unknown-unknown`; `cargo check --workspace --all-targets` green on native.
  - **Follow-on:** the same pattern must be applied to `Tool`, `InputSender`, and the store traits in `infinity-agent-core` (chunk 0.5) — the source of its 15 remaining wasm errors.

## D16 — Fork scope: crates are the product; `apps/` are consumer archetypes ✅
- **Correction (user):** `apps/local-first/android` and `apps/cloudflare-worker` are not fork-workspace directories — they are archetypes for any codebase that consumes the fork.
- **Decision:** the fork's deliverable surface is **crates** (core, `infinity-wasm`, `infinity-remote-adapters`, sandbox traits). Android Tauri apps and Cloudflare workers are built by consumers on top of those crates; the blueprint documents the archetypes (§3 capability table, §6 layout note, §7 build-order items 2/4) as reference implementations that double as integration tests for the crate APIs.
- **Rationale:** keeps the fork a framework — no app concerns (signing, store deployment, wrangler accounts, per-consumer capabilities/scopes). Consumers own their app shells, permission configuration, and deployment topology.
- **Rejected:** shipping `apps/` as fork workspace members (couples the framework to one app shape and one deployment); dropping the archetypes from the blueprint (they are the integration documentation consumers need).
- **Evidence:** audit §6 rows 6.2 (corrected), 6.3/6.4 (reframed), Preservation Map §6 row, Appendix C scope list; overview §3 intro, §6 scope note, §7 build-order items 2/4, §9.3 out-of-scope note.

## D17 — Cloud-optional by design: full-featured local operation, edge as accelerator ✅
- **Requirement (user):** the runtime must run full-featured — including long-running tasks — without depending on a cloud worker; the cloud worker is optional, for heavy-duty / very-long work. Verify the possibility **before** refactoring.
- **Verification (done first, 2026-11-17):** (1) Android long-running = foreground service `START_STICKY` via Tauri v2 background-service plugin (verified; Android 15 FGS timeout policies are the very-long caveat); (2) browser long-running = SharedWorker slices while the tab is open + hibernate/resume via the native slice model — background-timer throttling (Chrome M87+, ~1/min when hidden) delays wakes without breaking invariants; (3) LLM calls go device→provider directly (Anthropic CORS header verified; OpenAI `ACAO: *` — empirical matrix 2026-07); (4) RAP callback URLs are host-agnostic — the Android app hosts callbacks locally (`callback_server.rs:16-18`) and the user's own `infinity-daemon` is a non-cloud always-on endpoint; (5) local tool paths exist on both hosts (Shizuku native / wasmsh+OPFS).
- **Decision:** reframe the blueprint intro (cloud = optional accelerator, never a requirement); add overview §2.5 with the verified capability/boundary table; mark the §10.1 relay idea optional with the self-hosted-daemon alternative. All "route heavy tasks to Cloudflare" phrasing stays conditional.
- **Rejected:** making the edge a required backbone (violates the requirement); claiming a browser can receive events while closed with zero infrastructure (false — needs the user's own always-on endpoint, or deferral to next open).
- **Evidence:** research note §8; overview §2.5.

## D18 — `SliceTimer` promotion: deferred to Phase 3 (runtime, not compile) ⏸
- **Original plan:** chunk 0.5 would promote the Lambda timer abstraction (`WakeupScheduler`/`SleepTool`, `infinity-agent-lambda/src/tools/sleep.rs:46-115`) into a core `SliceTimer` trait with tokio / wasmtimer / DO-alarm impls.
- **Discrepancy found (2026-11-17, chunk 0.5 research):** `tokio::time` **compiles** for `wasm32-unknown-unknown` and only panics when used at runtime (tokio #7510); `std::fs` likewise compiles and errors at runtime; `event_processor.rs:5065`'s `tokio::time::Instant` is inside `#[cfg(test)]`; `system/local/` has no process/net/thread usage and compiles. None of them blocked the wasm build — the only compile blockers were the `Send` bounds.
- **Decision (user, 2026-11-17):** chunk 0.5 is scoped to **compile-green only** (the `Tool` cfg-gating). `SliceTimer`, the `std::fs` config-path work and optional `system/local` gating move to **Phase 3**, where the browser runtime actually needs a wake mechanism.
- **Numbering stays reserved:** D20 = `HostCtx` (chunk 1.4), D21 = optional edge relay (chunk 4.x).
- **Risk if forgotten:** `TokioSleepTool` / `TokioSleepUntilTool` / `SleepUntilEventOrInputTool` would panic if invoked in a browser — tracked as W5 in `PROGRESS.md` §3 and in the revisit triggers below.

## D19 — Phase 1 scope: additive-only; coordinator/runner extraction deferred ✅
- **Finding (research, 2026-11-17):** three of the five planned Phase 1 chunks did not match reality.
  1. **Chunk 1.5 is already implemented upstream** — `HttpMcpTransport` sends `accept: application/json, text/event-stream`, tracks `mcp-session-id` and parses `data:` frames. The chunk became "add tests for that path".
  2. **`SliceRunner` would wrap `AgentSystem::step`**, which already exists (`builder.rs:453`); the Lambda crate that would be refactored has **0 tests** and cannot be exercised locally without AWS.
  3. **`AgentCoordinator` would wrap a 1,918-LOC `SessionManager`** that is `Rc`/`!Send` and `LocalSet`-bound (14 tests), while Phase 3 (SharedWorker) and Phase 4 (Durable Objects) each need a *different* shape.
- **Decision (user):** Phase 1 is **trimmed** — 1.1 `SandboxExecutor` (additive, portable), 1.4 `HostCtx` **without** the deferred `SliceTimer`/`CallbackIngest` slots, and 1.5 as tests only. The `SliceRunner` / `AgentCoordinator` extractions move to Phase 3/4, where a second implementation forces the correct abstraction and the code is testable.
- **Rejected:** all five chunks as written (unverifiable AWS refactor, abstraction guessed from one implementation); only 1.1 (leaves no assembly point for Phases 2–3).
- **Consequence:** overview §4 still lists the four adapter roles, but only `SandboxExecutor` and `McpTransport` are realised in Phase 1; `SliceRunner` / `AgentCoordinator` remain planned names until Phase 3/4.

## D20 — `HostCtx` lives in a new `infinity-host` crate ✅
- **Question:** where to put the host capability bundle, given its pieces live in crates that do not depend on each other (`rap-client`, `infinity-agent-core`, `sandbox-core`, `infinity-mcp-bridge`).
- **Decision (user):** a new fork-only crate `crates/infinity-host` (`publish = false`) that depends on all four and forwards a default-on `native` feature to `sandbox-core` and `infinity-mcp-bridge`.
- **Rejected:** in `infinity-agent-core` without the sandbox slot (it would restate `AgentSystemBuilder<C, S, M, H>` and still omit sandbox/MCP); in `infinity-agent-core` with a `sandbox-core` dependency (couples the runtime core to the sandbox crate and drags its feature surface into every agent-core consumer).
- **Shape:** a generic bundle `HostCtx<C, S, H, T, E>` with accessors, `with_mcp_transport` and `into_parts`; MCP is `Option<McpTransportFactory>` because not every host exposes MCP servers. It deliberately **excludes** `ThreadConfigSource` — that trait is generic over the system's `M`/`H` and holds `Rc<dyn Tool<M>>`, so it is per-system, not per-host — and the deferred `SliceTimer` / `CallbackIngest` slots (D18).
- **Verified:** 2 tests (capability access, optional MCP + `into_parts`), compiles for `wasm32-unknown-unknown` with `--no-default-features`, clippy clean.

## D21 — Android TLS verifies against embedded Mozilla roots ✅
- **Question:** how should HTTPS work in the Android archetype, given `reqwest` 0.13's `rustls` feature (pulled in by `rig-core`'s `reqwest-rustls`) always uses `rustls-platform-verifier`?
- **Finding (measured on device):** without that crate's Android initialization the handshake **stalls** — no error, no log line, no `AgentEvent::Info`. Two probes timed out at 60 s with `reply: ""` and `notes: []` while the same key/model answered `HTTP 200` from the host in 2 s. The crate needs `init_with_env`/`init_with_runtime` (JNI `Env` + Android `Context`) **and** a Kotlin AAR in the Gradle build; upstream confirms both requirements ([reqwest#2966](https://github.com/seanmonstar/reqwest/issues/2966), [tauri#13267](https://github.com/tauri-apps/tauri/issues/13267)).
- **Decision:** build a `rustls::ClientConfig` from `webpki-roots` compiled into the binary, hand it to `reqwest` via `use_preconfigured_tls`, and inject the client into rig with `.http_client(...)`.
- **Rejected:** (a) shipping the AAR + `init_with_env` — the AAR lives in `gen/android`, which is regenerated and git-ignored, so every consumer would have to redo it; (b) `targetSdkVersion 28` to dodge the verifier — not viable for a current app; (c) plain HTTP — unacceptable for a real key.
- **Consequence:** the platform trust store is bypassed, so user-installed and corporate CAs do not apply. This is the same posture as the browser target, where no OS store exists either. Apps that need private CAs must ship the AAR and initialize the verifier before the first request — recorded in `apps/local-first/android/README.md`.
- **Verified:** `chat.json` → `{"model":"gemma-4-31b","reply":"infinity-on-android-llm","notes":[],"timed_out":false}`, ~10 s, and `cargo check --target aarch64-linux-android` clean.

## D22 — The Android plugin lives under the app, not in `crates/` ✅
- **Question:** `tauri-plugin-infinity-android` was scaffolded into `crates/`; should it stay there?
- **Decision (user):** move it to `apps/local-first/android/crates/tauri-plugin-infinity-android`.
- **Why:** it is an implementation detail of one archetype, not a crate the fork publishes. Inside the app it becomes a member of the app's own workspace, so it shares `target/` and `cargo fmt --all` instead of duplicating a build tree, and the fork's workspace never pulls a desktop `tauri` build (which would need `webkit2gtk` on Linux CI).
- **Mechanics:** the app declares `path = "crates/tauri-plugin-infinity-android"`; the root `exclude` stays `["apps/local-first/android"]`; the generated `gen/android/tauri.settings.gradle` picks the Android library up automatically from the plugin's `build.rs` (`android_path("android")`).
- **Verified:** `cargo metadata` lists both packages in the app workspace; APK rebuilt after the move; all three probes still pass on the device, and the Gradle settings file points at the new path.

## D23 — On-device tools run as Wasm modules, not extracted executables ✅
- **Question:** how do tools execute inside the Android app, given that Android 10+ denies `exec` on files in the app's writable data directory for apps targeting API 29+?
- **Options:** (A) ship executables as `jniLibs/…/lib*.so` and execute them from `nativeLibraryDir` (executable because the directory is labelled `apk_data_file`), or (B) embed a Wasm engine and interpret `.wasm` read from app data.
- **Decision:** B by default; A stays as the escape hatch for binaries that cannot be compiled to Wasm.
- **Measured cost (aarch64-linux-android, release, `llvm-strip`, empty `cdylib`):** 308 KB baseline vs 1.44 MB with `wasmi` 1.x (`Engine` + `Module::new`) — **+1.16 MB**.
- **Rejected as default:** `wasmtime` — the official minimal C API build is 2.1 MB, the Rust crate is larger, and its JIT pages were not validated on this device. `wasmi` is an interpreter: no executable memory, deterministic, and its fuel metering matches the slice model.
- **Consequences:** (1) tools must be distributed as `.wasm`, so tools that need a real OS (git, package managers, preview servers) fall back to option A or to the optional edge; (2) executables shipped via option A still need `patchelf --set-rpath '$ORIGIN'` plus their shared libraries, which is documented in `apps/local-first/android/README.md`; (3) `SandboxExecutor` is the seam that keeps this choice out of the core.

## D24 — WhatsApp Web is shown, never automated ✅
- **Question:** the blueprint promised "WhatsApp control" through an embedded WebView with JS injection. Does that work, and should the fork build it?
- **Verified on device (2026-11-17):** an overlay `WebView` with a desktop user agent loads `https://web.whatsapp.com` to `readyState: complete`, with `canvasCount: 1` (the QR), plus `serviceWorker`, `indexedDB` and `subtleCrypto` all present, and no UA redirect. `screencap` shows the genuine login screen. So the *mechanism* is sound.
- **Not verified, and deliberately so:** a paired session. Linking requires a human to scan the QR with the phone, and nothing after that was built.
- **Legal constraint (primary sources):** WhatsApp's Help Center article "About temporarily banned accounts" names "an unofficial version of WhatsApp" and "scraping" as ban triggers, and the Messaging Guidelines prohibit "use of unofficial clients … or automation". The consequence lands on the user's own number, not the developer's.
- **Decision:** the fork may display WhatsApp Web as a read-only view; it ships no DOM injection, scraping or message automation. The probe is opt-in through a `whatsapp-probe` marker file so the overlay never appears during normal runs.
- **Rejected:** shipping a scraper or auto-responder (ToS violation with account-ban risk); an unofficial API (same violation, and it still needs a QR-paired session); WhatsApp Cloud API (the supported path, but it requires a business account and Meta-hosted infrastructure — incompatible with a cloud-optional local app).

> **Revision (2026-11-17, user decision).** After being shown both primary sources, the user chose **full automation (read + send)** for personal use, accepting that any ban lands on their own number. The view-only boundary above no longer describes the shipped scope; D25 records the automation design and the guardrails that keep it opt-in and local. The factual findings in this entry stand: automation violates WhatsApp's terms, and nothing here makes it safe.

## D25 — WhatsApp automation is opt-in, local and explicitly unofficial ⚠️
- **Context:** D24 verified that the client boots, and the user chose full automation (read + send) knowing the terms violation and ban risk.
- **Design:** the WebView stays the only WhatsApp client; automation is JS injected over `evaluateJavascript` on a timer, with results written to the app data dir and later delivered through RAP callbacks.
- **Guardrails:** off unless `whatsapp-config.json` exists (pushed at runtime, never committed); no credentials or phone numbers in the repo; the pairing code is written locally for the user to type into WhatsApp; no bulk sending and no contact harvesting — the prototype reads only the conversation the user has open.
- **Reality check:** pairing cannot be automated. The phone's WhatsApp must approve the link, so the flow is: enter the number → read the 8-character code → the user types it into WhatsApp → the session persists in IndexedDB.
- **Consequence:** the fork's blueprint must not market this as a supported WhatsApp integration. The supported path for compliant automation remains the official Cloud API (`official_api` option), which is cloud-bound and therefore outside the cloud-optional promise.

## D26 — WhatsApp Web automation: what actually works ⚠️
- **Context:** D25 committed to automation. This records the measured mechanics, so nobody re-derives them.
- **Linking:** the pairing-code flow works (`Link with phone number instead` → number → 8-character code → typed into WhatsApp on the phone). The code renders one character per element, so it must be matched on whitespace-stripped text.
- **The link is fragile in one specific way:** restarting or reinstalling the app mid-link kills the handshake. Measured: a rebuild during the sync produced `Logging out` then `Couldn't link device. Keep WhatsApp open on your phone when your device is linking.` Do verification over CDP (§ below), never by restarting.
- **Session persistence works:** after a completed link, a restart restored the chat list from IndexedDB (21 rows) with no re-pairing.
- **Synthetic clicks do not work.** `element.click()` and dispatched `MouseEvent`s on a chat row do nothing. Trusted input does: CDP `Input.dispatchMouseEvent` opened the conversation, so production uses `WebView.dispatchTouchEvent` with real `MotionEvent`s at `CSS px × displayMetrics.density`.
- **Selectors that actually match:** chat rows `#pane-side [role="row"]` with `data-testid="list-item-N"` and `cell-frame-title`; messages `#main [data-testid^="conv-msg-"]` (`role="row"` inside `#main` matches only empty layout rows); composer `#main div[contenteditable="true"]`; send `button[aria-label="Send"]`; search `[aria-label="Search input textbox"]`; typing via `document.execCommand('insertText', …)`.
- **Rendering is asynchronous:** the message panel appears after the tap, so reads retry until more than one message exists.
- **Detection boundary:** no spoofing or evasion is implemented. WhatsApp's guidelines name unofficial clients, scraping and automation as ban triggers; the mitigations here are rate limits, dry-run-by-default (`whatsapp_send` drafts unless told to send), and stopping on warnings. See D24's revision for the risk the user accepted.
- **Status:** `whatsapp_state` ✅ (chat list, row count); `whatsapp_read` expression ✅ over CDP (23 messages / last 5 returned); the in-app retry path ⏳ unverified; `whatsapp_send` ⏳ implemented, dry-run untested.

## D27 — Browser stores need `?Send` propagation, not just relaxed trait bounds ⏸️
- **Question:** can an IndexedDB-backed `ConversationStore`/`StateStore` be dropped in by relaxing the traits' `Send + Sync` supertraits on wasm (the D15 pattern)?
- **Attempted:** introduced `StoreBounds` (`Send + Sync + Clone` natively, `Clone` on wasm) and made both store traits extend it.
- **Measured result:** the wasm build fails. `event_processor.rs:209` and `system/local/router.rs:338` cannot share `C`/`S` across threads, because the enclosing async functions require `Send` futures; holding `&C` across an `.await` then needs `C: Sync`. Relaxing only the supertraits does not relax the futures.
- **Why:** `idb`/`wasm-bindgen` handles are `!Send`, and their futures are `!Send` too. A `SendWrapper` around the handle does not help — the `JsFuture` itself is `!Send`.
- **Conclusion:** the browser store is blocked on making the core's async paths `?Send` on wasm (cfg-gated `async_trait(?Send)` plus `spawn_local` at the four call sites), which is a Phase 3 refactor, not a trait tweak. Reverted the change; both targets are green again.
- **Options for the refactor:** (a) propagate `?Send` through the core's async fns on wasm; (b) run the browser runtime inside a `LocalSet` and keep `Send` only where genuinely required; (c) `SendWrapper` — rejected, since it cannot fix `!Send` futures.
- **Ordering:** Phase 4 (Cloudflare worker, `infinity-remote-adapters`) does not depend on browser storage and can proceed first.

## D28 — SQLite-Wasm + OPFS is the browser storage target, and it was proven now ✅
- **Question (user direction):** IndexedDB was the cheap default, but SQLite+OPFS is where the consuming browser app is going. Deferring it would have meant designing stores we already know will be replaced.
- **Proven, not assumed:** a dedicated-worker spike (`apps/browser-storage-spike`) with `@sqlite.org/sqlite-wasm` 3.54.1-build1 in headless Chrome. First load inserted 100 rows; a reload read 100 and inserted 100 more — `persistence: PASS (100 → 200)`.
- **Measured cost:** `sqlite3.wasm` is **879,084 B raw / 407,552 B gzip**; the JS API `index.mjs` is another 656,872 B. That is a real addition on top of `wasmi`'s ~1.16 MB, and it is the price of the storage model the app will actually use.
- **Hard constraints:** `createSyncAccessHandle()` is exposed only in a **dedicated worker**; one writer per file, so multi-tab needs a SharedWorker electing an active tab; `navigator.storage.persist()` must be called from the main thread; OPFS is still quota-managed and can be cleared. Browser floor: Chrome 102+, Firefox 111+, Safari 15.2+ (sync handles) / 16.4+ (full OPFS).
- **Consequence:** the browser store will be a `SqliteConversationStore`/`SqliteStateStore` behind the existing traits, not an IndexedDB one. The D27 `?Send` blocker applies to either engine: it is about JS handles and the core's `Send`-bound futures, not about storage choice.
- **Rejected:** IndexedDB as the target engine (cheap, but the app would outgrow it); SQLite on the main thread or in a shared worker (API does not exist there); Asyncify builds (roughly double the wasm for no need here).

## D29 — No Wasm interpreter in the browser; the browser *is* the runtime ✅
- **Question:** does the browser bundle need `wasmi` for the sandbox, as Android does?
- **No.** A browser already hosts WebAssembly. A tool module is instantiated with the platform `WebAssembly` API and isolated by the runtime: separate linear memory, only the imports we pass (no `fetch`, no DOM, no filesystem), and a dedicated Worker so a runaway module is killed with `terminate()` instead of freezing the tab.
- **What the browser engine does not give us:** fuel metering and determinism (NaN canonicalization, threads). The policy is therefore a wall-clock timeout + memory cap, not instruction counting.
- **Consequence:** the +1.16 MB figure measured for `wasmi` on `aarch64` belongs to **Android only** (D23), not to the browser bundle. `wasmi` is still the smallest *memory-safe* option there, but only if tools ship independently of the app.
- **Android ladder (measured or reasoned):** compile tools into the app (0 KB) → native binary from `nativeLibraryDir` (fragile, per ABI) → `wasmi` (+1.16 MB, Rust, fuel metering) → `wasm3`/WAMR MicroRuntime (smaller, C, thinner maintenance). The last is rejected for untrusted bytecode unless APK size becomes a hard constraint.

## D30 — Browser storage engine: `wa-sqlite` sync build ✅ (extends D28)
- **Measured (this repo, 2026-11-17):**

| Package | wasm raw | wasm gzip | JS |
|---|---|---|---|
| `@sqlite.org/sqlite-wasm` 3.54.1 | 879 KB | 398 KB | 641 KB |
| **`wa-sqlite` 1.0.0 (sync)** | **546 KB** | **267 KB** | ~42 KB glue |
| `wa-sqlite` 1.0.0 (Asyncify) | 1.1 MB | — | — |

- **Decision:** use `wa-sqlite`'s **sync** build in a dedicated worker. The Asyncify build only buys access from contexts without sync access handles (main thread/service worker) at roughly double the wasm and a slower instrumented path; we do not need that.
- **VFS choice:** transparency (file export) matters only if the database file is the unit of replication. With an event log (D31) it is not, so the fast `AccessHandlePoolVFS` is acceptable **only with a single-writer guarantee** across tabs; otherwise use `OPFSCoopSyncVFS`/`OPFSWriteAheadVFS`, which allow multiple connections.
- **JSPI** (async suspension without Asyncify) is Chromium-only today; noted as the future replacement, not a baseline.

## D31 — Sync model: append-only event log, not a CRDT ✅
- **Context (user):** an agentic **local-first** app syncing across the user's own devices and sessions — explicitly **not** collaboration.
- **Decision:** sessions are single-writer. Replication is an immutable **event log** per session plus a **per-device cursor**, with a **session lease** so two devices never run the same session at once (the second forks or waits).
- **Why not Yjs:** CRDTs solve concurrent edits of shared state. Here the needs are durability, resume and dedupe; an ordered log gives all three, stays queryable, and ports to D1 unchanged. Yjs remains an option for a future *document* surface, scoped to that surface.
- **Dedupe/idempotency:** writer-assigned UUIDv7 `event_id`; the relay ignores a repeated `(session_id, event_id)`, so retries are free.
- **Ordering (corrected during implementation):** a writer's `seq` is **per device**, so two devices both produce 1, 2, 3 and `seq` cannot order a session across devices. The relay assigns a monotonic `position` per row; **only `position` is comparable globally**, and per-device cursors track it. `seq` remains useful for local tail reads and for a device's own log.
- **Compaction:** a `Snapshot` event carries full state; a new device replays from the latest snapshot instead of the whole history.
- **Stores:** `FileEventStore` (JSONL per session) for desktop/CLI and Android; SQLite in the browser. Both satisfy the same contract; the file store's tests pin the semantics (ordered reads, idempotent append, per-device cursors).

## D32 — Relay on D1; no Durable Objects unless server-push is needed ✅
- **D1 is SQLite**, so local schema and migrations are shared (`sessions`, `events`, `sync_state`), which is the main reason SQLite won over IndexedDB.
- **Single-user topology has no fan-out, presence or broadcast**, so a plain Worker endpoint suffices: `INSERT ... ON CONFLICT DO NOTHING` on push, `SELECT ... WHERE seq > ?` on pull. Durable Objects are earned only by server-initiated notifications or per-document real-time coordination.
- **Write-rate caution:** one row per keystroke is the wrong shape for D1; batch/coalesce events client-side or move hot paths to a DO.
- **Privacy:** event payloads may be encrypted client-side so D1 stores ciphertext; the sync protocol is unchanged.

## D33 — `crates/infinity-sync` placement and scope ✅
- New fork crate holding the wire types, the `EventStore` contract, the file store and per-device cursors.
- **Native-only for now:** a browser store is blocked by D27 (`?Send` propagation), not by storage choice. The crate deliberately excludes HTTP so the transport stays a separate seam (dev relay now, Worker later).

## D34 — Cloudflare deployment and the e2e findings ✅
- **Deployed:** Worker `local-first-sync` → `https://local-first-sync.beznet22.workers.dev`, D1 `local-first-sync` in **WEUR** (`database_id a23fb167-d10c-4100-93cb-c76e09f4380a`). Schema applied with `wrangler d1 execute --remote`; credentials come from `apps/.env` (git-ignored) and were never printed or committed.
- **Propagation:** the first push after `wrangler deploy` returned Cloudflare `404 / error 1042`; the same request succeeded seconds later. Deployments are not instantly routable — retry rather than assume a broken route.
- **Android TLS recurrence (D21):** the sync transport built its own reqwest client, which on Android goes through `rustls-platform-verifier` and **stalls**. Fixed by adding `HttpTransport::with_client` and passing the app's portable (compiled-in roots) client. Any new HTTP path on Android must reuse that client.
- **Android result:** `pushed 1, fetched 3, position 3` against the deployed Worker.
- **Browser result:** wa-sqlite 3.44.0 + `AccessHandlePoolVFS` over OPFS, `before 0 → after 8`, `devices: browser,laptop,android`, and a reload kept 8 rows while fetching only 1 new one (`persistence: PASS (8 → 9)`).
- **wa-sqlite 1.0.0 API facts (all cost a cycle to learn):** `libversion` is a **function** (not a string); VFS modules export **classes**, not `create()` factories (`new AccessHandlePoolVFS(dir)`, then `await vfs.isReady`, then `vfs_register(vfs, true)`); the package ships `AccessHandlePoolVFS` and an async OPFS VFS but **no `OPFSCoopSyncVFS`**; inserts go through `sqlite3.run(db, sql, params)` — the `exec({ bind })` form does not exist.
- **Build script:** the Android build recipe now lives in `scripts/build-apk.sh`, which validates `JAVA_HOME` (a stale `/opt/android-studio/jbr` was overriding a working JDK), sets the NDK linker env (otherwise cargo falls back to `cc`), and `cd`s to its own directory (running it from another cwd made pnpm look for a manifest in the root checkout).
- **One session, three devices:** the same `cloud1` session is readable from the CLI, the phone, and the browser, with each device pushing its own events and pulling the rest.

## D35 — The fork's core runs in a browser, with a real model ✅
- **What ran:** `crates/infinity-wasm` compiled with `wasm-pack --target web`, loaded in headless Chrome. Two entry points:
  - `run_smoke_turn` — full `AgentSystemBuilder` → local driver → thread → `StaticModel`, answered by a deterministic mock provider. Result: `{"ok": true, "reply": "infinity-wasm-turn-ok"}`.
  - `run_openrouter_turn(api_key, model, prompt)` — the same turn through **OpenRouter**, keyed at runtime. Result: `{"ok": true, "mode": "openrouter", "reply": "infinity-browser-openrouter-ok"}`.
- **No tokio runtime:** the local system is `!Send` (D15), so it runs inside a `LocalSet::run_until` awaited directly from a `wasm-bindgen` future. A successful turn never touches `tokio::time`; only the retry paths do, which is why the mock and real turns both work.
- **CORS:** OpenRouter serves the browser directly (verified `200` from the page with `Authorization: Bearer`). Providers that do not send CORS headers cannot be called this way — they need the Worker relay as a proxy. The Kimchi endpoint was not tested from the browser for this reason.
- **`!Send` bridge inside `async_trait`:** `ModelProvider` requires `Send` futures, but reqwest's wasm response is `!Send` (a JS promise). The fetch therefore runs in `wasm_bindgen_futures::spawn_local` and hands its result back through a `tokio::sync::mpsc` channel; the trait method only awaits a `Send` receiver. This is the pattern to reuse until D27's `?Send` propagation lands.
- **Key handling:** injected with CDP `Page.addScriptToEvaluateOnNewDocument`, so it exists only in that tab's memory. Never written to a file, never committed.
- **Bundle (release, after `wasm-pack` + `wasm-opt`):**

| Artifact | raw | gzip |
|---|---|---|
| `infinity_wasm_bg.wasm` (mock only) | 3,593,253 | 891,975 |
| `infinity_wasm_bg.wasm` (+ OpenRouter/reqwest) | **3,919,251** | **1,028,494** |
| extra `wasm-opt -Oz` pass | 3,770,160 | 1,026,769 |
| JS glue | 31,456 | — |
| debug build (for reference) | 9,149,331 | 2,103,705 |

- **Where the bytes go** (pre-bindgen release artifact, 5,820,847 B): `.rodata` 1,267,594 (21.8%), name section 657,334 (11.3%, stripped in the package), `__wasm_bindgen_unstable` 284,189 (4.9%), **Rhai** packages/parser ≈ 190 KB visible plus its `.rodata` share, `chrono-tz` tables 22,781, `infinity_agent_core::run_completion` 27,935, remainder spread over 8,803 items.
- **Next size lever:** Rhai and `chrono-tz` are the only clearly removable contributors. If the browser never evaluates Rhai scripts or formats timezones, gating them behind a feature is worth more than another `-Oz` pass (which buys ~4%).

## D36 — Generic OpenAI-compatible browser provider, and all-platform sizes ✅ / ⏸️
- **Provider:** the hardcoded OpenRouter client became `OpenAiCompatibleProvider(base_url, api_key, model)`; the export is `run_openai_compatible_turn(base_url, api_key, model, prompt)`. `…/chat/completions` is appended to the caller's base URL, so OpenAI (`https://api.openai.com/v1`), OpenRouter (`https://openrouter.ai/api/v1`), Kimchi (`https://llm.kimchi.dev/openai/v1`) and local servers all work. D35's `run_openrouter_turn` is superseded.
- **Verified in the browser:** the mock turn still passes (`infinity-wasm-turn-ok`, mode `mock`).
- **Not yet verified with a real endpoint — three concrete blockers, measured:**
  1. `panicked at std/time/...: time not implemented on this platform` — something on the real-path turn calls `std::time` (likely request timing), which `wasm32-unknown-unknown` does not implement.
  2. `js-sys ... RefCell already borrowed` in `futures/task/singlethread.rs` — a reentrancy panic from polling a JS-backed future inside `spawn_local`; the bridge must not re-enter the same `JsFuture`.
  3. OpenRouter answered **429** for the free model used (`inclusionai/ling-3.2-flash:free`), so the test also needs a non-throttled model.
  These are the browser analogues of N1/N2 in the wasm plan: `std::time` and single-threaded future reentrancy, not provider logic.
- **All-platform sizes (measured, release unless noted):**

| Target | Artifact | Raw | Stripped | gzip |
|---|---|---|---|---|
| CLI (fork agent) | `target/release/infinity` | 26,289,856 | 20,896,328 | 8,617,892 |
| CLI (app sync) | `apps/local-first/.../local-sync` | 7,503,696 | 6,159,688 | 2,757,516 |
| Wasm | `infinity_wasm_bg.wasm` + JS | 3,920,352 + 31,828 | — | 1,029,467 |
| Wasm (debug, reference) | same | 9,149,331 | — | 2,103,705 |
| Android release APK | `app-universal-release-unsigned.apk` | 27,502,271 | — | — |
| Android debug APK | `app-universal-debug.apk` | 523,872,165 | — | — |

- **Android APK composition (release, arm64 only):** native `.so` **25,836,608 B (94%)**, dex 735,410, resources 259,547, other 570,928. The `.so` is stored, not compressed, so the APK is dominated by Rust code. The debug APK is 19× larger because of symbols and all ABIs — never quote it as the app size.
- **`infinity-wasm` is 3.9 MB raw / 1.03 MB gzip**, of which reqwest added +326 KB / +137 KB; Rhai and chrono-tz remain the largest removable contributors (D35).
- **Build script:** `scripts/build-apk.sh [target] [debug|release]`; Tauri treats release as the default, so only `--debug` is a flag.

## D37 — Browser turn works with a real provider; retries must use the platform timer ✅
- **A1 root cause (measured):** the `time not implemented on this platform` panic came from `event_processor`'s **retry path**, not from the request itself. A provider error (429) reached `tokio::time::sleep`, which calls `Instant::now()` and aborts the page. Fixed by routing every retry wait through a cfg-gated `retry_delay`: `tokio::time` natively, `wasmtimer` on `wasm32` (agent-core now depends on wasmtimer for the web target only).
- **A2 was a cascade:** with the time panic fixed, the js-sys `RefCell already borrowed` panic did not recur on the success path. No separate fix was needed; treat it as fallout from the aborted poll.
- **A3 verified, with a real model:** the fork's `run_openai_compatible_turn` produced `{"ok": true, "mode": "provider", "reply": "infinity-browser-openrouter-ok"}` in headless Chrome.
- **CORS reality on this machine:**
  - OpenRouter: browser-callable, but the account has **no credits** (€402 on paid models) and the free model rate-limits (429).
  - Kimchi: **not** browser-callable (`TypeError: Failed to fetch`) — no CORS headers.
  - Cloudflare fronting Kimchi rejects Python's default user agent with **error 1010**; a custom UA is required.
  - Therefore a relay is mandatory for non-CORS providers. Verified today with `apps/local-first/browser/dev-cors-proxy.py`, a local stand-in that injects the key server-side (so it never reaches the page) and adds CORS headers.
- **B1 landed:** `SimpleHttpClient::with_client`, `HttpMcpTransport::with_client`, `McpClient::http_with_client`, and the Android host now assembles its RAP client with the portable TLS client instead of `new()`. This closes the D21/D34 class of "transport built its own client".
- **Measured bundle at this revision:** wasm 3,920,352 B raw / 1,029,467 B gzip + 31,828 B JS.

## D38 — Durable browser storage without the `?Send` refactor: an actor handle ✅
- **Problem:** the store traits require `Send + Sync`, but IndexedDB handles and their futures are `!Send` (D27). Relaxing the traits alone fails because the core's async functions require `Send` futures.
- **Decision:** keep the core unchanged and give the browser a **`Send + Sync` handle**. `IndexedDbStateStore` owns only a `tokio::sync::mpsc::Sender`; the IndexedDB connection lives in a task pinned to the page's `LocalSet`. Each trait method is a request/response round-trip through a `oneshot`, so no `!Send` future is ever held across an await in the caller.
- **Why not `?Send` propagation now:** it is a core-wide refactor; the actor achieves the same capability at a fraction of the risk. The `?Send` work remains valid for stores that must be `!Send` *by type*, but nothing in the browser needs that today.
- **Verified on device (headless Chrome, page reload):** `state_probe()` returned `before=0 after=1`, then after a reload `before=1 after=2` — the state survived the reload. Turns now run with `IndexedDbStateStore` supplying processed ids, metadata, subscriptions and pending choices.
- **idb 0.6 API notes (each cost a compile cycle):** `event.database()` comes from the `DatabaseEvent` **trait**, which must be imported; `ObjectStore::get` resolves to `Option<JsValue>` (absent key); `put` resolves to the key, not the value; `VersionChangeEvent` itself only exposes versions.
- **Remaining:** the same actor for `ConversationStore` (history) — still in-memory in the browser — and optionally a SQLite/OPFS backend behind the same handle. Neither needs the core to change.

## D39 — Durable conversation history, and the IDB versioning trap ✅
- **What:** `IndexedDbConversationStore` implements the full `ConversationStore` trait (root bootstrapping, history slicing, append, spawn bookkeeping, ancestor chain, compaction summaries) behind the same `Send + Sync` actor handle as the state store. Semantics are mirrored method-for-method from `InMemoryConversationStore`, so the core cannot tell them apart.
- **Trap found by testing (first attempt hung):** two stores opened the **same database at the same version 1**. Only the first open fires `on_upgrade_needed`, so the second store's object store was never created — and its transaction failed, leaving the page waiting forever. Fix: one database per store (`infinity-agent-state`, `infinity-agent-history`). Object stores can only be created in an upgrade, so sharing a DB across independent features couples their versions.
- **Verified in headless Chrome across a reload:**
  `state: before=0 after=1`, `history: before=0 after=1`, mock turn ok → reload → `state: 1→2`, `history: 1→2`. Both durabilities **PASS**, and turns now run with both stores backed by IndexedDB.
- **Consequence:** the browser's storage is now the core's own stores, not a JS side path. A SQLite/OPFS backend can be swapped behind the same handle later if query needs appear; nothing in the core changes.
- **Remaining browser work:** the Worker-isolated tool sandbox and the wake path (Web Push + RAP callbacks).

## D40 — Leases and snapshots are events, derived not coordinated ✅
- **Lease design:** `LeaseClaim` events carry `{ owner, action, expires_at_ms }`. `current_lease(events, now)` replays them: later wins, an expired claim means nobody holds it, and an explicit `Release` clears immediately. `may_write(events, device, now)` is the guard. Default TTL is 60 s.
- **Why events:** the relay stays dumb (D31), every device derives the same answer from the same history, and an offline device still knows who owned a session as of its last sync. No coordinator, no heartbeat endpoint.
- **Snapshot design:** `Snapshot { state, covered_events }` plus `replay(events) -> (Option<state>, tail)` — "latest snapshot + everything after it". A fresh device and one that missed three events use the same rule.
- **Tested:** `infinity-sync` now has **12 tests** — claim/renew/expire/release, last-claim-wins, malformed lease payloads, replay with and without snapshots, multi-snapshot tail, payload round-trip.
- **Demonstrated through the CLI** (`local-sync`): `write` → `lease` (`holder: laptop`) → `snapshot` (`covered: 2`) → `write` → `replay` (`has_snapshot: true, tail: 1`).
- **Not yet enforced:** the lease is advisory in the sync layer. Wiring `may_write` into turn start (and renewing it during a long turn) is the remaining step; until then two devices *can* both run a session.

## D41 — The gates are enforceable: optional tooling no longer blocks, CI covers the fork ✅
- **`check.bash` was all-or-nothing:** a missing `jj` (or `bwrap`/`sandbox-exec`) aborted the entire run, so none of the fork's own gates could be exercised on a clean machine. It now treats them as optional, says so, and excludes `sandbox-local`'s tests when they are absent. Everything else runs.
- **Verified:** `./check.bash` exits **0** — formatting, clippy (`-D warnings`), compile, tests, doctests, the wasm gate (9 crates) and the Android gate (8 crates, NDK detected) all pass; Playwright e2e and `cargo-about` are skipped with instructions.
- **New `.github/workflows/fork-ci.yml`** (fork-specific, leaves upstream `ci.yml` alone): wasm gate, Android gate with `setup-ndk` r27, archetype tests plus the Worker's `tsc --noEmit`, and a `wasm-pack` build that prints raw+gzip size.
- **Clippy findings fixed on the way** (they had never been seen because the run aborted before lint):
  - dead `HttpMcpTransport::new` after the client-injection refactor;
  - a compile-time-constant assertion in `infinity-wasm`'s test;
  - `to_string()` on `&str` (several) and `unwrap()` in tests in the archetype;
  - `#[allow]` that the workspace lint requires to be `#[expect(reason = ...)]`.
- **Consequence:** "green" now means the same thing locally and in CI, and a regression in either target fails a real job rather than a hand-run command.

## D42 — The lease is enforceable through one guard object ✅
- **API:** `LeaseGuard::acquire(store, session, device, ttl)` returns `Ok(None)` when another device holds the session — the "do not start a turn" signal — or a guard that can `renew()` on the turn's heartbeat and `release()` when done.
- **Why a guard rather than a check:** it pairs the check with the claim, so there is no window where two devices both pass `may_write` and both start. Release is explicit: dropping the guard cannot release, because releasing is an event and needs an await.
- **Tested (3 new tests, 15 total):** one-device-at-a-time (second acquire refused), release hands over, an expired lease does not block, and renewal keeps the hold while still refusing the other device.
- **Demonstrated through the CLI:**
  `laptop guard` → `{"acquired":true,"holder":"laptop"}`; `phone guard` → `{"acquired":false,"holder":"laptop"}`; `laptop unguard` → released; `phone guard` → `{"acquired":true,"holder":"phone"}`.
- **Remaining:** hosts must actually call it around a turn. The Android chat probe is the natural first consumer (it owns both an event store and a turn); until that wiring lands the guarantee is available but unused.

## Revisit triggers 🔁
- **Pyodide major bump:** wasmsh pins Python 3.13; current Pyodide is 314.0.x (Python 3.14). When picking the sandbox runtime, check wasmsh's Pyodide version vs. current.
- **Background Sync:** re-check caniuse if Chromium-only status changes.
- **wasm-pack org/maintenance:** healthy as of 0.15.1 (2026-10-30) under wasm-bindgen org; rustwasm working group sunset (July 2025) means watch for tooling drift (e.g. `wasm-tools` alternatives).
- **Shizuku primitives:** verify `newProcess`/uiautomator/`am` surface when scaffolding the Android plugin.
- **Trait naming for the fork:** settle final names (`SandboxExecutor` etc.) when creating `crates/infinity-wasm`.
- **tokio wasm support upstream:** #7550 (fs/threads/timer on unknown-wasm) and #8194 (emscripten) are open — re-check before choosing timer/FS replacements for Phase 0 (W5).
- **reqwest wasm `!Send` handling:** resolved (D15 resolution) with cfg-gated traits; revisit if reqwest ships `Send`-compatible wasm futures.
- **Browser timers before Phase 3:** if any browser code path invokes `TokioSleepTool`/`TokioSleepUntilTool`/`SleepUntilEventOrInputTool` before D18 lands, it will panic at runtime — implement `SliceTimer` first.

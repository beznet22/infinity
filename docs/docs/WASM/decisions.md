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
- **Decision:** the rewrite states current reality and makes "move `reqwest` to a rustls feature for Android/WASM builds" an explicit plan item instead of asserting rustls as existing behavior (audit 3.2).

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
- **Correction (user):** `apps/android-tauri` and `apps/cloudflare-worker` are not fork-workspace directories — they are archetypes for any codebase that consumes the fork.
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
- **Numbering stays reserved:** D19 = `HostCtx` (chunk 1.4), D20 = optional edge relay (chunk 4.x).
- **Risk if forgotten:** `TokioSleepTool` / `TokioSleepUntilTool` / `SleepUntilEventOrInputTool` would panic if invoked in a browser — tracked as W5 in `PROGRESS.md` §3 and in the revisit triggers below.

## Revisit triggers 🔁
- **Pyodide major bump:** wasmsh pins Python 3.13; current Pyodide is 314.0.x (Python 3.14). When picking the sandbox runtime, check wasmsh's Pyodide version vs. current.
- **Background Sync:** re-check caniuse if Chromium-only status changes.
- **wasm-pack org/maintenance:** healthy as of 0.15.1 (2026-10-30) under wasm-bindgen org; rustwasm working group sunset (July 2025) means watch for tooling drift (e.g. `wasm-tools` alternatives).
- **Shizuku primitives:** verify `newProcess`/uiautomator/`am` surface when scaffolding the Android plugin.
- **Trait naming for the fork:** settle final names (`SandboxExecutor` etc.) when creating `crates/infinity-wasm`.
- **tokio wasm support upstream:** #7550 (fs/threads/timer on unknown-wasm) and #8194 (emscripten) are open — re-check before choosing timer/FS replacements for Phase 0 (W5).
- **reqwest wasm `!Send` handling:** resolved (D15 resolution) with cfg-gated traits; revisit if reqwest ships `Send`-compatible wasm futures.
- **Browser timers before Phase 3:** if any browser code path invokes `TokioSleepTool`/`TokioSleepUntilTool`/`SleepUntilEventOrInputTool` before D18 lands, it will panic at runtime — implement `SliceTimer` first.

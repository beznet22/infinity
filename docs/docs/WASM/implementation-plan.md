# Implementation Plan — Infinity Hybrid Fork (Phases 0–4)

Chunk-by-chunk implementation spec for the fork described in [`overview.md`](./overview.md), respecting decisions D1–D17 ([`decisions.md`](./decisions.md)) and the compiler-verified readiness findings (overview §9, W1–W10). Status lives in [`PROGRESS.md`](./PROGRESS.md); each chunk updates it on completion.

**Ground rules (from the decisions log):**
- Feature-gate per target; keep one core (D14). The `infinity-provider-protocol` optional-feature design is the template; `cfg(unix)` nix gating is the in-repo precedent.
- `apps/` are consumer-side archetypes, not workspace members (D16) — they double as integration tests for the crate APIs.
- Cloud is optional: everything through Phase 3 works with zero cloud dependency (D17).
- Hosts bring their own HTTP via the `HttpClient`/`CallbackClient` seams (D15); LLM streaming stays on the provider seam, never `HttpClient`.
- Toolchain per overview §8 / D9–D10: wasm-pack 0.15.1, trunk, wabt, Twiggy (⚠ archived upstream), panic hook, DWARF, wasm-bindgen-test, chrome-devtools-mcp debug loop.
- Trait names per D4: `SandboxExecutor`, `AgentCoordinator`, `SliceRunner` are **defined for the first time in Phase 1** — until then they are planned names, never asserted as existing.

**Decisions promoted by this plan** (from overview §10; recorded here now, echoed into `decisions.md` when each chunk starts — numbering follows the order the work begins):
- **D18 — `SliceTimer` trait promotion** (§10.2 idea; starts in chunk 0.5): generalize `WakeupScheduler`/`SleepTool` (`crates/infinity-agent-lambda/src/tools/sleep.rs:46-115`) into a core timer capability with tokio / wasmtimer / DO-alarm impls.
- **D19 — `HostCtx` capability bundle** (§10.2 idea; starts in chunk 1.4): one assembly point where a host provides `HttpClient`, `ToolsetCache`, stores, `SliceTimer`, config source, sandbox executor, MCP transport factory, `CallbackIngest`.
- **D20 — Optional edge relay** (§10.1 idea; starts in chunk 4.3): browser callback relay on DO hibernation WS; the non-cloud alternative is the user's own `infinity-daemon` (D17).

**Deferred beyond Phase 4** (backlog, not forgotten): cross-host session migration (§10.4), full capability-permission matrix (§10.3 — Phase 3 ships minimal fetch-allowlist enforcement only), offline-first `DeferQueue` outbox (§10.1 third row).

---

## Chunk 0 — Phase 0: WASM Readiness

**Goal.** `cargo check --target wasm32-unknown-unknown` passes for the wasm foundation set: `infinity-provider-protocol` (already green), `rap-protocol`, `infinity-protocol`, `rap-client`, `infinity-agent-core`, `sandbox-core`, `infinity-mcp-bridge` — while host builds and tests stay green. Implements W1–W10 (overview §9.2) in dependency order.

**Preconditions.** `rustup target add wasm32-unknown-unknown`; work in the `../infinity-hybrid` worktree (overview §1).

### 0.1 Build configuration foundation (W4, W10, CI gate)
- **Files:** new `.cargo/config.toml`; workspace `Cargo.toml`; `check.bash`.
- **Work:** `[target.wasm32-unknown-unknown]` rustflags `--cfg getrandom_backend="wasm_js"` (getrandom 0.3 documented requirement — fixes uuid v4 RNG); enable uuid's `js` feature on wasm; add a `check.bash` step running the per-crate wasm check for the core set (fails the suite on regression).
- **Note:** `check.bash` already aborts at the `jj` prerequisite in this dev environment (pre-existing, unrelated); the gate is added regardless and verified by invoking the wasm step directly.
- **Accept:** gate executes and is red for exactly the expected crates; no source changes yet.

### 0.2 `rap-protocol` — resolve the `!Send` clash (W2)
- **Files:** `crates/rap-protocol` (client traits + wasm-gated adapter module), `Cargo.toml`.
- **Root cause (verified):** 2× `E0277` — reqwest-on-wasm fetch futures are `!Send` against `Send`-bounded client traits (`rap-protocol/src/lib.rs:241-243`).
- **Work (as implemented):** **cfg-gated trait definitions** — native keeps `#[async_trait]` + `Send + Sync`, wasm uses `#[async_trait(?Send)]` without them; request bodies in shared inherent methods. A `SendWrapper` adapter was rejected: the `!Send` value lives inside reqwest's future, so wrapping cannot make it `Send` without `unsafe`, and unconditional `?Send` breaks the axum servers that require `Send` handlers (D15 resolution, 2026-11-17).
- **Also:** keep `default-tls` native; the rustls switch for Android/WASM lands with the consuming app config (D11, chunk 2.1).
- **Accept:** `cargo check --target wasm32-unknown-unknown -p rap-protocol` green; native tests green.

### 0.3 `rap-client` — gate `net` + callback server; define the ingestion seam (W1, W3)
- **Files:** `crates/rap-client/Cargo.toml`, `src/callback_server.rs` (gated), new ingestion seam.
- **Work (as implemented):** tokio `net` + `hyper`/`hyper-util`/`http-body-util` moved into `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`; `callback_server` module `#[cfg]`-gated. The `CallbackIngest` seam is **deferred to chunk 1.4**, where `HostCtx` actually consumes it — introducing it here without a consumer would add an unused public abstraction. Ingestion impls: (a) local loopback server (native, exists), (b) in-page delivery (chunk 3.3), (c) relay push (D20).
- **Accept:** crate green on wasm; native callback behavior unchanged (host tests).

### 0.4 `infinity-protocol` — config-path injection (W9) *(reclassified: correctness, not a compile blocker)*

- **Finding (2026-11-17):** the crate already compiles for wasm — `dirs` supports the target; `home_dir()` simply resolves to none at runtime. Injection therefore belongs with the browser config wiring (chunk 3.1), not Phase 0.
- **Files:** `crates/infinity-protocol/src/lib.rs` (`dirs::home_dir()` at 194-217), `Cargo.toml`.
- **Work:** injected home/config path; `dirs` moves behind a native-gated helper.
- **Accept:** crate green on wasm; daemon/CLI unaffected.

### 0.5 `infinity-agent-core` — timer capability + no direct fs (W5, W6, **D18**)
- **Files:** `src/traits.rs` or new `slice_timer` module (trait), `tools/sleep.rs`, `system/local/`, `event_processor.rs`, `tools/config.rs`.
- **Work:** define `SliceTimer` (sleep-until / now) with a tokio impl (native) and a wasmtimer impl stub (wasm, validated in 3.1); route `tools/sleep.rs:100`, `system/local/handle.rs:291`, `event_processor.rs:5065` through it or behind native gates; gate `system/local/` native wholesale (desktop-resident machinery per its module docs); remove `std::fs::read_to_string` (`tools/config.rs:125`) — config arrives via `ThreadConfigSource`, with a native file-based impl replacing the old read.
- **Accept:** crate green on wasm; `cargo test --all-targets` green on host.

### 0.6 `sandbox-core` — trait/impl split (W7)
- **Files:** `Cargo.toml` (default-on `native` feature), `sandbox.rs` (core type), `server.rs`, `git.rs`, `jj.rs`.
- **Work:** native-only: git/jj/rg spawning, `which`, `tempfile`, tokio-fs server surface, `tokio::process::Child` in the core type (`sandbox.rs:11`). Portable: `ModeProvider`/`SandboxBackend`/`MetadataStore` + pure types. Keep existing `[target.'cfg(unix)']` nix gating.
- **Accept:** crate green on wasm with `default-features = false`; host builds unchanged.

### 0.7 `infinity-mcp-bridge` — gate stdio transport (W8)
- **Files:** `src/lib.rs` (`StdioMcpTransport`, 24-94), `Cargo.toml`.
- **Work:** `#[cfg(not(target_arch = "wasm32"))]` on the stdio transport + `tokio/process`; trait stays portable.
- **Accept:** crate green on wasm.

### 0.8 Sweep, record, exit
- **Work:** re-run the per-crate sweep (all 7 green); tick `PROGRESS.md` §3; update overview §9.1 statuses.
- **Phase 0 exit criteria:** all 7 core-adjacent crates green on `cargo check --target wasm32-unknown-unknown`; `check.bash` wasm gate in place; host `cargo test --all-targets` green.
- **Open questions:** uuid↔getrandom feature wiring (verify first in 0.1); whether any daemon path requires `Send` futures (0.2 — decides wrapper vs `?Send`).

---

## Chunk 1 — Phase 1: Core Adapter Traits

**Goal.** Define the fork's four adapter roles as real traits (D4) plus the `HostCtx` assembly point (D19), so Phases 2–4 implement rather than invent.

### 1.1 `SandboxExecutor` (`sandbox-core`)
- Trait over "run shell/code, read/write virtual FS", grounded on `ModeProvider`/`SandboxBackend`/`MetadataStore` (`sandbox.rs:57,155`, `metadata.rs:10`); async, `?Send`-compatible for wasm consumers.
- Impl now: thin adapter over existing local/remote routing (native). wasmsh impl → 3.2; Sandbox SDK impl → 4.1.
- **Accept:** trait + native adapter + mock tests.

### 1.2 `SliceRunner` (`infinity-agent-core`)
- "Wake on event → run bounded slice → hibernate", extracted from the step-mode flow (`builder.rs:399-424`) and the Lambda per-invocation pattern (`infinity-agent-lambda/src/main.rs`, `bin/rap-receiver.rs`). Lambda refactored onto it, behavior-preserving.
- **Accept:** trait defined; Lambda refactored; all Lambda tests green.

### 1.3 `AgentCoordinator` (`infinity-agent-core`)
- "Thread registry, routing, persistence", grounded on `infinity-daemon/src/session/mod.rs:11-17` + `PersistentConversationStore`/`PersistentStateStore`. Daemon refactored onto it, behavior-preserving. SharedWorker (3.3) and DO (4.1) become future impls.
- **Accept:** trait defined; daemon refactored; daemon tests green.

### 1.4 `HostCtx` capability bundle (**D19**)
- One struct/trait assembled by the host: `HttpClient`, `ToolsetCache`, `ConversationStore`+`StateStore`, `SliceTimer`, `ThreadConfigSource`, `SandboxExecutor`, MCP transport factory, `CallbackIngest`. Generic where the pieces are already injected (`system/config.rs:38-40` pattern).
- **Accept:** compiles on host + wasm32; daemon/lambda assemble it natively; a full mock bundle unit test exists.

### 1.5 `McpTransport` HTTP/SSE impl (native, `infinity-mcp-bridge`)
- Streamable-HTTP/SSE transport behind a feature; `request(method, params)` contract unchanged; stdio path untouched.
- **Accept:** integration test against a stub MCP HTTP server.
- **Phase 1 exit criteria:** traits compile on host + wasm32; daemon/lambda behavior-preserving (full suite green); PROGRESS updated.

---

## Chunk 2 — Phase 2: Consumer Archetype — Android Tauri App (consumer-side, D16)

**Goal.** A reference consuming app (`apps/android-tauri`; standalone project referencing the fork crates by git/path) proving the crate APIs end-to-end on Android, including long-running operation with **no cloud** (D17).

### 2.1 Scaffold + host wiring
- `tauri init` + `tauri android init`; `tauri-plugin-background-service` (foreground service, `START_STICKY`); `reqwest` switched to rustls features (D11); `INTERNET` permission; assemble `HostCtx` natively (JSON-file stores in the app data dir → `memory_store.rs` pattern; tokio current-thread in the service loop; file-based `ThreadConfigSource`).
- **Accept:** `tauri android build` succeeds; runtime boots inside the foreground service.

### 2.2 End-to-end agent on device
- Webview prompt → slice → provider stream over rustls (rig: OpenAI/Anthropic/Gemini) → `ThreadObserver` events → Tauri events; app binds the loopback RAP callback server for hibernating async tools.
- **Accept:** full conversation on device; survives backgrounding (FGS) and process kill (store persistence + sticky restart).

### 2.3 Kotlin plugins (D12 implementation-time checks)
- Notifications (`NotificationListenerService`) + Contacts (`ContactsContract`) plugins with `@Command` methods, events pushed back as RAP `subscription_event`; Shizuku shell plugin via Binder IPC — **verify** `newProcess`/uiautomator/`am` against [Shizuku-API](https://github.com/RikkaApps/Shizuku-API); document ADB-permission and Android 9 hidden-API limits.
- **Accept:** tools callable from an agent thread; stdout/stderr stream back.

### 2.4 WhatsApp WebView prototype (highest-risk row — validate early)
- Embedded WebView + `WebViewClient` + JS injection; Rust tool sends DOM/action commands over Tauri IPC and awaits scraped state.
- **Accept:** minimal DOM read/write round-trip — **explicitly allowed to fail**; either outcome (feasible / needs a different mechanism) is recorded as a decision note (D12 anticipates this).
- **Phase 2 exit criteria:** archetype builds and runs end-to-end on a device; crate-API gaps discovered are filed into Phase 3 scope.

---

## Chunk 3 — Phase 3: Browser Runtime (`crates/infinity-wasm`)

**Goal.** The full agent runtime in the browser with **zero backend** (D17): conversation, local sandboxed tools, persistence, cross-tab coordination — developed with the overview §8 toolchain.

### 3.1 Crate scaffold + wasm `HostCtx`
- New `crates/infinity-wasm` (wasm-pack, `--target web`/`bundler`); `console_error_panic_hook`; current-thread runtime; fetch-based `HttpClient` (0.2 adapter lives here), IndexedDB `ConversationStore`/`StateStore` + `ToolsetCache`, OPFS file area, wasmtimer `SliceTimer` impl (completes D18's wasm leg).
- **Accept:** `wasm-pack test --headless chrome` green for stores/timer/HTTP against recorded fixtures (mock pattern, overview §10.2).

### 3.2 Sandbox adapter (W7 positive half)
- `WasmSandboxExecutor` over **wasmsh** (overview §5 primary candidate) in a dedicated Web Worker, OPFS-backed VFS, network via wasmsh's allowlist broker (minimal capability enforcement).
- Check at implementation time: wasmsh's Pyodide version vs current (revisit trigger); fallback = `@bsull/conch` (bash-only workloads).
- **Accept:** agent-executed bash/python pipelines run in-Worker with budgets enforced.

### 3.3 Coordinator + eventing (completes W3)
- SharedWorker-hosted `AgentCoordinator` (IndexedDB) + `BroadcastChannel` observer fan-out (tabs are pure renderers, overview §10.4); in-page `CallbackIngest` impl — same-page results resolve directly; async/relay results arrive via WS/polling from the optional relay (4.3) or the user's own daemon (D17).
- **Accept:** two tabs share one coordinator; thread survives reload (hibernate → resume); an async tool completes across a reload.

### 3.4 Dev loop + quality gates
- `trunk serve` app with BYOK provider config (provider CORS headers, overview §2.5); chrome-devtools-mcp loop wired (D10); `twiggy top` size budget in CI; DWARF debug profile for dev builds.
- **Accept:** agent-driven debug loop demonstrated (build → load → console → fix → reload); bundle within the recorded size budget.
- **Phase 3 exit criteria:** full conversation in-browser, zero backend, tools in the local sandbox, persistence + multi-tab verified.

---

## Chunk 4 — Phase 4: Consumer Archetype — Cloudflare Worker + Optional Relay

**Goal.** The optional accelerator (D17): edge slices for heavy/very-long work, plus the optional browser relay (D20). Consumer-side per D16, built on `crates/infinity-remote-adapters`.

### 4.1 `crates/infinity-remote-adapters` (fork crate)
- Edge `SliceRunner` on Durable Objects + Agents SDK (SQLite-backed stores as the DSQL/Dynamo analog; DO alarms as the edge `SliceTimer` impl, D18); RAP callback router with the hibernation WebSocket API; `SandboxExecutor` edge impl over Sandbox SDK/Containers; MCP relay via `MCPClientManager`.
- **Accept:** crate compiles for the Workers wasm target; unit tests with worker-mock env.

### 4.2 Worker archetype (`apps/cloudflare-worker`, consumer-side)
- Wrangler config, DO classes/migrations (`new_sqlite_classes`), routes; consumes 4.1; integration tests: deploy to preview, run a slice, assert callback delivery.
- **Accept:** `wrangler deploy` succeeds; an offloaded slice runs in a container and returns results.

### 4.3 Optional browser callback relay (**D20**)
- Relay exposes the RAP callback URL for browser threads; pushes `tool_result`/`subscription_event` via hibernation WS (or Web Push when absent). Browser side plugs into the 3.3 ingestion seam; the self-hosted `infinity-daemon` is the documented non-cloud alternative.
- **Accept:** browser thread receives an async tool result via relay; docs state the non-cloud alternative.
- **Phase 4 exit criteria:** deployable worker archetype; browser↔edge offload round-trip demonstrated; **cloud-optional regression check** — with the worker undeployed, all Phase 0–3 flows still work.

---

## Verification Strategy (cross-chunk)

1. **Per-chunk gates:** each chunk lists accept criteria; PROGRESS.md is updated in the same change that completes them.
2. **Regression guards:** host `cargo test --all-targets` stays green after every Phase 0/1 change; the wasm sweep re-runs at each Phase 0 sub-chunk; feature preservation (audit Appendix A) re-checked after any doc change.
3. **CI gate:** `check.bash` wasm32 step (0.1) prevents silent regressions from Phase 0 onward.
4. **Debug loops:** chrome-devtools-mcp (D10) in Phase 3; `chrome://inspect` in Phase 2; `wrangler dev` + tail in Phase 4.
5. **Cloud-optional regression check** at Phase 4 exit and after any later change: worker undeployed ⇒ Phases 0–3 unaffected (D17).

## Risks

| Risk | Chunk | Mitigation |
|---|---|---|
| uuid↔getrandom `wasm_js` wiring differs from expectation | 0.1 | verify first — cheapest item, unblocks uuid consumers |
| `Send`-bounded futures required by daemon code conflict with the wasm HTTP adapter | 0.2 | wrapper-first; `?Send` fallback; decision note required before closing |
| sandbox-core split churn (`server.rs` is large) | 0.6 | feature-gate in place, no file moves; native default keeps host builds untouched |
| Lambda/daemon refactors onto traits regress behavior | 1.2/1.3 | behavior-preserving refactor; existing suites are the safety net; small commits |
| WhatsApp WebView mechanism infeasible | 2.4 | explicitly allowed to fail; outcome recorded as a decision note (D12) |
| wasmsh Pyodide drift / size-budget overrun | 3.2/3.4 | conch fallback for bash-only; budget recorded and enforced in CI |
| Workers platform churn (beta Containers, Agents SDK) | 4.x | pin SDK versions; archetype scope keeps fork crates thin |

## Open Questions (implementation-time, non-blocking)

1. Exact uuid `js` ↔ getrandom `wasm_js` feature combination (first step of 0.1).
2. Final `Send` strategy for the wasm HTTP adapter (0.2) — wrapper vs bound relaxation.
3. wasmsh Pyodide version vs 314.0.x at integration time (3.2).
4. Bundle-size budget number (set from the first `twiggy top` baseline in 3.4).

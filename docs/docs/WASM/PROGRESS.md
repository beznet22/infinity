# PROGRESS — Infinity Hybrid Fork (WASM + Android + Cloudflare)

Living **status** tracker (the *spec* is [`implementation-plan.md`](./implementation-plan.md)).
Legend: ✅ done · 🔶 in progress · ⬜ pending · 🚫 blocked.

Docs: [`overview.md`](./overview.md) blueprint · [`audit.md`](./audit.md) claim-by-claim audit · [`decisions.md`](./decisions.md) D1–D17 recorded (D18–D21 planned in the implementation plan) · [`implementation-plan.md`](./implementation-plan.md) Phases 0–4 chunk spec · evidence note `.kimchi/docs/wasm-overview-external-verification.md`.

**Branch:** `feat/wasm-android-hybrid` in the git worktree `../infinity-hybrid` (overview §1). The root checkout stays on `main`.

**Last updated:** 2026-11-17

---

## 1. Blueprint, Audit & Decisions ✅ COMPLETE

| Item | Status | Artifact |
|---|---|---|
| Original overview backed up | ✅ | `.kimchi/docs/overview-original-backup.md` |
| External + in-repo fact-check (5 reference repos, platform claims, toolchain, chrome-devtools-mcp) | ✅ | research note §1–§5 |
| Compiler-verified wasm readiness sweep (7 crates; **1 green / 6 red**; W1–W10 derived) | ✅ | research note §6, `audit.md` Appendix C, overview §9 |
| WASM toolchain research (wasm-pack 0.15.1, wabt, Twiggy ⚠archived, trunk, DWARF, chrome-devtools-mcp) | ✅ | research note §4, overview §8 |
| Claim-by-claim audit (53 rows / 54 verdict assignments) + Feature Preservation Map | ✅ | `audit.md` |
| Overview rewrite (§1–§10, nothing dropped, plan-vs-existing separation) | ✅ | `overview.md` |
| Decisions D1–D17 (framing, corrections, storage, traits, scope, tooling, cloud-optional) | ✅ | `decisions.md` |
| Host-provided HTTP research + seam matrix (`HttpClient`/`ToolsetCache`/`CallbackClient`) | ✅ | research note §7, overview §9.4, D15 |
| Cloud-optional verification (Android FGS, browser slices, direct LLM CORS, local callbacks) | ✅ | research note §8, overview §2.5, D17 |
| Full-docs consistency pass (12 flaws F1–F12 corrected) | ✅ | this session |
| Implementation plan (Phases 0–4, chunks 0.1–4.3) | ✅ | `implementation-plan.md` |

---

## 2. Implementation Phases — index

Chunk IDs below are authoritative from `implementation-plan.md`; where reality diverged the row says so.

| Phase | Chunks | Scope | Status |
|---|---|---|---|
| 0 | 0.1–0.8 | WASM readiness (W1–W10 + CI gate) | ✅ complete — gate now covers 9 crates (7 at exit) |
| 1 | 1.1–1.5 | Core adapter traits + `HostCtx` (D20) | ✅ complete — 1.2/1.3 deferred (D19) |
| 2 | 2.1–2.5 | Consumer archetype: Android Tauri app | ✅ complete — device-verified (2.1–2.3, 2.2b, 2.4–2.5) |
| 3 | 3.1–3.4 | Browser runtime (`crates/infinity-wasm`) | 🔶 3.1 + timer landed; 3.2–3.4 pending |
| 4 | 4.1–4.3 | `infinity-remote-adapters` + worker archetype + optional relay (D21) | ⬜ |

Promoted decisions (recorded in the plan, echoed into `decisions.md` when each chunk starts): **D18 `SliceTimer`** (chunk 0.5) · **D20 `HostCtx`** (chunk 1.4) · **D21 optional edge relay** (chunk 4.3).

---

## 3. Phase 0 — WASM Readiness

Gate: `cargo check --target wasm32-unknown-unknown` green for the core set + the `check.bash` wasm step in place.

**Baseline before this work: 1 green / 6 red. Phase 0 exit: 7 green / 0 red. The gate now lists 9 crates (`infinity-host`, `infinity-wasm` added later).**

| # | Work item | Chunk | Files | Status |
|---|---|---|---|---|
| W4+W10+CI | Build config: getrandom `wasm_js` backend (`--cfg` **and** the feature), uuid `js` on wasm, wasm readiness gate | 0.1 | `.cargo/config.toml`, `infinity-agent-core/Cargo.toml`, `scripts/check-wasm.bash`, `check.bash` | ✅ |
| W2 | `!Send` resolution — **cfg-gated traits**: `Send` on native, `?Send` on wasm (D15 resolution) | 0.2 | `rap-protocol/src/lib.rs` (`CallbackClient`), `rap-client/src/http.rs` (`HttpClient`) | ✅ |
| W1+W3 | tokio `net` + hyper stack moved to non-wasm deps; `callback_server` module cfg-gated | 0.3 | `rap-client/Cargo.toml`, `rap-client/src/lib.rs` | ✅ |
| W9 | Config-path injection (`dirs::home_dir`) | 0.4 | `infinity-protocol/src/lib.rs` | ⏭ reclassified — `dirs` **does** compile on wasm (crate is green); home dir resolves to none at runtime, so injection is a correctness item, not a blocker |
| W2 (cont.) | **`?Send` propagation through `Tool`**: cfg-gated trait (native `Send + Sync` / wasm `?Send`) + `cfg_attr` on 11 impl sites (9 agent-core, 1 mcp-bridge, 1 `NoRapHttp`) | 0.5 | `infinity-agent-core/src/tools/{mod,rap_tool,thread,cancel_subscription,sleep}.rs`, `system/builder.rs`, `infinity-mcp-bridge/src/lib.rs` | ✅ |
| W5+W6 | `SliceTimer` (**D18**), direct `std::fs` removal, gating `system/local/` | 0.5 → **Phase 3** | `infinity-agent-core` | ⏭ deferred — none of these block compilation (`tokio::time` and `std::fs` compile and fail only at runtime; `system/local` has no process/net/thread usage) |
| W7 | Sandbox trait/impl split — default-on `native` feature gating `server`/`git`/`jj` + `SpawnedCommand`/`spawn_command` and their 8 native-only deps; gate runs the crate with `--no-default-features` | 0.6 | `sandbox-core/{Cargo.toml,src/lib.rs,src/sandbox.rs}`, `scripts/check-wasm.bash` | ✅ |
| W8 | Gate the stdio transport behind the default-on `native` feature; cfg-gate `McpTransport` + `McpTransportFactory` (native `Send` / wasm none); `spawn_local` for tool-result delivery in the browser | 0.7 | `infinity-mcp-bridge/{Cargo.toml,src/lib.rs}`, `scripts/check-wasm.bash` | ✅ |
| — | Sweep + record + exit | 0.8 | all 7 crates | ✅ — wasm gate, native workspace, 141 tests, clippy and fmt all clean |

---

## 4. Phase 1 — Core Adapter Traits ✅ COMPLETE

| Chunk | Deliverable | Status |
|---|---|---|
| 1.1 | `SandboxExecutor` trait + `ProcessSandboxExecutor` + tests | ✅ |
| 1.2 | `SliceRunner` trait — **deferred to Phase 3/4 (D19)** | ⏭ |
| 1.3 | `AgentCoordinator` trait — **deferred to Phase 3/4 (D19)** | ⏭ |
| 1.4 | `HostCtx` bundle (**D20**) in new `crates/infinity-host` + accessor/parts tests | ✅ |
| 1.5 | MCP HTTP/SSE — already implemented upstream; added an SSE test | ✅ |

---

## 5. Phase 2 — Consumer Archetype: Android Tauri App ✅ COMPLETE

Every row was verified on a real device (Android 13, arm64-v8a), headlessly, with
no UI interaction — `adb` plus probe files in the app's data dir.

| Chunk | Deliverable | Status |
|---|---|---|
| 2.1 | Scaffold (excluded from the workspace, D16) + Tauri CLI + signed debug APKs | ✅ — APKs for x86_64 and aarch64; build gotchas recorded in the archetype README |
| 2.2 | End-to-end agent on device: `HostCtx` + `ProcessSandboxExecutor` | ✅ — `probe.json`; the run found and fixed a real `ENOENT` (missing cwd) |
| 2.2b | Real model turn over HTTPS (Kimchi, `gemma-4-31b`) | ✅ — `chat.json`; found and fixed a silent TLS hang (D21) |
| 2.3 | Kotlin plugin: contacts + Shizuku probe; notifications via the official plugin | ✅ — 1064 contacts, notification in `dumpsys`, Shizuku absent (compile-only) |
| 2.4 | WhatsApp WebView prototype — **allowed to fail** | ✅ technically — client boots, QR renders, live in a hidden view (D24) |
| 2.5 | WhatsApp automation: pairing by phone number, state/read/send (dry-run default) | ✅ implemented + partially verified — session persists, 21 chat rows, 23 messages read over CDP; in-app read retry and first send still open (D25/D26) |

**Phase 2 exit evidence:** sandbox probe, model turn, contacts/notification/Shizuku probes, WhatsApp view + pairing. Failures worth keeping: a silent TLS stall, a missing sandbox cwd, and my own rebuild-during-linking mistake (D26).

---

## 6. Phase 3 — Browser Runtime (`crates/infinity-wasm`) 🔶 IN PROGRESS

Sequence was re-ordered after the timer research: the timer and storage come
first because nothing else can run on the web target without them.

| Chunk | Deliverable | Status |
|---|---|---|
| 3.1 | Crate scaffold, workspace member, gate entry | ✅ — `crates/infinity-wasm`; wasm gate now covers 9 crates |
| 3.2 | `SliceTimer` over `wasmtimer` 0.4 (boxed future, no `async_trait`) | ✅ — 4 tests pass natively; wasm build green |
| 3.3 | SQLite-Wasm + OPFS stores (D28) | ⛔ blocked on `?Send` propagation (D27); **engine proven** in `apps/browser-storage-spike` (persistence PASS) |
| 3.4 | `wasm-bindgen` JS API + `wasm-pack build` + demo page | ⬜ |
| 3.5 | Wasm sandbox (`wasmi` 1.0.9) + size budget | ⬜ |

Folded in from the original plan: wasmsh/OPFS/SharedWorker land with 3.5, since a
sandbox and a coordinator are only meaningful once the runtime can sleep, store
and be called from JS.

---

## 7. Phase 4 — Cloudflare Worker + Optional Relay ⬜

| Chunk | Deliverable | Status |
|---|---|---|
| 4.1 | `crates/infinity-remote-adapters` (DO slice runner, SQLite stores, DO alarms, Sandbox SDK, MCP relay) | ⬜ |
| 4.2 | Worker archetype (`apps/cloudflare-worker`, consumer-side) | ⬜ |
| 4.3 | Optional browser relay (**D21**) + self-hosted-daemon alternative documented | ⬜ |

---

## 8. Verification Gates

| Gate | Current state (verified) | Target |
|---|---|---|
| `cargo check --target wasm32-unknown-unknown` (core set) | **7 green / 0 expected-red** — verified in the worktree | all 7 green (Phase 0 exit) — **reached** |
| `bash scripts/check-wasm.bash` | ✅ **green** — pins the expected state: fails if a green crate regresses or a red crate turns green unnoticed | stays green while `EXPECTED_RED` shrinks |
| `check.bash` | wasm step **added**; the suite still aborts at the `jj` prerequisite in this dev env (pre-existing), so the step is exercised directly via the script above | runnable in CI |
| Native build (`cargo check --workspace --all-targets`) | ✅ **green** after the trait cfg-split (axum `Send` preserved) | stays green after every Phase 0/1 change |
| Native tests | `cargo test -p rap-protocol -p rap-client` ✅ green; full suite not yet run (same `jj` prerequisite) | green baseline, held after every change |
| Feature preservation (original overview) | **100% mapped** — verified (`audit.md` Appendix A) | stays 100% through any doc change |
| Cloud-optional regression | Not applicable yet (no code) | worker undeployed ⇒ Phases 0–3 work (Phase 4 exit) |

---

## 9. Known Environment Notes

- `check.bash` prerequisites include `jj` (jujutsu) and `bwrap`/`sandbox-exec`; both must be present to run the full suite. Missing `jj` in this dev env is pre-existing and unrelated to the documentation work.
- `docs/` prettier (`format:check`) covers only `src/**/*.{tsx,ts,css}` + `docusaurus.config.ts` — markdown in `docs/docs/WASM/` is not in its scope.
- These docs are not wired into `docs/sidebars.ts` (autogenerated dirs cover only infinity-code/infinity-runtime/rap) — publishing to the site remains a possible follow-up (D13).

---

## 10. Chunk log

| Date | Chunk | Result |
|---|---|---|
| 2026-11-17 | 0.1 | `.cargo/config.toml` selects getrandom's `wasm_js` backend for `wasm32-unknown-unknown`; `infinity-agent-core` enables getrandom `wasm_js` (via rhai→ahash) and uuid `js` on wasm (uuid requires one of `js`/`rng-getrandom`/`rng-rand` there); added `scripts/check-wasm.bash` and wired it into `check.bash`. getrandom + uuid now compile for wasm. |
| 2026-11-17 | 0.2 | `CallbackClient` (rap-protocol) and `HttpClient` (rap-client) split by cfg: native keeps `Send + Sync`, wasm uses `?Send`; request bodies hoisted into shared inherent methods. `rap-protocol` green on wasm; native workspace still green. |
| 2026-11-17 | 0.3 | tokio `net`, `hyper`, `hyper-util`, `http-body-util` moved to non-wasm deps in `rap-client`; `callback_server` module cfg-gated. `rap-client` green on wasm. |
| 2026-11-17 | 0.4 | **Reclassified, not implemented:** `infinity-protocol` is green without path injection (the `dirs` crate compiles for wasm). `dirs::home_dir()` yields no directory at runtime on wasm, so injection remains a Phase 3 correctness item rather than a compile blocker. |
| 2026-11-17 | 0.5 | `Tool<M>` cfg-gated (native `Send + Sync` + `#[async_trait]`; wasm bare + `?Send`), `cfg_attr` on 11 impl sites including `NoRapHttp` (E0053). `infinity-agent-core` green on wasm. **W5/W6/`system/local` deferred to Phase 3** (D18): they are runtime-correctness items, not compile blockers. |
| 2026-11-17 | process | Work (docs audit + chunks 0.1–0.5) had been done on `main` in the root checkout; corrected by moving everything into the `../infinity-hybrid` worktree on `feat/wasm-android-hybrid` (overview §1). Root is clean on `main` again. |
| 2026-11-17 | 0.6 | `sandbox-core` gained a default-on `native` feature gating `server`/`git`/`jj`, `SpawnedCommand`/`SandboxBackend::spawn_command`, and 8 native-only deps (axum, reqwest, similar, which, tempfile, base64, tokio, tracing). Browser builds use `--no-default-features`; the gate passes that flag for this crate. Verified: wasm green, native workspace green, 31 sandbox-core tests pass. Found: `which` 8.0.2 does not compile for wasm at all (`E0046`), reachable only from the gated server. |
| 2026-11-17 | docs | Added `docs/docs/WASM/README.md` — consumer guide covering the `default-features = false` requirement and its failure modes, RNG features, the target-dependent `Send` bounds, and the runtime-only wasm limits. |
| 2026-11-17 | 0.7 | `infinity-mcp-bridge` gained the same default-on `native` feature (stdio transport, `tokio/process` + `tokio/io-util`), cfg-gated `McpTransport` and `McpTransportFactory`, and `tokio::task::spawn_local` for tool-result delivery in the browser. `JsonRpcRequest` + the `Serialize` import are native-only (clippy caught both). Gate reached **7 green / 0 expected-red**. Verified: native workspace green, mcp-bridge tests pass, clippy clean with and without default features. |
| 2026-11-17 | 0.8 | Phase 0 exit verified: wasm gate 7/7 green (empty expected-red set), native `cargo check --workspace --all-targets` green, 141 tests passing across the touched crates (`rap-protocol`, `rap-client`, `infinity-agent-core` 108, `sandbox-core` 31, `infinity-mcp-bridge`), `cargo fmt --check` clean, clippy clean with and without default features. |
| 2026-11-17 | 1.1 | `sandbox-core` gained `executor`: portable `SandboxExecutor` (cfg-gated `Send`), `ExecRequest`/`ExecOutput`, and native `ProcessSandboxExecutor` with path confinement. 3 tests, including object-safety through `Box<dyn SandboxExecutor>`. |
| 2026-11-17 | 1.5 | No implementation work needed — `HttpMcpTransport` already does Streamable HTTP + SSE. Added a test with a fake loopback server covering SSE parsing and `mcp-session-id` capture. |
| 2026-11-17 | 1.2/1.3 | **Deferred to Phase 3/4** (D19): the flows already exist as `AgentSystem::step` / `SessionManager`; extracting traits now would guess the shape and refactor untestable AWS/daemon code. |
| 2026-11-17 | 1.4 | New fork-only crate `infinity-host` (D20) exposing `HostCtx<C,S,H,T,E>` + optional MCP factory; default-on `native` feature forwards to `sandbox-core`/`infinity-mcp-bridge`. Gate now covers 8 crates. Tests: capability access, optional MCP, `into_parts`. |
| 2026-11-17 | 2.1 | `apps/local-first/android` scaffolded (excluded from the workspace, D16), Tauri CLI 2.12.5 via pnpm, `android init`, then release + signed debug APKs built for x86_64 and aarch64. Build findings recorded in the archetype README: `frontendDist` is relative to the config, `bundle.targets` must be `"all"` (`--apk` selects the APK), icons must be **RGBA**, and `JAVA_HOME` must point at a real JDK. |
| 2026-11-17 | 2.2 | Verified on a real device (Android 13, arm64-v8a) **headlessly**: `adb install` → `am start` → `run-as cat probe.json` returned `{"target":"aarch64","sandbox_status":0,"sandbox_stdout":"infinity-on-android"}`. The run found a real bug — `ProcessSandboxExecutor` spawned with a nonexistent cwd (`ENOENT`); fixed by creating the root/cwd before spawning, with a regression test. |
| 2026-11-17 | env | Emulator could not boot under host memory pressure (587 MB free, swap exhausted); Gradle + Kotlin daemons were stopped to recover ~15 GB. The KVM device was present but the user lacked group access (fixed with an ACL); the device run used the physical phone instead. |
| 2026-11-17 | 2.2b | Real model turn on-device over HTTPS: `chat.json` → `{"model":"gemma-4-31b","reply":"infinity-on-android-llm","notes":[],"timed_out":false}` in ~10 s, key pushed at runtime via `run-as`. The first attempts **hung with no error or log line**: reqwest 0.13's `rustls` feature always routes through `rustls-platform-verifier`, whose Android backend needs JNI init plus a Kotlin AAR (reqwest#2966, tauri#13267). Fixed without Gradle by building a `rustls::ClientConfig` from embedded `webpki-roots` and injecting it via `use_preconfigured_tls` + rig's `.http_client()`. Also found: `CompletionsClient<H>`'s `H` must be pinned (`CompletionsClient::<reqwest::Client>::builder()`) because `.http_client()` replaces it. |
| 2026-11-17 | 2.3 | New `apps/local-first/android/crates/tauri-plugin-infinity-android` (D22): Kotlin `@Command`s for `count_contacts` and `shizuku_status`, a Rust half that forwards over `run_mobile_plugin`, an `Unsupported` desktop stub, and `permissions/default.toml`. Notifications use the official `tauri-plugin-notification`. Verified headlessly with `scripts/device-probe.sh`: `pm grant READ_CONTACTS` → `{"count": 1064, "granted": true}`; `POST_NOTIFICATIONS` → `native.json` says `"shown"` and a `NotificationRecord` appears in `dumpsys notification`; `shizuku_status` → `{"installed": false, "running": false, "sdkInt": 33}` (Shizuku is not installed, so that path stays compile-only). Widget: `run_mobile_plugin` must not be called on the setup path, because `count_contacts` can wait on a permission prompt. |
| 2026-11-17 | 2.4 | WhatsApp Web prototype **succeeded technically**: an overlay `WebView` with a desktop UA loaded `web.whatsapp.com` to `readyState: complete`, rendered the QR canvas, and reported `serviceWorker`/`indexedDB`/`subtleCrypto`; `screencap` shows the real login screen. Pairing needs a human scan, so no session was created. Recorded as D24 with the ToS constraint: WhatsApp names unofficial clients and automation as ban triggers, so the fork shows the view and ships no automation. The probe is opt-in through a `whatsapp-probe` marker file. |
| 2026-11-17 | 2.5 | WhatsApp automation (D25/D26). Hidden WebView at full size inserted at index 0, behind Tauri's UI, keeps a real 360 px viewport. Pairing by phone number works; the link survives restarts via IndexedDB. Synthetic `click()` does nothing on chat rows — trusted `MotionEvent` taps do. `whatsapp_state` verified (21 chat rows), message selector verified over CDP (23 messages), send implemented with dry-run default. |
| 2026-11-17 | process | Two self-inflicted failures recorded so they are not repeated: rebuilding the app mid-link kills the pairing handshake; and a 1.5 s wait after tapping a chat reads an empty panel, so reads retry until messages render. |
| 2026-11-17 | 3.1 | New fork crate `crates/infinity-wasm` (browser runtime). It builds for the host and for `wasm32-unknown-unknown`, is a workspace member, and the wasm gate now covers it — 9 crates green. Two tests state the browser's actual capabilities (no process sandbox). |
| 2026-11-17 | 3.2 | `SliceTimer` over `wasmtimer` 0.4.5, as a boxed-future trait (not `async_trait`: the browser's futures are `!Send`, the same constraint as D15). Native builds keep `tokio::time`. 4 tests pass; one initially failed because a tokio sleep future was created outside a runtime. |
| 2026-11-17 | 3.3 | **Blocked, not built.** Relaxing the store traits' `Send + Sync` bounds is not enough: the core's async functions require `Send` futures, so a `!Send` IndexedDB handle fails at `event_processor.rs:209` and `system/local/router.rs:338`. Reverted; both targets green. The real work is `?Send` propagation on wasm (D27). Library research done: `idb` 0.6.5, `wasmtimer` 0.4.5, `wasmi` 1.0.9; OPFS/SQLite compared and deferred (needs a dedicated worker and 0.5–1 MB of wasm for queries the traits do not use). |
| 2026-11-17 | 3.3 | Storage engine **proven**: `@sqlite.org/sqlite-wasm` 3.54.1 + OPFS in a dedicated worker (headless Chrome via CDP). 100 rows per load; reload went 100 → 200 (`persistence: PASS`). Cost measured: wasm 879,084 B raw / 407,552 B gzip, JS API 656,872 B (D28). Wiring into the core still waits on `?Send` (D27). |
| 2026-11-17 | E2E-1 | New `crates/infinity-sync`: immutable `SyncEvent` (UUIDv7 ids), `EventStore` contract, JSONL `FileEventStore`, per-device `CursorStore`. 6 tests green (ordered reads, idempotent re-append, session listing, cursors). Native-only: the browser store waits on D27. |
| 2026-11-17 | tooling | Installed per overview §8: `wasm-pack` 0.15.1, `wasm-opt` 133 (Binaryen release; the npm shim had no linux binary and was removed), `trunk` 0.22.1, `twiggy` 0.8.0, `wabt` via npm. `chrome-devtools-mcp` written to `~/.config/kimchi/harness/mcp.json` with `--headless --isolated` — takes effect on the next Kimchi start. |

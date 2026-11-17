# Handoff: replace the agent environment in a cross-platform consumer app

**Audience:** the agent adding Infinity to a consumer application on **desktop, web, and Android**.

**Source:** `beznet22/infinity`, branch `feat/wasm-android-hybrid`. The implementation inspection for this guide was anchored at `f9c1b32`; use the later commit that introduces this document and confirm subsequent diffs.

**Status:** this is a **porting and remediation guide**, not a claim that the consumer runtime is finished. The archetypes prove important mechanisms independently. They are **not** one production-ready application. The defects and integration gaps below are deliberately left in the blueprint repository; **fix them in the consumer app rather than copying them**.

Companion reading:

- [`README.md`](./README.md) — target-dependent compilation rules.
- [`overview.md`](./overview.md) — architectural background.
- [`decisions.md`](./decisions.md) — D1–D64, evidence and limitations; later entries supersede “next step” remarks in earlier entries.
- [`implementation-plan.md`](./implementation-plan.md) — original phases, not a current completion report.
- [`PROGRESS.md`](./PROGRESS.md) — the later chunk log is more current than the original phase tables.

## 1. What to build

Keep the consumer application's identity, UX, authentication, permission prompts, billing/model accounts and tool policy. Replace its agent execution/session layer with:

```text
 consumer UI and settings
        │ SessionView + commands
        ▼
 one application-session driver per platform
        │ stable session/ThreadId, FIFO queue, errors, cancellation
        ▼
 infinity-agent-core (local driver)
        ├── ModelProvider → configured provider, credentials held at runtime
        ├── Tool registry → explicit capabilities and isolated executors
        ├── ConversationStore + StateStore → durable model context/execution state
        └── event projection → SessionView, queue/edit/truncate/restore
                                      │
                               optional authenticated relay
                             (not required for local use)
```

**Required invariant:** the session shown by the UI and the `ThreadId` used by the model must refer to the same durable conversation. Sending through a new random thread each time is not resume, even if old messages happen to render from another database.

### Decide these host questions before implementation

| Choice | Safe starting point | Do not silently assume |
|---|---|---|
| Desktop/mobile framework | Tauri v2 if reusing the supplied native integration | Electron/React Native bridges are not included or tested |
| Provider credentials | Host-supplied in memory or platform secret storage; explicit mock model for development | No bundled keys, universal CORS access, or silent fallback to a paid endpoint |
| Local storage | Private app-data SQLite on native; browser Worker-owned OPFS plus the core's IndexedDB actor stores | OPFS quota/persistence is not automatic or permanent |
| Sessions | Single-user, caller-owned stable thread ids; local one-writer ownership | No distributed exactly-once execution or multi-tenant isolation |
| Tools | Explicit registry, least privilege, user approval for sensitive capabilities | No arbitrary process execution or unrestricted WASI/network/filesystem |
| Cloud | Optional authenticated sync after local use works | The test relay is deleted; no public relay or browser wake service is included |

If the consumer requires multi-user accounts, strong deletion guarantees, arbitrary untrusted tools, or exactly-once external actions, those are additional product requirements, not configuration toggles in the archetype.

## 2. Reusable code — what it actually proves

Paths below are relative to the fork root.

| Source | Reuse | Proven here | What it does **not** prove |
|---|---|---|---|
| `crates/infinity-host/src/lib.rs` | `HostCtx<C,S,H,T,E>` injection of conversation/state stores, HTTP, toolset cache, sandbox, optional MCP factory | Portable host assembly | A populated production host or persistent app session |
| `crates/infinity-agent-core` | Local agent driver, durable-store contracts, observer/replay, provider/tool machinery | Core local execution, native and Wasm | Your host's session identity, storage and tool policy |
| `crates/infinity-wasm/src/browser.rs` | Browser mock/OpenAI-compatible turns and IndexedDB core stores | Real browser provider turn; core stores survive reload | Integration with the OPFS UI, stable session reuse, or complete provider features |
| `apps/local-first/android/src/lib.rs` | Rig provider wiring, portable TLS, foreground-service/lease and queue-driving probes | Real Android model turn; lease/heartbeat demonstrated on hardware | Durable model history or a production native driver |
| `apps/local-first/crates/infinity-sync` | Event types, SQLite log, queue/edit/truncate/restore helpers, lease/fence APIs, summaries and `session_view()` | Store/view tests and native usage | Core model-context persistence; production multi-device ordering |
| `apps/local-first/cli` | Reference commands for log operations | Useful local test/debug surface | An agent/model driver |
| `apps/local-first/browser/renderer/renderer.js` | Pure `renderThreads(root, {sessions, active}, {onCommand, onSelect})`; DOM uses text nodes | Shared view fixture and UI behavior | Platform auth/lease enforcement or complete conversation UX |
| `apps/local-first/browser/view` | JS `sessionView` mirror with a Rust-generated parity fixture | Fixture parity today | A guarantee against future drift |
| `apps/local-first/browser/app` | Dedicated-worker `OpfsDb("infinity-app.db")`, load/append, session-page wiring | UI mutation survives reload in Chrome | Real agent turns, leases or multi-tab coordination |
| `apps/local-first/browser/sandbox` | Per-tool Worker, one allowed `env.memory` import, 16-page maximum, 500 ms termination | Benign/hostile/spin JS-Wasm probes | Tools connected to the application's registry |
| `apps/local-first/crates/infinity-wasm-sandbox` | Android wasmi with no imports, 1 MiB memory, 10M fuel, `run() -> i32` | Host and device benign/hostile/spin probes | Dynamic module loading, richer host capabilities or a tool registry |
| `apps/local-first/worker` | D1 push/pull/sessions, conditional fence insert, shared-token gate for local trials | Local D1/auth/fence-race tests | Public deployment, per-user authorization, browser CORS verification or rate limiting |
| `scripts/{check-wasm,check-android}.bash`, `.github/workflows/fork-ci.yml`, `apps/local-first/scripts/e2e.py` | Initial verification structure | Existing regression gates | Full product matrix or shared-relay conversation E2E |

`apps/local-first` is a **consumer archetype**, not a root workspace member. Its nested Rust crate and Android workspaces are intentional (D16). If vendoring it wholesale, retain those workspace boundaries or adapt them with equal care; do not pull the Tauri app into a foundation-only build accidentally.

## 3. Critical: two persistence layers, not one

A mistake in this section silently destroys the agent's memory after restart.

### 3.1 Core execution stores

The core needs the traits in `crates/infinity-agent-core/src/traits.rs`:

- `ConversationStore`: thread existence/roots, history and dedup message ids, parent/ancestor links, spawn cutoffs, fresh-context boundaries, closure/subscription flags, and compaction summaries.
- `StateStore`: processed input ids, metadata, active subscriptions and pending user choices.

Use these stores in every `AgentSystemBuilder::new_local(conversation_store, state_store, model)` call. Their per-thread identity must be stable across process/runtime creation.

- **Native starting point:** `SqliteEventStore` in `infinity-sync` does **not** implement these traits. Android still inserts `InMemoryConversationStore` and `InMemoryStateStore` in its provider path. Supply real implementations in the consumer.
- **Preferred reuse, not duplication:** the core's `InMemoryConversationStore` publicly exposes serializable `ThreadInfo`, `CompactionSummary`, `thread_messages`, and corresponding `set_thread_*` accessors. `InMemoryStateStore` exposes `ThreadState` and `set_thread_state`. A thin durable native wrapper can delegate core semantics and persist per-thread snapshots instead of porting hundreds of lines of behavior.
- **Do not blindly import `infinity-daemon` for this:** its `PersistentConversationStore`/`PersistentStateStore` depend on daemon-specific ids, model metadata and session/stopped-session policy. Evaluate it only if you actually want the daemon architecture. Its filesystem write paths also contain intentionally best-effort behavior; that does not meet a fail-fast consumer-storage requirement without review.
- **Browser already has real implementations:** `IndexedDbConversationStore`, `IndexedDbStateStore` and their actor handles in `crates/infinity-wasm/src/{conversation_store,state_store}.rs`. Preserve the **separate** databases `infinity-agent-history` and `infinity-agent-state`. Their independent version upgrades avoid the missing-object-store hang documented in D39.

Persist every mutation and **return** encoding, database, permission and corruption errors. Never turn a malformed persisted thread into an empty successful history. Tests must reopen with fresh handles, not query the same in-memory wrapper.

Define stopped-session behavior explicitly. `StateStore::is_thread_stopped` defaults to `false`; inheriting it without thought means synthetic results/subscription inputs may resume a conversation your product marked as stopped. Persist the desired stopped state or reject those inputs in your driver; ordinary user text remains the intentional resume path.

### 3.2 Projected event log and view

`infinity-sync` is the second store: immutable `SyncEvent`s power cross-terminal transcripts, queues, edit history, lease state and restoration. `session_view()` returns the shape the renderer consumes:

- `session`, `device`, `read_only`, `holder`, `fence`, `last_seq`
- `messages[]`: `event`, `role`, `text`, `versions`, `reply_to`
- `queue[]`: `event`, `text`
- `truncations[]`: `id`, `from`, `active`

Project `UserMessage`/`MessageQueued`, `MessageSent`, model/tool lifecycle and final `AssistantMessage` from the driver into this log. Store the core's actual message/dedup ids in the projection so retries cannot submit a second turn. Do not make two unrelated histories and hope their strings match.

A snapshot in `infinity-sync::replay()` is a generic serialized value until your host explicitly connects it to core execution state. It is not automatic model snapshot/restore. Truncation hides a transcript suffix; it is **not** erasure of the event log.

## 4. Stable session identity and runtime lifecycle

### 4.1 Use caller-owned `ThreadId`

Do **not** follow `thread_builder().launch()` blindly for the default UI session: it generates a random thread id. That is useful for genuinely new subagents, not for every user turn.

The verified lower-level seam is in `system/local/router.rs` and `system/builder.rs`:

1. Build via `AgentSystemBuilder::new_local(...)`.
2. Start via `start_with_observer(...)`.
3. `subscribe(...)` to the caller's `ThreadId` and process its replay/event stream using a `ThreadObserver`.
4. Send `InputMessage::user_text(thread_id, ...)` through `RunningSystem::send(...)` with a **stable dedup id**, or use its convenience text method only when retry-deduplication is handled elsewhere.
5. Shut down the running system on cancellation or terminal cleanup. Dropping a UI handle does not release a lease and is not equivalent to cancelling execution.

Map one consumer session to one root thread id and preserve it. Built-in child threads retain their own ids/ancestry; don't replace those with the user's display session id.

On native, keep the core's `Rc`/`LocalSet`-bound machinery on a dedicated thread with `tokio::runtime::Builder::new_current_thread()` and `LocalSet`, as the Android setup does. Do not send `!Send` state through Tauri, a global static, or an unsafe `Send` wrapper. Send request/response messages across the host boundary. Browser `LocalSet` runs without constructing a native runtime/time driver.

**Minimum resume test:** run turn A against a controlled mock provider, destroy the engine, rebuild stores at the same path and the same thread id, then run B. Inspect the second provider request: it must contain A's context. Assert that no other session's history is present. A reload count in a UI database is **not** a substitute for this test.

### 4.2 One driver owns commands and work

A consumer-side driver should own:

- monotonically persisted command/event ids and stable retry dedup ids;
- one keyboard-to-model FIFO per session, with a bounded per-drain batch;
- `MessageSent` when an item is taken and `AssistantMessage` only on a successful response;
- progress, cancellation, timeout, provider, storage and permission errors visible to the user;
- renewal/expiry, stale-writer refusal and foreground/background lifecycle.

Do not issue a fire-and-forget provider call from each Tauri/UI callback. A single worker/driver can serialize local model/tool effects while allowing UI reads.

## 5. Android integration

Start from the wiring in `apps/local-first/android/src/lib.rs`, not by copying its startup probe into a product.

1. **Extract transport setup.** `portable_http_client()` embeds `webpki_roots` through `use_preconfigured_tls`; it avoids the measured Android platform-verifier TLS stall. Inject that client into `SimpleHttpClient`, relay transport and rig `.http_client(...)`, pinning `CompletionsClient::<reqwest::Client>`. Private/corporate/user CAs are consequently **not** loaded; a host requiring them must deliberately ship and initialize the platform-verifier AAR/JNI path instead (D21/D34).
2. **Replace in-memory core stores** using §3. The visible SQLite event log alone is not memory for the model.
3. **Move probe behavior into a driver.** `chat_probe` presently owns acquisition, model setup, queue draining, heartbeat, projection and release. Extract those responsibilities around a stable session and persistent core stores; preserve bounded queue draining and explicit error return. Do not run optional tests/pairing/automation at every startup.
4. **Pass runtime configuration safely.** Do not cargo-bundle or commit probe files such as `chat-config.json`, `sync-config.json`, or `.env`. Development probe configuration is not a production secret/policy store.
5. **Use foreground service deliberately:** channel `infinity-session`, service type `dataSync`, explicit start/stop around work. Lease heartbeat uses 5 s with a 60 s TTL. Android 12/14/15 foreground-start/type/time restrictions and OEM process kills still apply; this is run reliability, not an immortal background worker. Select a host-appropriate service type for long-lived non-data-sync work.

Hard bug to fix rather than copy: `sync_probe` sets `root = dir.join("sync")` and then opens `root.join("sync/events.sqlite")`, while the UI opens `dir.join("sync/events.sqlite")`. Canonicalize **one** `<app-data>/sync/events.sqlite` path and test that a probe write appears in the UI. Treat absent optional sync configuration separately from malformed configuration rather than silently treating both as “no relay.”

The Android contacts/notification probes were device-tested. **Shizuku was not installed**, so that code is compile-only, not a working privileged-tool claim.

## 6. Desktop integration and the confirmed blocker

The desktop Tauri GUI path has **not** been verified. On this source revision, a host check specifically reached four `E0107` diagnostics: desktop `InfinityAndroid` is non-generic, while `desktop.rs` and the plugin trait reference `InfinityAndroid<R>`.

Affected code:

- `apps/local-first/crates/tauri-plugin-infinity-android/src/desktop.rs`
- `apps/local-first/crates/tauri-plugin-infinity-android/src/lib.rs`

**Repair choices for the consuming app:**

- If Android-only registration is irrelevant to the desktop shell, gate/remove that plugin build/registration from desktop and provide explicit native services instead; **or**
- Make the unsupported desktop state/runtime type generic consistently (including construction), retaining its current honest `Unsupported` errors rather than pretending to implement Android capabilities.

In either case gate mobile foreground/contacts/WhatsApp calls behind the platform they actually support and expose desktop substitutes or clear unsupported results.

Required desktop acceptance:

1. Compile the **whole desktop app**, not just core crates, on the consumer's target OS.
2. Run the same-session/restart driver test with native durable core stores (§4.1).
3. Exercise actual UI send/queue/progress/error/recovery; there is no `cargo check` proof of a working desktop window.
4. Adopt desktop-native sandbox/permission/notification paths. `/system/bin/sh` and Android native-library execution do not generalize to desktop OSes.
5. Replace the archetype's `tauri.conf.json` `"csp": null` with a reviewed content security policy. Restrict commands/capabilities instead of inheriting development probes.

The browser-model and independent desktop `local-sync` CLI checks are **not** evidence of a desktop GUI or full desktop agent.

## 7. Browser integration

There are **three separate experiments** in the source:

1. `crates/infinity-wasm/src/browser.rs` — real core turns and durable core IDB stores;
2. `browser/app` — OPFS event log and shared renderer;
3. `browser/sync-worker.mjs` / root `browser/index.html` — older relay/storage experiment.

Do not treat their combined feature lists as if they were one application. Compose the first two deliberately; remove rather than re-point the third.

### 7.1 Required repair of the old relay experiment

`browser/sync-worker.mjs` targets the **deleted** `local-first-sync.beznet22.workers.dev` deployment, uses an older local schema (`created_at` instead of `created_at_ms`, only `event_id` primary key, no fence on the event row), contains no auth, and writes a `cloud1` event on loading the page. Do not ship or “fix” this by inserting a new URL and key. Replace its automatic initialization with the production worker protocol described below or delete this entry point.

### 7.2 Connecting real turns to the UI

- Use the stable `ThreadId`/observer seam in §4 with the existing `IndexedDbConversationStore`/`IndexedDbStateStore`. The current exported turn helpers call `thread_builder().launch()`, so they must not be reused unchanged for a stable user session.
- Retain the OPFS dedicated module worker for event-log access; the page shouldn't touch SQLite synchronously. Drive the same command/projection contract as the native host.
- Open real provider config with `run_openai_compatible_turn(base_url, api_key, model, prompt)` as a **reference**, but split configuration from the stable-session runner. Credentials remain runtime-only, are never placed in page URLs/log output, and are not persisted with conversations. A non-CORS provider needs a properly authenticated consumer-owned backend, not an open CORS proxy.
- The current browser OpenAI-compatible provider flattens text requests/replies. Revalidate it before promising tool calls, structured output, media or true streamed provider deltas. `run_smoke_turn`/`"infinity-wasm-turn-ok"` proves execution, not a paid-provider service.
- Preserve tokio/wasmtimer substitutions; native filesystem/processes/sockets and `tokio::time` cannot be used simply because they type-check on Wasm. See the root guide's getrandom/uuid/`Send` notes.

### 7.3 One browser authority, not independent racing tabs

`browser/app` currently uses fixed ids (`SESSION = "browser-chat"`, `DEVICE = "browser"`), no lease, and no shared ownership. Give the installation/profile a securely generated persistent identity and distinguish active execution clients/tabs. Do not derive a new root id at every reload.

Recommended shape:

- A **same-origin SharedWorker broker** manages connected views, session-scoped locks, command dispatch and statuses.
- One **dedicated OPFS DB worker** is the only event-store connection. The SharedWorker may forward requests to it; verify nested-worker support on the consumer's browser matrix before selecting this.
- Lease claim/renew/release run as **one transaction** at that authority. Compare owner and expected fence with append in the same critical section. The UI may read at any time; only an owned live lease can drive work.
- While a model call is in flight, renew independently of its completion. On `LeaseLost`, reject further projected writes and prevent later queued turns; do not continue merely because the model already started.
- `200 OK` plus a JavaScript response is not proof that a lease is globally serialized. Test two real tabs, collision, close/reload, TTL expiry and explicit release.

Lifecycle limits: browser/SharedWorker termination is normal, OPFS can be evicted, and `navigator.storage.persist()` is a request, not a guarantee. No background-agent or wake guarantee follows from this local design. If SharedWorker ownership is unsupported for your required matrix, choose another **measured** single-authority design; don't allow one independent OPFS/worker authority per tab.

### 7.4 Reproduce before adding sync

Browser acceptance requires fresh browser state and two tabs:

1. Turn A runs the actual core Wasm provider using stable session identity, and its complete answer appears in the shared renderer.
2. After recreating the engine and reloading, turn B's provider request contains A's history; OPFS contains one projection, not a duplicate assistant answer.
3. Tab 1 owns a live lease; tab 2 renders the same view but cannot start a model call. After valid release/expiry, ownership may transfer with a new fence.
4. No stale renewal, dequeue promotion, freed-tab duplicate event, or blank error is allowed.

## 8. Lease and data-safety repairs required on every host

The existing primitives are useful, but some **call paths** are prototype-only. Do not copy these behaviors:

| Current behavior to avoid | Consumer-app repair | Required regression |
|---|---|---|
| Android `run_prompt` logs a `guard.renew` error and continues | Stop scheduling work; do not append more events with the obsolete fence; stop affected tools/cancel where supported and surface failure | Supersede during renew: no later queue turn or stale projected write |
| `apply_command` renders `read_only` but does not enforce a fresh lease check | Recheck state in the command/drive path, not just disable buttons | Direct native command against another owner's valid claim is refused |
| Android direct send does nothing when the existing queue is nonempty | Preserve FIFO and enqueue/drive the new item; never drop it or run it out of order | Existing queue + new send preserves all messages and sequence |
| Shared renderer emits `versions`; native enum/browser default do not implement it | Implement the version-switch fold/UI, or mark that entry unavailable; do not call edit/branch UX completed | Badge selecting old/new version restores both its text and answer lineage |
| `cursor`/writes use per-device `seq` as if it were total order | Keep relay `position` as canonical received order and persist its association with events; do not infer global priority from `seq` or UUIDv7 | Interleaved writes from A and B produce the same fold after reordering arrival retries |
| Local claim/check-before-model is treated as distributed mutual exclusion | It is cooperative local ownership; fencing rejects stale **appends**, it cannot undo an already invoked provider/tool effect | Concurrent contenders demonstrate the exact guarantee you claim; use an authoritative coordinator for exactly-once external effects |
| Every local operation treats fixed `android`/`browser` device ids as unique | Generate/store a stable identity per app installation/profile and a separate active-client identity where required | Restart preserves identity; concurrent instances cannot accidentally become the same writer |
| Whole-session fold is loaded/rebuilt for each UI action | Add pagination/caching/virtualization appropriate to the host after preserving semantics | Long history still meets your memory/latency budgets |

`LeaseGuard` is single-process/useful application machinery, not proof against two offline devices racing claims. In replay, `current_lease` is order-sensitive. The D1 conditional INSERT and trigger close the measured stale-insert race **at the relay**, but it does not convert a local, unordered copy into a universal causal history or a distributed exactly-once executor.

For local mode, a worker-scoped SQLite transaction with busy/timeout handling is a practical synchronization seam. Test restart, partial commit and a failed write, not only an in-memory happy path.

## 9. Optional relay: retain local-only default

The user needs the whole app to work with **no relay configured**. The public test deployment is deleted. Do not recreate it or store credentials automatically.

The relevant native seam is `infinity_sync::http::HttpTransport::{new, with_client, with_token}`. Android must use `with_client(portable_http_client(), ...)`. Browser implementation must supply the same protocol and configure CORS before claiming parity.

| Endpoint | Contract |
|---|---|
| `POST /v1/sync/push` | Body `{events: SyncEvent[]}`; dedupe on `(session_id,event_id)`, fenced writes, relay-assigned positions on stored events |
| `GET /v1/sync/pull?session=<id>&after=<position>` | Stored events after relay position, oldest first |
| `GET /v1/sync/sessions` | Authorized session ids only, not a global unfiltered user-data list |
| `GET /health` | Liveness only, not authorization or sync verification |

Mandatory work before deployment:

- Replace one global shared `RELAY_TOKEN` with authenticated **per-user/device authorization**, session ACLs and tenant-scoped queries. A shared token is not per-user isolation.
- Add payload/event-size limits, quotas, rate limits and abuse/error telemetry without secrets in logs.
- The Worker currently sends `"access-control-allow-headers": "content-type"`, while the authenticated native client sends `Authorization`. Authenticated cross-origin browser calls can therefore fail preflight. Explicitly allow `authorization`, use an exact intended origin policy, then verify the real browser preflight and request.
- Handle accepted vs duplicate/stale acknowledgements correctly. D1 acceptance is based on row presence; duplicate pushes are idempotent, but that does not mean a fresh event was newly created.
- Persist and test canonical relay position/order; make pull/append/cursor advancement safe across crash and retry.
- Choose an explicit consumer disclosure/data-retention/erasure policy. Immutable tombstones do not meet a secure-delete requirement.
- Use a new D1 database identifier, schema and secret if deployment is later approved. Do not revive identifiers from this development configuration as though a database exists.

`local-sync watch` is **polling**, not push. There is no browser service-worker wake path in the present implementation; Web Push requires its own service worker, VAPID key management, subscription storage and an approved push service. Even a delivered notification does not make a browser a permanently resident agent.

## 10. Tool execution and other sensitive capabilities

### Untrusted Wasm

The supplied **executors** are valid small foundations, not finished tool stacks:

- Android wasmi rejects **every** host import, restricts memory to 1 MiB, and limits each run with fuel.
- Browser Workers accept only host-supplied `env.memory` with maximum 16 pages, reject self-declared memory/WASI, and are terminated on an execution deadline.
- Integrate the correct tool module through a reviewed registry, handle non-I32/error exports, JSON argument/result transport and lifecycle; expose additional capabilities narrowly, with user-visible permission controls.
- Add regression tests whenever a host import is added: lack of imports is the current structural guarantee, not a claim that an arbitrary plugin SDK is isolated.

`ProcessSandboxExecutor` is a native adapter with path confinement and spawn behavior, not proof of a full OS security jail. The `/system/bin/sh` probe is intentionally limited. Use a reviewed OS sandbox where running arbitrary untrusted native processes is a requirement. Never silently fall back from a failed isolated executor to unsandboxed execution.

### WhatsApp (optional, unofficial automation)

Do not make this part of standard startup or acquire a conversation indirectly just because an agent asks “send something.” This is DOM automation of a real third-party account with ban/session-loss risk.

- Pair/link requires user involvement and the configured personal details are never repository material. Do not rebuild/restart during a link handshake.
- `whatsapp_read` retry and missing-row rejection fixes exist, and successful read was device-tested. One user-approved real delivery was performed through **direct CDP**, not through the plugin's own `whatsapp_send` command.
- Before relying on the plugin submit path, retest end-to-end in dry-run then with one explicitly approved message. The inspected current search interface used an `input[aria-label="Search or start a new chat"]`, not the previous contenteditable selector.
- Select the exact intended **direct** conversation, read its header, and ensure it is not a group or merely a matching participant in a group. Require human confirmation for each externally published message. Check composer content exactly; a one-character/bad draft is a failed operation.
- Default drafts to dry-run; never claim automatic success merely from a JS return value. Verify the message clears from the composer and is in the intended conversation afterwards. No marker/config/session/private export should be retained in a consumer handoff or snapshot.

## 11. Verification for the target project

### Baseline commands in this fork

Run from the actual directories, ensuring SDK/NDK/JDK values come from your environment rather than copying this machine's defaults:

```bash
# Root: toolchain and native regression baseline.
bash scripts/check-wasm.bash
bash scripts/check-android.bash
./check.bash

# App-layer reference tests; Tauri/mobile pieces are intentionally not this workspace.
cd apps/local-first
cargo test --workspace
node browser/view/test.mjs

# Real worker typecheck, not a no-op tsconfig.
cd worker
pnpm install --frozen-lockfile
pnpm exec tsc --noEmit

# Browser core package and checked budget (CI: 4,300,000 raw / 1,100,000 gzip).
cd ../../../crates/infinity-wasm
wasm-pack build --target web --release

# Native desktop host gate: currently fails at the Android plugin until repaired (§6).
cargo check --manifest-path ../../../apps/local-first/crates/tauri-plugin-infinity-android/Cargo.toml --lib
```

Android, after Tauri project generation and explicit SDK/NDK/JDK setup:

```bash
cd apps/local-first/android
scripts/build-apk.sh aarch64 debug
scripts/device-probe.sh <approved-device-serial>
# Optional release gate: arm64 unsigned APK, 33,000,000 byte cap.
scripts/check-size.sh
```

The Rust Android target needs `aarch64-linux-android` (and any additional consumer ABIs), with NDK C toolchain/linker settings. `build-apk.sh` handles known environment names but defaults to a local NDK/JDK; set `ANDROID_NDK_HOME`, `ANDROID_HOME` and a working `JAVA_HOME`. Regenerate `android/gen` rather than treating it as a portable source directory. `tauri android init` requires an interactive TTY in this development environment. Release is Tauri's default; only debug receives `--debug`.

The reference combined E2E (Python 3, Chrome/Chromium, `websockets`, initialized browser dependencies and a built `local-sync` CLI) is:

```bash
cd apps/local-first
python3 scripts/e2e.py
# Only on the user's explicitly approved device:
ADB_DEVICE=<approved-device-serial> python3 scripts/e2e.py
```

It tests independent local desktop/browser/Android legs and prints `E2E PASS`. **That is not one replicated conversation and not product coverage.** Its Android leg reads the sandbox probe rather than driving the complete model/UI flow. Use the tests below as your admission gate.

### Consumer acceptance matrix

| Area | Must pass in the target app |
|---|---|
| Offline | Fresh local profile can create/read/edit/queue/truncate/restore with no relay and no network permission/service |
| Context resume | Native and Wasm second-turn provider request includes first-turn context using fresh handles and a stable thread id; another session is isolated |
| Retry | Retrying the same command/dedup id doesn't submit a second model call or duplicate a projected event |
| Native UI | Desktop (on target OS) and Android real UI send, progress, completion, persistent restart, errors and queue |
| Browser UI | Core Wasm turn + OPFS projection + IDB model history survive reload; no experiment calls a deleted service |
| Leases | Second active tab/device cannot start work; stale renewal and release fail; later queued work stops |
| Local durability | Crash during append cannot create partial or phantom complete writes; corrupt persisted state is a visible error |
| Security | No keys/relay credentials/phone/account data in logs, bundle or repo; explicit CSP/capability/tool permission policy |
| Tools | Benign, WASI-denial, memory/CPU limits, invalid module, timeout and denied-capability tests in the **app's actual registry** |
| Optional sync | Auth/tenant namespacing, CORS preflight, independent-origin browser push/pull, causal order, acceptance of retry, no version/tenant leaks |
| Optional wake | Only after deployment: real service-worker push reaches and wakes a closed browser tab; polling is not reported as push |
| Size | Measure and gate the consumer's own artifact after added UI/stores/tool registry; don't inherit another platform's headline size |

Historical artifact budgets are regression guides, not guarantees for the target app. At the measured revision, core Wasm was under 4.3 MB raw/1.1 MB gzip; the arm64 release APK measured 31,784,275 bytes against a 33,000,000-byte cap. Debug APK size is not a release metric.

Root `check.bash` can report unavailable optional `jj`, `bwrap`, Playwright or tool licenses as skips. Read the report; a zero exit does not mean skipped matrix items ran.

## 12. Recommended implementation order

1. **Vendor/submodule or depend on the concrete fork revision.** Preserve default-feature switches (`infinity-host`, `sandbox-core`, `infinity-mcp-bridge` use `default-features = false` for browser). Rerun root wasm/Android gates before modifying them.
2. **Native local vertical slice.** Establish stable `ThreadId`, durable core stores, real driver and command error propagation. Fix desktop registration/build before promising a UI.
3. **Android vertical slice.** Reuse native driver, correct canonical SQLite path, preserve TLS/browser capability separation; make background service operation explicit. Test UI and core history after process recreation.
4. **Browser vertical slice.** Introduce one worker authority, stable identity/leases, core-Wasm plus OPFS/IDB integration, browser security/quotas and an in-memory only provider credential boundary. Add two-tab tests before remote sync.
5. **Wire tools only after their execution/approval boundary is in the registry.** Keep tool/WASI default-deny and policy errors visible.
6. **Optional authenticated relay.** Add user identity, tenancy, canonical position, CORS/auth, rate limiting, quotas and live browser/network interruption tests. Local-only stays functional and default.
7. **Optional Web Push and external integrations** only with their own permissions/explicit user action/deployment approval.
8. **Add your product CI**: core crate compiler gates + actual desktop application target + Android APK/device path + real browser integration + resume/error tests. Compilation alone is insufficient.

Each step must be independently runnable, evidence-backed, and committed separately. Update the target project's decision/status documents when evidence contradicts an earlier blueprint.

## 13. First actions for the receiving agent

1. Read this document and the target/platform sections in `README.md` about `default-features`, RNG, cfg-gated `Send`, and browser runtime substitutions.
2. Record the chosen framework/browser matrix/provider credential model/storage retention and tool privilege requirements as explicit host decisions.
3. Create the stable-session/durable-core vertical slice **first**. Do not copy `chat_probe`, `apply_command`, or `browser/app/app.mjs` unchanged as a runtime.
4. Make the browser's deleted-relay experiment unreachable; remove the source or clearly quarantine it in dev-only fixtures. Do not deploy from `worker/wrangler.toml` unchanged.
5. Execute §4.1's model-request assertion on the consumer's native and browser targets before claiming memory/resume works.
6. Fix the confirmed desktop generic mismatch and run the **whole desktop app** build; don't rely on the Android-only target being green.
7. Build real target E2E and require the matrix in §11. Report skips/unknown platforms explicitly.
8. Only after local acceptance decide whether any service need exists. The optional relay and wake mechanism are not prerequisites to a working local agent.

The intended reuse is the **well-defined seam and proven platform adaptations**, not the prototype's accidental lifecycle and error policies. Carry over the testable boundaries, not the incidents they've exposed.

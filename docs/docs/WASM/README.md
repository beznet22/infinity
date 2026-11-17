# WASM fork — working docs

Documentation for forking Infinity into a hybrid runtime (on-device Android + in-browser WASM, with an optional Cloudflare edge). Start with the blueprint.

| Document | Purpose |
|---|---|
| [`overview.md`](./overview.md) | The fork blueprint (§1–§10): worktree strategy, core pillars, Android/Tauri architecture, trait adapters, reference repos, layout, build order, WASM toolchain, readiness analysis, design ideas. |
| [`audit.md`](./audit.md) | Claim-by-claim audit of the original overview (53 rows / 54 verdicts), feature-preservation map, Appendix C (compiler-verified wasm readiness). |
| [`decisions.md`](./decisions.md) | Decisions D1–D18 with options, rationale and rejected alternatives. |
| [`implementation-plan.md`](./implementation-plan.md) | Phase 0–4 chunk spec (what to build, in what order, with acceptance criteria). |
| [`PROGRESS.md`](./PROGRESS.md) | Living status tracker: what is done, what is next, gate states. |

Evidence notes (external verification, compiler sweeps, HTTP research) live outside the repo at `.kimchi/docs/wasm-overview-external-verification.md`.

---

## Consuming the fork from a `wasm32-unknown-unknown` build

### 1. `sandbox-core` needs `default-features = false`

`sandbox-core` ships a **default-on `native` feature** that gates its computer-only surface:

- modules `server` (the sandbox HTTP server), `git`, `jj`;
- `SpawnedCommand` and `SandboxBackend::spawn_command` (they hold a `tokio::process::Child`);
- the dependencies `axum`, `reqwest`, `similar`, `which`, `tempfile`, `base64`, `tokio`, `tracing`.

A browser build **must** disable it:

```toml
[dependencies]
sandbox-core = { path = "../sandbox-core", version = "^0.2.0", default-features = false }
```

**If you forget the flag** the build fails deep inside `mio`, with errors that say nothing about your own code:

```text
error: This wasm target is unsupported by mio. If using Tokio, disable the net feature.
error[E0433]: failed to resolve: could not find `event` in `sys`
```

`which` 8.0.2 additionally fails on this target with `E0046: not all trait items implemented, missing: is_valid_executable`; it is only reachable from the gated `server` module, so disabling `native` removes it entirely.

Notes:

- `scripts/check-wasm.bash` passes `--no-default-features` for `sandbox-core` explicitly, so the readiness gate checks the same configuration a browser build uses.
- Anything that pulls `sandbox-core` into a wasm target (today: nothing; from Phase 3: `crates/infinity-wasm`) must carry the same flag.
- `infinity-mcp-bridge` uses the same switch for its stdio transport:

  ```toml
  infinity-mcp-bridge = { path = "../infinity-mcp-bridge", version = "^0.2.0", default-features = false }
  ```

  With `native` off, `McpClient::stdio` / `McpToolSet::stdio` are not available; the Streamable-HTTP transport stays.
- The flag is safe on native too: it only drops the server/git/jj (and stdio-MCP) surface, which the native crates enable by default.

### 2. RNG: getrandom and uuid

`getrandom` 0.3/0.4 refuse to build for wasm until a backend is chosen, in two steps:

1. `/.cargo/config.toml` sets `--cfg getrandom_backend="wasm_js"` for `wasm32-unknown-unknown`.
2. The crate that needs randomness enables the feature — `infinity-agent-core` does so today (getrandom `wasm_js` for the `rhai`→`ahash` chain, and uuid `js` for `Uuid::new_v4`).

If you see `The "wasm_js" backend requires the wasm_js feature for getrandom`, step 2 is missing for a crate in your dependency graph. If you see

```text
to use `uuid` on `wasm32-unknown-unknown`, specify a source of randomness
using one of the `js`, `rng-getrandom`, or `rng-rand` features
```

enable `uuid`'s `js` feature on the wasm target; uuid's `getrandom`/`rand` dependencies are non-wasm-only, so `js` is the only viable option.

### 3. `Send` bounds are target-dependent

Traits that carry an HTTP client are cfg-gated, because reqwest's browser backend returns `!Send` futures (`JsFuture` wraps thread-local JS objects) while native servers (axum handlers) require `Send`:

| Trait | Crate | Native | wasm32 |
|---|---|---|---|
| `Tool` | `infinity-agent-core` | `Send + Sync`, `#[async_trait]` | no bounds, `#[async_trait(?Send)]` |
| `HttpClient` | `rap-client` | `Send + Sync + Clone`, `#[async_trait]` | `Clone`, `#[async_trait(?Send)]` |
| `CallbackClient` | `rap-protocol` | `Send + Sync + 'static`, `#[async_trait]` | `'static`, `#[async_trait(?Send)]` |

Any new implementation must use the matching attribute pair, or it fails with `E0053: method ... has an incompatible type for trait`:

```rust
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Tool<M> for MyTool { /* ... */ }
```

### 4. No socket, no process, no clock (yet)

- A browser cannot bind a socket: `rap_client::callback_server` is gated to native. In-browser callback ingestion is an in-page seam (implementation plan chunk 3.3).
- `std::fs` and `tokio::time` **compile** for wasm but fail at runtime (`std::fs` always errors; tokio timers panic on this target). `SliceTimer` (decision D18) is the Phase 3 replacement — until it lands, do not invoke `TokioSleepTool` / `TokioSleepUntilTool` / `SleepUntilEventOrInputTool` in a browser.
- `std::thread::spawn` panics; the wasm runtime is current-thread only.
- `McpTool::execute` delivers results through `tokio::task::spawn_local` on wasm. The surrounding runtime must therefore run inside a `tokio::task::LocalSet`; calling it outside one panics. This is wired up with the browser runtime in Phase 3.

---

## Verifying a wasm build

```bash
rustup target add wasm32-unknown-unknown
bash scripts/check-wasm.bash        # readiness gate (also run by check.bash)
cargo check --target wasm32-unknown-unknown -p sandbox-core --no-default-features
```

The gate fails in both directions: a crate that should compile but does not, and a crate listed as expected-red that starts passing (so the exception list in `scripts/check-wasm.bash` cannot silently rot).

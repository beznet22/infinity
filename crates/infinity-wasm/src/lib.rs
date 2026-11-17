//! Browser runtime for Infinity (`wasm32-unknown-unknown`).
//!
//! Phase 3 assembly point. The browser has no processes, no filesystem and no
//! OS trust store, so this crate pairs the shared core with the capabilities a
//! browser actually provides:
//!
//! | Capability | Browser implementation |
//! |---|---|
//! | HTTP | `reqwest` on the JS stack (`web_async`/`wasmtimer` timers) |
//! | Storage | IndexedDB (`idb`) — step 3.3 |
//! | Sleeps | `wasmtimer` over `performance.now()` — step 3.2 |
//! | Sandbox | a Wasm interpreter, since `exec` does not exist here |
//!
//! Steps 3.1–3.5 are tracked in `docs/docs/WASM/PROGRESS.md`.

/// Whether this build targets a browser.
pub const IS_BROWSER_BUILD: bool = cfg!(target_family = "wasm");

pub mod timer;

#[cfg(target_family = "wasm")]
pub mod conversation_store;
#[cfg(target_family = "wasm")]
pub mod state_store;

#[cfg(target_family = "wasm")]
pub mod browser;

pub use timer::{SliceTimer, browser_timer};

/// The sandbox backend this build can host.
///
/// A browser cannot spawn processes, so the process executor used on Android and
/// desktop has no counterpart. Saying so explicitly keeps host code honest: it
/// gets a stated capability, not a silent no-op.
pub fn sandbox_backend() -> &'static str {
    "none (no process spawning in a browser; Wasm interpreter planned)"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_backend_is_stated_not_implied() {
        assert!(sandbox_backend().contains("no process spawning"));
    }
}

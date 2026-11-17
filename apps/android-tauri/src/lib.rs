//! Consumer archetype: Infinity Hybrid on Android through Tauri v2.
//!
//! The point of this app is to prove the fork's crate API end to end on a
//! device: [`HostCtx`] is assembled from the host's own capability
//! implementations, and a probe exercises the sandbox executor. It is a
//! reference for consumers, not part of the fork's workspace (decision D16).

use infinity_agent_core::stores::{InMemoryConversationStore, InMemoryStateStore};
use infinity_host::HostCtx;
use rap_client::http::{InMemoryToolsetCache, SimpleHttpClient};
use sandbox_core::executor::{ExecRequest, ProcessSandboxExecutor, SandboxExecutor};
use serde::Serialize;
use tauri::Manager;

/// The bundle this host provides to the core.
type AndroidHost = HostCtx<
    InMemoryConversationStore,
    InMemoryStateStore,
    SimpleHttpClient,
    InMemoryToolsetCache,
    ProcessSandboxExecutor,
>;

/// Assemble the host capabilities.
///
/// Stores and caches are in-memory for the archetype. A production app swaps
/// in durable implementations (the daemon crate's `PersistentConversationStore`
/// and `PersistentStateStore`, pointed at the app's data directory) without
/// touching the rest of the wiring.
fn build_host() -> AndroidHost {
    let sandbox_root = std::env::temp_dir().join("infinity-hybrid-sandbox");
    HostCtx::new(
        InMemoryConversationStore::new(),
        InMemoryStateStore::new(),
        SimpleHttpClient::new(),
        InMemoryToolsetCache::new(),
        ProcessSandboxExecutor::new(sandbox_root),
    )
}

/// Result of the on-device health probe.
#[derive(Serialize)]
pub struct HostHealth {
    /// Crate version reported by the app.
    pub app_version: &'static str,
    /// Target architecture the app was compiled for.
    pub target: &'static str,
    /// Exit status of the sandbox probe (`0` means success).
    pub sandbox_status: i32,
    /// Captured stdout of the sandbox probe.
    pub sandbox_stdout: String,
}

/// Run a command through the sandbox executor and report the result.
///
/// This is the smallest end-to-end proof that the fork's crates work on
/// Android: the host bundle is assembled, and the sandbox executor spawns a
/// real process inside the app.
async fn probe() -> Result<HostHealth, String> {
    let host = build_host();
    let output = host
        .sandbox()
        .run(ExecRequest::new(["sh", "-c", "echo infinity-on-android"]))
        .await
        .map_err(|error| error.to_string())?;

    Ok(HostHealth {
        app_version: env!("CARGO_PKG_VERSION"),
        target: std::env::consts::ARCH,
        sandbox_status: output.status,
        sandbox_stdout: output.stdout_text().trim().to_owned(),
    })
}

/// Tauri command: run the probe on demand (used by the UI button).
#[tauri::command]
async fn host_health() -> Result<HostHealth, String> {
    probe().await
}

/// Entry point shared by the desktop binary and the mobile library.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Run the probe once at startup and persist the result inside the app's
        // data dir. On a device this is readable without a UI round-trip:
        //   adb shell run-as run.hydro.infinityhybrid cat files/probe.json
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let payload = match probe().await {
                    Ok(health) => serde_json::to_string_pretty(&health)
                        .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}")),
                    Err(error) => serde_json::json!({ "error": error }).to_string(),
                };
                match handle.path().app_data_dir() {
                    Ok(dir) => {
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = std::fs::write(dir.join("probe.json"), payload);
                    }
                    Err(error) => eprintln!("no app data dir: {error}"),
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![host_health])
        .run(tauri::generate_context!())
        .expect("bug: failed to run the Tauri application");
}

//! Consumer archetype: Infinity Hybrid on Android through Tauri v2.
//!
//! The point of this app is to prove the fork's crate API end to end on a
//! device: [`HostCtx`] is assembled from the host's own capability
//! implementations, a probe exercises the sandbox executor, and an optional
//! chat probe runs a real model turn through the fork's provider stack. It is a
//! reference for consumers, not part of the fork's workspace (decision D16).

use std::sync::Arc;

use infinity_agent_core::stores::{InMemoryConversationStore, InMemoryStateStore};
use infinity_agent_core::system::{AgentEvent, AgentSystemBuilder, StaticModel};
use infinity_host::HostCtx;
use infinity_provider_protocol::{ModelEntry, ModelProvider};
use infinity_provider_rig::RigCompletionModel;
use rap_client::http::{InMemoryToolsetCache, SimpleHttpClient};
use rig::client::CompletionClient;
use sandbox_core::executor::{ExecRequest, ProcessSandboxExecutor, SandboxExecutor};
use serde::{Deserialize, Serialize};
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

/// Configuration for the on-device conversation probe.
///
/// Pushed into the app's data dir by the developer (`chat-config.json`) — it
/// holds an API key, so it is never committed, never bundled into the APK, and
/// deleted before shipping.
#[derive(Deserialize)]
struct ChatConfig {
    /// OpenAI-compatible base URL, e.g. `https://llm.kimchi.dev/openai/v1`.
    base_url: String,
    /// Provider key (runtime-only).
    api_key: String,
    /// Provider-scoped model id, e.g. `gemma-4-31b`.
    model: String,
    /// Prompt to send.
    prompt: String,
}

/// Result of the conversation probe.
#[derive(Serialize)]
struct ChatOutcome {
    model: String,
    reply: String,
    /// Runtime diagnostics (retries, provider errors) surfaced as `Info` events.
    notes: Vec<String>,
    /// Whether the turn was still running when the probe gave up.
    timed_out: bool,
}

/// Builds an HTTP client whose TLS trust anchors are compiled into the binary.
///
/// reqwest's `rustls` feature always routes certificate verification through
/// `rustls-platform-verifier`. On Android that backend needs JNI initialization
/// plus a Kotlin AAR packaged in the APK; without them the TLS handshake stalls
/// with no error and no diagnostics. Verifying against the embedded Mozilla
/// roots keeps the handshake pure Rust and identical on every platform, which
/// also matches the browser target where no OS trust store is available.
fn portable_http_client() -> Result<reqwest::Client, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("configure TLS versions: {error}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();

    reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .build()
        .map_err(|error| format!("build the HTTP client: {error}"))
}

/// Run one real model turn through the fork's provider stack.
async fn chat_probe(dir: &std::path::Path) -> Result<ChatOutcome, String> {
    let config_path = dir.join("chat-config.json");
    let raw = std::fs::read_to_string(&config_path)
        .map_err(|error| format!("read {}: {error}", config_path.display()))?;
    let config: ChatConfig = serde_json::from_str(&raw).map_err(|error| error.to_string())?;

    // Kimchi (and OpenRouter) speak the OpenAI *chat completions* API, so build
    // rig's completions client rather than the Responses-API default. The
    // concrete HTTP client type is named explicitly because the builder's type
    // parameter is not inferable at the call site.
    let http: reqwest::Client = portable_http_client()?;
    let client: rig::providers::openai::CompletionsClient<reqwest::Client> =
        rig::providers::openai::CompletionsClient::<reqwest::Client>::builder()
            .api_key(&config.api_key)
            .base_url(&config.base_url)
            .http_client(http)
            .build()
            .map_err(|error| error.to_string())?;

    let model = client.completion_model(&config.model);
    let entry = ModelEntry {
        model_id: config.model.clone(),
        display_name: config.model.clone(),
        context_window: 8_192,
        max_output_tokens: Some(512),
        supports_image_input: false,
    };
    let provider: Arc<dyn ModelProvider> =
        Arc::new(RigCompletionModel::new(model).into_provider(entry));

    let system = AgentSystemBuilder::new_local(
        InMemoryConversationStore::new(),
        InMemoryStateStore::new(),
        StaticModel::new(provider, &config.model)
            .await
            .map_err(|error| error.to_string())?,
    )
    .start();

    let mut thread = system.thread_builder().launch().await;
    thread
        .send_user_text(config.prompt.clone())
        .await
        .map_err(|error| error.to_string())?;

    let mut reply = String::new();
    let mut notes = Vec::new();
    let mut timed_out = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
    loop {
        match tokio::time::timeout_at(deadline, thread.recv()).await {
            Ok(Some(AgentEvent::TextChunk { text })) => reply.push_str(&text),
            Ok(Some(AgentEvent::CompletionFinished { .. })) => break,
            Ok(Some(AgentEvent::Info { text })) => {
                eprintln!("agent info: {text}");
                notes.push(text);
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {
                timed_out = true;
                break;
            }
        }
    }

    Ok(ChatOutcome {
        model: config.model,
        reply,
        notes,
        timed_out,
    })
}

/// Entry point shared by the desktop binary and the mobile library.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Run the probes once at startup and persist their results inside the
        // app's data dir. On a device they are readable without a UI round-trip:
        //   adb shell run-as run.hydro.infinityhybrid cat probe.json
        //   adb shell run-as run.hydro.infinityhybrid cat chat.json
        // (verified on a real device: `app_data_dir()` resolves to the app's
        // data root, so the files are `probe.json` / `chat.json`.)
        .setup(|app| {
            // The agent system is deliberately `!Send` (Rc-based, LocalSet-bound),
            // so it cannot run on Tauri's Send-bound async runtime. Give it a
            // dedicated current-thread runtime on its own thread.
            let dir = match app.path().app_data_dir() {
                Ok(dir) => dir,
                Err(error) => {
                    eprintln!("no app data dir: {error}");
                    return Ok(());
                }
            };
            let _ = std::fs::create_dir_all(&dir);

            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("bug: failed to build the agent runtime");
                let local = tokio::task::LocalSet::new();
                local.block_on(&runtime, async move {
                    let payload = match probe().await {
                        Ok(health) => serde_json::to_string_pretty(&health)
                            .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}")),
                        Err(error) => serde_json::json!({ "error": error }).to_string(),
                    };
                    let _ = std::fs::write(dir.join("probe.json"), payload);

                    // The conversation probe only runs when a config was pushed.
                    if dir.join("chat-config.json").exists() {
                        // Bound the probe: a provider error can otherwise be
                        // retried indefinitely, leaving no result file behind.
                        let outcome = tokio::time::timeout(
                            std::time::Duration::from_secs(60),
                            chat_probe(&dir),
                        )
                        .await;
                        let payload = match outcome {
                            Ok(Ok(outcome)) => serde_json::to_string_pretty(&outcome)
                                .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}")),
                            Ok(Err(error)) => serde_json::json!({ "error": error }).to_string(),
                            Err(_) => {
                                serde_json::json!({ "error": "timeout after 60s" }).to_string()
                            }
                        };
                        let _ = std::fs::write(dir.join("chat.json"), payload);
                    }
                });
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![host_health])
        .run(tauri::generate_context!())
        .expect("bug: failed to run the Tauri application");
}

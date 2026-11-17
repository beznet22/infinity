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
use infinity_sync::EventStore;
use tauri::Manager;
use tauri_plugin_infinity_android::InfinityAndroidExt;

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
fn build_host() -> Result<AndroidHost, String> {
    let sandbox_root = std::env::temp_dir().join("infinity-hybrid-sandbox");
    Ok(HostCtx::new(
        InMemoryConversationStore::new(),
        InMemoryStateStore::new(),
        // Not `SimpleHttpClient::new()`: reqwest's default TLS stack stalls on
        // Android (D21/D34), so the host owns the client.
        SimpleHttpClient::with_client(portable_http_client()?),
        InMemoryToolsetCache::new(),
        ProcessSandboxExecutor::new(sandbox_root),
    ))
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
    let host = build_host()?;
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
    /// `acquired`/`released` on a normal run, `denied` when another device holds
    /// the session.
    lease: String,
    /// Queued messages this turn took from the log, in order.
    #[serde(default)]
    drained: usize,
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

    // One device per session: claim it before touching the model. The event log
    // lives under the app's data dir, so the claim is visible to other devices
    // once it is pushed.
    let device = read_sync_device(dir);
    let session = infinity_sync::SessionId::new("android-chat");
    let lease_store = infinity_sync::SqliteEventStore::open(dir.join("sync/events.sqlite"))
        .map_err(|error| format!("open store: {error}"))?;
    let relay = read_sync_relay(dir);

    // Pull first: the claim must be judged against what other devices already
    // wrote, not just against this device's local log.
    if let Some(relay) = &relay {
        pull_session(&lease_store, &dir.join("sync"), relay, &device, &session).await?;
    }

    let guard = infinity_sync::LeaseGuard::acquire(
        &lease_store,
        session.clone(),
        device.clone(),
        infinity_sync::DEFAULT_TTL_MS,
    )
    .await
    .map_err(|error| format!("lease: {error}"))?;

    let Some(mut guard) = guard else {
        return Ok(ChatOutcome {
            model: config.model,
            reply: String::new(),
            notes: vec!["another device holds this session".to_owned()],
            timed_out: false,
            lease: "denied".to_owned(),
            drained: 0,
        });
    };

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

    // Keep the lease alive while any turn runs. The interval is shorter than the
    // TTL so two missed beats still leave the claim valid; the foreground
    // service (started by the caller) is what keeps this ticking when the app is
    // backgrounded.
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(5));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await; // the first tick completes immediately; skip it

    // Anything the user queued while the phone was busy is delivered first, in
    // the order it was written. Each message becomes a MessageSent event and its
    // answer is appended to the log, so other devices see the whole exchange.
    let mut drained = 0usize;
    let mut queue_replies: Vec<String> = Vec::new();
    while drained < 5 {
        let events = lease_store
            .events_since(&session, 0)
            .await
            .map_err(|error| error.to_string())?;
        let Some(entry) = infinity_sync::queue(&events).into_iter().next() else {
            break;
        };

        let seq = lease_store
            .last_seq(&session)
            .await
            .map_err(|error| error.to_string())?
            + 1;
        let sent = infinity_sync::send_event(
            session.clone(),
            device.clone(),
            seq,
            entry.event_id,
        )
        .with_fence(guard.fence());
        lease_store
            .append(&sent)
            .await
            .map_err(|error| error.to_string())?;

        let (text, _, _) = run_prompt(
            &mut thread,
            entry.text.clone(),
            &mut guard,
            &lease_store,
            45,
            &mut heartbeat,
        )
        .await?;

        let seq = lease_store
            .last_seq(&session)
            .await
            .map_err(|error| error.to_string())?
            + 1;
        let answer = infinity_sync::message_event(
            session.clone(),
            device.clone(),
            seq,
            infinity_sync::EventKind::AssistantMessage,
            text.clone(),
            Some(entry.event_id),
            None,
        )
        .with_fence(guard.fence());
        lease_store
            .append(&answer)
            .await
            .map_err(|error| error.to_string())?;

        queue_replies.push(text);
        drained += 1;
    }

    let mut reply = String::new();
    let mut notes = Vec::new();
    let mut timed_out = false;

    if drained == 0 {
        // Nothing was queued, so the probe runs its configured prompt.
        let (text, info, timed) = run_prompt(
            &mut thread,
            config.prompt.clone(),
            &mut guard,
            &lease_store,
            45,
            &mut heartbeat,
        )
        .await?;
        reply = text;
        notes = info;
        timed_out = timed;
    } else if let Some(last) = queue_replies.last() {
        // A queued run already produced the answer; report it rather than asking
        // the same question twice.
        reply = last.clone();
    }

    // Publish the claim immediately, so a second device that starts now is
    // refused rather than racing us.
    if let Some(relay) = &relay {
        push_session(&lease_store, relay, &session).await?;
    }

    if read_hold_lease(dir) {
        // Two-device test: stay the holder instead of releasing.
        return Ok(ChatOutcome {
            model: config.model,
            reply,
            notes,
            timed_out,
            lease: "held".to_owned(),
            drained,
        });
    }

    guard
        .release(&lease_store)
        .await
        .map_err(|error| format!("lease release: {error}"))?;
    if let Some(relay) = &relay {
        push_session(&lease_store, relay, &session).await?;
    }

    Ok(ChatOutcome {
        model: config.model,
        reply,
        notes,
        timed_out,
        lease: "released".to_owned(),
        drained,
    })
}

fn read_sync_config(dir: &std::path::Path) -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(dir.join("sync-config.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

fn read_sync_relay(dir: &std::path::Path) -> Option<String> {
    read_sync_config(dir)?.get("relay_url")?.as_str().map(str::to_owned)
}

/// Run one prompt on the thread, renewing the lease while it works.
///
/// Returns `(reply, info notes, timed out)`. The caller owns the deadline policy
/// (the probe uses 45 s per turn).
async fn run_prompt(
    thread: &mut infinity_agent_core::system::local::ThreadHandle,
    prompt: String,
    guard: &mut infinity_sync::LeaseGuard,
    lease_store: &infinity_sync::SqliteEventStore,
    timeout_secs: u64,
    heartbeat: &mut tokio::time::Interval,
) -> Result<(String, Vec<String>, bool), String> {
    thread
        .send_user_text(prompt)
        .await
        .map_err(|error| error.to_string())?;

    let mut reply = String::new();
    let mut notes = Vec::new();
    let mut timed_out = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if let Err(error) = guard.renew(lease_store).await {
                    eprintln!("lease renew failed: {error}");
                }
            }
            received = tokio::time::timeout_at(deadline, thread.recv()) => match received {
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
            },
        }
    }

    Ok((reply, notes, timed_out))
}

fn read_hold_lease(dir: &std::path::Path) -> bool {
    read_sync_config(dir)
        .and_then(|config| config.get("hold_lease").and_then(|value| value.as_bool()))
        .unwrap_or(false)
}

/// Pull the session's events into the local log and advance the cursor.
async fn pull_session(
    store: &infinity_sync::SqliteEventStore,
    root: &std::path::Path,
    relay: &str,
    device: &infinity_sync::DeviceId,
    session: &infinity_sync::SessionId,
) -> Result<(), String> {
    use infinity_sync::{CursorStore, EventStore};

    let transport = infinity_sync::http::HttpTransport::with_client(
        portable_http_client()?,
        relay.to_owned(),
    );
    let cursors = CursorStore::new(root, device);
    let after = cursors
        .load()
        .await
        .map_err(|error| error.to_string())?
        .get(session.as_str())
        .copied()
        .unwrap_or(0);

    let stored = transport
        .pull(session, after)
        .await
        .map_err(|error| error.to_string())?;
    let mut highest = after;
    for item in &stored {
        store.append(&item.event).await.map_err(|error| error.to_string())?;
        highest = highest.max(item.position);
    }
    cursors
        .set(session, highest)
        .await
        .map_err(|error| error.to_string())
}

/// Push everything this device knows for the session (the relay dedupes).
async fn push_session(
    store: &infinity_sync::SqliteEventStore,
    relay: &str,
    session: &infinity_sync::SessionId,
) -> Result<(), String> {
    use infinity_sync::EventStore;

    let transport = infinity_sync::http::HttpTransport::with_client(
        portable_http_client()?,
        relay.to_owned(),
    );
    let events = store
        .events_since(session, 0)
        .await
        .map_err(|error| error.to_string())?;
    transport
        .push(&events)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Device id from `sync-config.json`, or a stable default.
fn read_sync_device(dir: &std::path::Path) -> infinity_sync::DeviceId {
    std::fs::read_to_string(dir.join("sync-config.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|config| {
            config
                .get("device_id")
                .and_then(|value| value.as_str())
                .map(|value| value.to_owned())
        })
        .map(infinity_sync::DeviceId::new)
        .unwrap_or_else(|| infinity_sync::DeviceId::new("android"))
}

/// One sync round-trip: write a phone event, push it, then pull everything the
/// relay has for the session. Mirrors what the CLI does for the laptop.
#[cfg(target_os = "android")]
async fn sync_probe(dir: &std::path::Path) -> Result<serde_json::Value, String> {
    use infinity_sync::http::HttpTransport;
    use infinity_sync::{
        CursorStore, DeviceId, EventKind, EventStore, SessionId, SqliteEventStore, SyncEvent,
        now_ms,
    };

    let raw = std::fs::read_to_string(dir.join("sync-config.json"))
        .map_err(|error| format!("read sync-config.json: {error}"))?;
    let config: serde_json::Value =
        serde_json::from_str(&raw).map_err(|error| error.to_string())?;

    let relay = config
        .get("relay_url")
        .and_then(|value| value.as_str())
        .ok_or("sync-config.json needs a relay_url")?;
    let device = DeviceId::new(
        config
            .get("device_id")
            .and_then(|value| value.as_str())
            .unwrap_or("android"),
    );
    let session = SessionId::new(
        config
            .get("session")
            .and_then(|value| value.as_str())
            .unwrap_or("s1"),
    );

    let root = dir.join("sync");
    let store = SqliteEventStore::open(root.join("sync/events.sqlite"))
        .map_err(|error| format!("open store: {error}"))?;
    // Reuse the app's portable TLS client: reqwest's default stack stalls on
    // Android (D21), and the sync probe must not repeat that bug.
    let transport = HttpTransport::with_client(
        portable_http_client()?,
        relay.to_string(),
    );

    let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
    let event = SyncEvent::new(
        session.clone(),
        device.clone(),
        seq,
        EventKind::UserMessage,
        serde_json::json!({ "text": "hello from android", "at": now_ms() }),
    );
    store.append(&event).await.map_err(|e| e.to_string())?;
    let pushed = transport
        .push(std::slice::from_ref(&event))
        .await
        .map_err(|e| e.to_string())?;

    let cursors = CursorStore::new(&root, &device);
    let after = cursors
        .load()
        .await
        .map_err(|e| e.to_string())?
        .get(session.as_str())
        .copied()
        .unwrap_or(0);
    let stored = transport
        .pull(&session, after)
        .await
        .map_err(|e| e.to_string())?;
    for item in &stored {
        store.append(&item.event).await.map_err(|e| e.to_string())?;
    }
    let highest = stored.iter().map(|item| item.position).max().unwrap_or(after);
    cursors
        .set(&session, highest)
        .await
        .map_err(|e| e.to_string())?;
    let local_events = store
        .events_since(&session, 0)
        .await
        .map_err(|e| e.to_string())?
        .len();

    Ok(serde_json::json!({
        "device": device.as_str(),
        "session": session.as_str(),
        "pushed": pushed.accepted,
        "fetched": stored.len(),
        "position": highest,
        "local_events": local_events,
    }))
}

/// Entry point shared by the desktop binary and the mobile library.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Android-only capabilities (contacts, Shizuku). On desktop every command
        // reports that it is unsupported instead of pretending to work.
        .plugin(tauri_plugin_infinity_android::init())
        .plugin(tauri_plugin_notification::init())
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

            // Android-only capabilities: the Kotlin bridge (contacts, Shizuku) and
            // the official notification plugin. Results go to `native.json` so a
            // device run can be checked without touching the UI:
            //   adb shell run-as run.hydro.infinityhybrid cat native.json
            // It runs off the setup path because `count_contacts` may wait for a
            // runtime permission answer, which must not block app startup.
            #[cfg(target_os = "android")]
            {
                use tauri_plugin_infinity_android::InfinityAndroidExt;
                use tauri_plugin_notification::NotificationExt;

                let handle = app.handle().clone();
                let dir = dir.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let contacts = handle.infinity_android().count_contacts();
                    let shizuku = handle.infinity_android().shizuku_status();
                    let notification = handle
                        .notification()
                        .builder()
                        .title("Infinity Hybrid")
                        .body("on-device notification probe")
                        .show();

                    let payload = serde_json::json!({
                        "contacts": match contacts {
                            Ok(count) => serde_json::to_value(count)
                                .unwrap_or(serde_json::Value::Null),
                            Err(error) => serde_json::json!({ "error": error.to_string() }),
                        },
                        "shizuku": match shizuku {
                            Ok(status) => serde_json::to_value(status)
                                .unwrap_or(serde_json::Value::Null),
                            Err(error) => serde_json::json!({ "error": error.to_string() }),
                        },
                        "notification": match notification {
                            Ok(()) => serde_json::json!("shown"),
                            Err(error) => serde_json::json!(format!("error: {error}")),
                        },
                    });
                    let _ = std::fs::write(
                        dir.join("native.json"),
                        serde_json::to_string_pretty(&payload).unwrap_or_default(),
                    );

                    // Opt-in: the WhatsApp WebView only runs when this marker exists.
                    // It is hidden behind the UI, but it is still a live client.
                    if dir.join("whatsapp-probe").exists() {
                        let probe = handle.infinity_android().whatsapp_web_probe();
                        let payload = match probe {
                            Ok(probe) => {
                                serde_json::to_value(probe).unwrap_or(serde_json::Value::Null)
                            }
                            Err(error) => serde_json::json!({ "error": error.to_string() }),
                        };
                        let _ = std::fs::write(
                            dir.join("whatsapp.json"),
                            serde_json::to_string_pretty(&payload).unwrap_or_default(),
                        );
                    }

                    // Pairing needs a phone number the user supplies at runtime
                    // (`whatsapp-config.json`), never one baked into the app.
                    if dir.join("whatsapp-pair").exists() {
                        let paired = std::fs::read_to_string(dir.join("whatsapp-config.json"))
                            .ok()
                            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                            .and_then(|config| {
                                config
                                    .get("phone")
                                    .and_then(|phone| phone.as_str())
                                    .map(str::to_string)
                            });

                        let payload = match paired {
                            Some(phone) => match handle.infinity_android().whatsapp_pair(&phone) {
                                Ok(outcome) => {
                                    serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null)
                                }
                                Err(error) => serde_json::json!({ "error": error.to_string() }),
                            },
                            None => serde_json::json!({
                                "error": "whatsapp-config.json is missing a phone number"
                            }),
                        };
                        let _ = std::fs::write(
                            dir.join("whatsapp-pair.json"),
                            serde_json::to_string_pretty(&payload).unwrap_or_default(),
                        );
                    }
                });
            }

            // The probe thread runs turns, so it needs the Android handle for the
            // foreground service (start/stop around the chat probe).
            let probe_handle = app.handle().clone();
            std::thread::spawn(move || {
                let handle = probe_handle;
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

                    // Tool isolation proof, run on every launch: a plain tool, one
                    // that wants WASI, and one that never returns.
                    let report: serde_json::Map<String, serde_json::Value> =
                        infinity_wasm_sandbox::self_test()
                            .into_iter()
                            .map(|(name, outcome)| {
                                let value = match outcome {
                                    Ok(value) => serde_json::json!({ "ok": value }),
                                    Err(error) => serde_json::json!({ "denied": error }),
                                };
                                (name.to_owned(), value)
                            })
                            .collect();
                    let sandbox = serde_json::to_string_pretty(&report)
                        .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"));
                    let _ = std::fs::write(dir.join("sandbox.json"), sandbox);

                    // The conversation probe only runs when a config was pushed.
                    if dir.join("chat-config.json").exists() {
                        // Reliability layer: keeps the process (and the lease
                        // heartbeat) alive if the user leaves the app.
                        let service_started = handle
                            .infinity_android()
                            .start_foreground_service()
                            .is_ok();
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
                        if service_started {
                            let _ = handle.infinity_android().stop_foreground_service();
                        }
                    }

                    // Session sync: opt-in via sync-config.json, which carries the
                    // relay URL and this device's id (never baked into the APK).
                    if dir.join("sync-config.json").exists() {
                        // Never let a stuck handshake keep the result file from
                        // being written.
                        let outcome = tokio::time::timeout(
                            std::time::Duration::from_secs(45),
                            sync_probe(&dir),
                        )
                        .await
                        .unwrap_or_else(|_| Err("sync probe timed out after 45s".to_string()));
                        let payload = match outcome {
                            Ok(value) => serde_json::to_string_pretty(&value)
                                .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}")),
                            Err(error) => serde_json::json!({ "error": error }).to_string(),
                        };
                        let _ = std::fs::write(dir.join("sync.json"), payload);
                    }
                });
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![host_health])
        .run(tauri::generate_context!())
        .expect("bug: failed to run the Tauri application");
}

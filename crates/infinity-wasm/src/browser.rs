//! Browser entry points: run a real agent turn inside the page.
//!
//! Everything below is the fork's own code — `AgentSystemBuilder`, the local
//! driver, the thread API and the provider trait — compiled to
//! `wasm32-unknown-unknown`. Two providers are offered: a deterministic mock
//! (no network) and a generic **OpenAI-compatible** provider, so any endpoint
//! that speaks `/chat/completions` works (OpenAI, OpenRouter, Kimchi, a local
//! server) as long as it allows browser CORS.

use std::sync::Arc;

use async_trait::async_trait;
use infinity_agent_core::stores::{InMemoryConversationStore, InMemoryStateStore};
use infinity_agent_core::system::{AgentEvent, AgentSystemBuilder, StaticModel};
use infinity_provider_protocol::{
    AssistantContent, CompletionError, CompletionRequest, ErrorClass, FinalResponse, Message,
    ModelEntry, ModelProvider, ModelStream, StreamChunk, UserContent,
};
use serde_json::json;
use wasm_bindgen::prelude::*;

/// The text the stand-in model produces. The page asserts on this value.
const MOCK_REPLY: &str = "infinity-wasm-turn-ok";
const MOCK_MODEL_ID: &str = "mock-echo";

type BoxError = Box<dyn std::error::Error + Send + Sync>;

struct MockProvider;

#[async_trait]
impl ModelProvider for MockProvider {
    async fn list_models(&self) -> Result<Vec<ModelEntry>, BoxError> {
        Ok(vec![ModelEntry {
            model_id: MOCK_MODEL_ID.to_string(),
            display_name: "Mock echo".to_string(),
            context_window: 8_192,
            max_output_tokens: Some(512),
            supports_image_input: false,
        }])
    }

    async fn invoke_model(
        &self,
        _model_id: &str,
        _request: CompletionRequest,
    ) -> Result<ModelStream, CompletionError> {
        Ok(Box::pin(futures_util::stream::iter([
            Ok(StreamChunk::Text(MOCK_REPLY.to_string())),
            Ok(StreamChunk::Final(FinalResponse { usage: None })),
        ])))
    }
}

/// Talks to any OpenAI-compatible endpoint from the browser.
///
/// Base URL, key and model all arrive at call time and only live in memory on
/// this page; nothing is bundled, logged, or persisted by this crate. Providers
/// that do not send CORS headers cannot be used directly — they need a relay
/// (see the Worker archetype).
struct OpenAiCompatibleProvider {
    base_url: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    /// `/chat/completions` appended to the caller's base URL, with the trailing
    /// slash normalised so `…/v1` and `…/v1/` behave the same.
    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

fn render_user(content: &[UserContent]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            UserContent::Text(text) => Some(text.text.clone()),
            UserContent::ToolResult(result) => Some(format!("[tool result] {result:?}")),
            UserContent::Image(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_assistant(content: &[AssistantContent]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            AssistantContent::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleProvider {
    async fn list_models(&self) -> Result<Vec<ModelEntry>, BoxError> {
        Ok(vec![ModelEntry {
            model_id: self.model.clone(),
            display_name: self.model.clone(),
            context_window: 131_072,
            max_output_tokens: Some(1_024),
            supports_image_input: false,
        }])
    }

    async fn invoke_model(
        &self,
        _model_id: &str,
        request: CompletionRequest,
    ) -> Result<ModelStream, CompletionError> {
        let mut messages = Vec::new();
        if let Some(preamble) = &request.preamble {
            messages.push(json!({ "role": "system", "content": preamble }));
        }
        for message in &request.chat_history {
            match message {
                Message::User { content } => {
                    messages.push(json!({ "role": "user", "content": render_user(content) }))
                }
                Message::Assistant { content } => messages.push(
                    json!({ "role": "assistant", "content": render_assistant(content) }),
                ),
            }
        }

        let mut body = json!({ "model": self.model, "messages": messages });
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }

        // `async_trait` requires a `Send` future, but reqwest's wasm response is
        // `!Send` (it wraps a JS promise). Run the fetch on the current
        // `LocalSet` and hand the result back through a channel, so nothing
        // `!Send` is held across this method's await points.
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let endpoint = self.endpoint();
        wasm_bindgen_futures::spawn_local(async move {
            let outcome = async {
                let response = client
                    .post(&endpoint)
                    .bearer_auth(&api_key)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|error| CompletionError::RequestError(Box::new(error)))?;

                let status = response.status();
                let raw = response
                    .text()
                    .await
                    .map_err(|error| CompletionError::RequestError(Box::new(error)))?;
                if !status.is_success() {
                    return Err(CompletionError::ProviderError {
                        message: format!("{status}: {raw}"),
                        class: if status.as_u16() == 429 {
                            ErrorClass::Throttled
                        } else {
                            ErrorClass::Fatal
                        },
                    });
                }

                let parsed: serde_json::Value =
                    serde_json::from_str(&raw).map_err(CompletionError::JsonError)?;
                Ok(parsed["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string())
            }
            .await;
            let _ = tx.send(outcome).await;
        });

        let content = rx
            .recv()
            .await
            .ok_or_else(|| CompletionError::ResponseError("request task dropped".to_string()))??;

        Ok(Box::pin(futures_util::stream::iter([
            Ok(StreamChunk::Text(content)),
            Ok(StreamChunk::Final(FinalResponse { usage: None })),
        ])))
    }
}

/// One turn through the fork's system, decoupled from the provider.
async fn run_turn(
    provider: Arc<dyn ModelProvider>,
    model_id: &str,
    prompt: String,
) -> Result<String, String> {
    let model = StaticModel::new(provider, model_id)
        .await
        .map_err(|error| format!("model setup: {error}"))?;

    let system = AgentSystemBuilder::new_local(
        InMemoryConversationStore::new(),
        InMemoryStateStore::new(),
        model,
    )
    .start();

    let mut thread = system.thread_builder().launch().await;
    thread
        .send_user_text(prompt)
        .await
        .map_err(|error| format!("send: {error}"))?;

    let mut reply = String::new();
    while let Some(event) = thread.recv().await {
        match event {
            AgentEvent::TextChunk { text } => reply.push_str(&text),
            AgentEvent::CompletionFinished { .. } => break,
            _ => {}
        }
    }
    Ok(reply)
}

/// Run one turn with the deterministic mock model (no network).
///
/// The local system is `!Send` by design (D15), so it runs inside a `LocalSet`
/// rather than a tokio runtime — there is no runtime driver in a browser.
#[wasm_bindgen]
pub async fn run_smoke_turn(prompt: String) -> Result<String, JsValue> {
    console_error_panic_hook::set_once();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(run_turn(Arc::new(MockProvider), MOCK_MODEL_ID, prompt))
        .await
        .map_err(|error| JsValue::from_str(&error))
}

/// Run one real turn against any OpenAI-compatible endpoint.
///
/// `base_url` is the API root without `/chat/completions` (e.g.
/// `https://api.openai.com/v1`, `https://openrouter.ai/api/v1`,
/// `https://llm.kimchi.dev/openai/v1`). The key is supplied by the host at
/// runtime and never bundled.
#[wasm_bindgen]
pub async fn run_openai_compatible_turn(
    base_url: String,
    api_key: String,
    model: String,
    prompt: String,
) -> Result<String, JsValue> {
    console_error_panic_hook::set_once();
    let provider = OpenAiCompatibleProvider {
        base_url,
        api_key,
        model: model.clone(),
        client: reqwest::Client::new(),
    };
    let local = tokio::task::LocalSet::new();
    local
        .run_until(run_turn(Arc::new(provider), &model, prompt))
        .await
        .map_err(|error| JsValue::from_str(&error))
}

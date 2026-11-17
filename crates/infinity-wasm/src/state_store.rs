//! Durable `StateStore` for the browser.
//!
//! IndexedDB handles are `!Send`, and the core's store traits require
//! `Send + Sync` on the store (D27). Wrapping the handle in `unsafe impl Send`
//! would be unsound, so instead the handle owns only an **mpsc sender** — which
//! is `Send + Sync` — and the IndexedDB connection lives in a task pinned to the
//! page's `LocalSet`. Every trait method is a message round-trip, so no `!Send`
//! future is ever held across an await in the caller.

use std::collections::HashSet;

use async_trait::async_trait;
use infinity_agent_core::system::UserChoice;
use infinity_agent_core::traits::StateStore;
use idb::DatabaseEvent;
use js_sys::JsString;
use rap_protocol::{ThreadId, ToolCallId};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use wasm_bindgen::JsValue;

const DB_NAME: &str = "infinity-agent";
const STORE: &str = "thread_state";

/// Errors surfaced by the browser state store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("indexeddb: {0}")]
    IndexedDb(String),
    #[error("serialization: {0}")]
    Serde(String),
    #[error("state actor is not running")]
    ActorGone,
}

impl StoreError {
    fn idb(error: impl std::fmt::Display) -> Self {
        Self::IndexedDb(error.to_string())
    }
}

/// Everything the core keeps per thread.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ThreadState {
    #[serde(default)]
    processed_ids: HashSet<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    subscriptions: Vec<String>,
    #[serde(default)]
    pending_choices: Vec<UserChoice>,
}

enum Op {
    GetProcessedIds,
    AddProcessedIds(Vec<String>),
    GetMetadata,
    SetMetadata(serde_json::Value),
    GetSubscriptions,
    AddSubscription(String),
    RemoveSubscription(String),
    AddChoice(UserChoice),
    RemoveChoice(String),
    GetChoices,
}

enum Reply {
    Ids(HashSet<String>),
    Metadata(Option<serde_json::Value>),
    Subscriptions(Vec<String>),
    Choices(Vec<UserChoice>),
    Unit,
}

struct Request {
    thread: String,
    op: Op,
    reply: oneshot::Sender<Result<Reply, StoreError>>,
}

/// A `Send + Sync` handle to a per-page IndexedDB actor.
#[derive(Clone)]
pub struct IndexedDbStateStore {
    tx: mpsc::Sender<Request>,
}

impl IndexedDbStateStore {
    /// Open the database and start the actor.
    ///
    /// Must be awaited inside a `LocalSet` (the actor is a local task), which is
    /// exactly where the browser runtime already runs turns.
    pub async fn open() -> Result<Self, StoreError> {
        let (tx, rx) = mpsc::channel(64);
        let (ready_tx, ready_rx) = oneshot::channel();

        wasm_bindgen_futures::spawn_local(async move {
            match open_db().await {
                Ok(db) => {
                    let _ = ready_tx.send(Ok(()));
                    run_actor(db, rx).await;
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            }
        });

        ready_rx.await.map_err(|_| StoreError::ActorGone)??;
        Ok(Self { tx })
    }

    async fn request(&self, thread: &str, op: Op) -> Result<Reply, StoreError> {
        let (reply, response) = oneshot::channel();
        self.tx
            .send(Request {
                thread: thread.to_string(),
                op,
                reply,
            })
            .await
            .map_err(|_| StoreError::ActorGone)?;
        response.await.map_err(|_| StoreError::ActorGone)?
    }
}

async fn open_db() -> Result<idb::Database, StoreError> {
    let factory = idb::Factory::new().map_err(StoreError::idb)?;
    let mut request = factory.open(DB_NAME, Some(1)).map_err(StoreError::idb)?;
    request.on_upgrade_needed(|event| {
        if let Ok(db) = event.database() {
            let _ = db.create_object_store(STORE, Default::default());
        }
    });
    request.await.map_err(StoreError::idb)
}

async fn run_actor(db: idb::Database, mut rx: mpsc::Receiver<Request>) {
    while let Some(request) = rx.recv().await {
        let result = apply(&db, &request.thread, request.op).await;
        let _ = request.reply.send(result);
    }
}

async fn apply(db: &idb::Database, thread: &str, op: Op) -> Result<Reply, StoreError> {
    let mut state = load(db, thread).await?;
    let mut dirty = false;

    let reply = match op {
        Op::GetProcessedIds => Reply::Ids(state.processed_ids.clone()),
        Op::AddProcessedIds(ids) => {
            for id in ids {
                dirty |= state.processed_ids.insert(id);
            }
            Reply::Unit
        }
        Op::GetMetadata => Reply::Metadata(state.metadata.clone()),
        Op::SetMetadata(metadata) => {
            state.metadata = Some(metadata);
            dirty = true;
            Reply::Unit
        }
        Op::GetSubscriptions => Reply::Subscriptions(state.subscriptions.clone()),
        Op::AddSubscription(id) => {
            if !state.subscriptions.contains(&id) {
                state.subscriptions.push(id);
                dirty = true;
            }
            Reply::Unit
        }
        Op::RemoveSubscription(id) => {
            let before = state.subscriptions.len();
            state.subscriptions.retain(|known| known != &id);
            dirty |= before != state.subscriptions.len();
            Reply::Unit
        }
        Op::AddChoice(choice) => {
            if !state.pending_choices.iter().any(|known| known.id == choice.id) {
                state.pending_choices.push(choice);
                dirty = true;
            }
            Reply::Unit
        }
        Op::RemoveChoice(id) => {
            let before = state.pending_choices.len();
            state.pending_choices.retain(|known| known.id.as_ref() != id);
            dirty |= before != state.pending_choices.len();
            Reply::Unit
        }
        Op::GetChoices => Reply::Choices(state.pending_choices.clone()),
    };

    if dirty {
        save(db, thread, &state).await?;
    }
    Ok(reply)
}

async fn load(db: &idb::Database, thread: &str) -> Result<ThreadState, StoreError> {
    let tx = db
        .transaction(&[STORE], idb::TransactionMode::ReadOnly)
        .map_err(StoreError::idb)?;
    let store = tx.object_store(STORE).map_err(StoreError::idb)?;
    // `get` resolves to `None` when the key is absent.
    let value = store
        .get(JsValue::from(JsString::from(thread)))
        .map_err(StoreError::idb)?
        .await
        .map_err(StoreError::idb)?;

    let Some(value) = value else {
        return Ok(ThreadState::default());
    };
    if value.is_undefined() || value.is_null() {
        return Ok(ThreadState::default());
    }
    serde_wasm_bindgen::from_value(value).map_err(|error| StoreError::Serde(error.to_string()))
}

async fn save(db: &idb::Database, thread: &str, state: &ThreadState) -> Result<(), StoreError> {
    let tx = db
        .transaction(&[STORE], idb::TransactionMode::ReadWrite)
        .map_err(StoreError::idb)?;
    let store = tx.object_store(STORE).map_err(StoreError::idb)?;
    let value = serde_wasm_bindgen::to_value(state)
        .map_err(|error| StoreError::Serde(error.to_string()))?;
    // `put` resolves with the record's key; only success matters here.
    store
        .put(&value, Some(&JsValue::from(JsString::from(thread))))
        .map_err(StoreError::idb)?
        .await
        .map_err(StoreError::idb)?;
    tx.commit().map_err(StoreError::idb)?.await.map_err(StoreError::idb)?;
    Ok(())
}

#[async_trait]
impl StateStore for IndexedDbStateStore {
    type Error = StoreError;

    async fn get_processed_ids(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<HashSet<String>, Self::Error> {
        match self.request(thread_id.as_ref(), Op::GetProcessedIds).await? {
            Reply::Ids(ids) => Ok(ids),
            _ => Err(StoreError::ActorGone),
        }
    }

    async fn add_processed_message_ids(
        &self,
        thread_id: &ThreadId<str>,
        message_ids: Vec<String>,
    ) -> Result<(), Self::Error> {
        self.request(thread_id.as_ref(), Op::AddProcessedIds(message_ids))
            .await
            .map(|_| ())
    }

    async fn get_metadata(
        &self,
        root_thread_id: &ThreadId<str>,
    ) -> Result<Option<serde_json::Value>, Self::Error> {
        match self.request(root_thread_id.as_ref(), Op::GetMetadata).await? {
            Reply::Metadata(metadata) => Ok(metadata),
            _ => Err(StoreError::ActorGone),
        }
    }

    async fn set_metadata(
        &self,
        root_thread_id: &ThreadId<str>,
        metadata: serde_json::Value,
    ) -> Result<(), Self::Error> {
        self.request(root_thread_id.as_ref(), Op::SetMetadata(metadata))
            .await
            .map(|_| ())
    }

    async fn get_active_subscriptions(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Vec<ToolCallId>, Self::Error> {
        match self.request(thread_id.as_ref(), Op::GetSubscriptions).await? {
            Reply::Subscriptions(ids) => Ok(ids.iter().map(|id| ToolCallId::new(id.clone())).collect()),
            _ => Err(StoreError::ActorGone),
        }
    }

    async fn add_active_subscription(
        &self,
        thread_id: &ThreadId<str>,
        tool_call_id: &ToolCallId<str>,
    ) -> Result<(), Self::Error> {
        self.request(
            thread_id.as_ref(),
            Op::AddSubscription(tool_call_id.as_ref().to_string()),
        )
        .await
        .map(|_| ())
    }

    async fn remove_active_subscription(
        &self,
        thread_id: &ThreadId<str>,
        tool_call_id: &ToolCallId<str>,
    ) -> Result<(), Self::Error> {
        self.request(
            thread_id.as_ref(),
            Op::RemoveSubscription(tool_call_id.as_ref().to_string()),
        )
        .await
        .map(|_| ())
    }

    async fn add_pending_user_choice(
        &self,
        thread_id: &ThreadId<str>,
        choice: UserChoice,
    ) -> Result<(), Self::Error> {
        self.request(thread_id.as_ref(), Op::AddChoice(choice))
            .await
            .map(|_| ())
    }

    async fn remove_pending_user_choice(
        &self,
        thread_id: &ThreadId<str>,
        choice_id: &rap_protocol::ChoiceId<str>,
    ) -> Result<(), Self::Error> {
        self.request(
            thread_id.as_ref(),
            Op::RemoveChoice(choice_id.as_ref().to_string()),
        )
        .await
        .map(|_| ())
    }

    async fn get_pending_user_choices(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Vec<UserChoice>, Self::Error> {
        match self.request(thread_id.as_ref(), Op::GetChoices).await? {
            Reply::Choices(choices) => Ok(choices),
            _ => Err(StoreError::ActorGone),
        }
    }
}

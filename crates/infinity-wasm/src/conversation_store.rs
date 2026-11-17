//! Durable conversation history for the browser.
//!
//! Same shape as [`crate::state_store`]: a `Send + Sync` handle owning only a
//! channel, with the IndexedDB connection living in a `LocalSet` task. The
//! semantics mirror `InMemoryConversationStore` method for method, so the core
//! cannot tell which backend it is talking to.

use async_trait::async_trait;
use idb::DatabaseEvent;
use infinity_agent_core::message::InfinityMessage;
use infinity_agent_core::traits::{ConversationStore, SpawnContext};
use js_sys::JsString;
use rap_protocol::{ThreadId, ToolCallId};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use wasm_bindgen::JsValue;

const DB_NAME: &str = "infinity-agent-history";
const STORE: &str = "threads";

/// Errors surfaced by the browser history store.
#[derive(Debug, thiserror::Error)]
pub enum ConversationError {
    #[error("indexeddb: {0}")]
    IndexedDb(String),
    #[error("serialization: {0}")]
    Serde(String),
    #[error("conversation actor is not running")]
    ActorGone,
}

impl ConversationError {
    fn idb(error: impl std::fmt::Display) -> Self {
        Self::IndexedDb(error.to_string())
    }
}

/// One thread, exactly the information the in-memory store keeps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ThreadRecord {
    #[serde(default)]
    messages: Vec<InfinityMessage>,
    /// Message ids travel with their messages in the memory store; kept here so
    /// the shape stays faithful if a backend ever needs them.
    #[serde(default)]
    message_ids: Vec<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    spawn_order: Option<i64>,
    #[serde(default)]
    spawn_tool_call: Option<String>,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    subscription_event: bool,
    #[serde(default)]
    compaction: bool,
    #[serde(default)]
    fresh_context: bool,
    /// (summary, up_to_order) in insertion order.
    #[serde(default)]
    summaries: Vec<(String, i64)>,
}

enum Op {
    EnsureRoot,
    Exists,
    LoadHistory {
        start: Option<i64>,
        up_to: Option<i64>,
    },
    Append(Vec<(InfinityMessage, String)>),
    Spawn {
        new_id: String,
        parent: String,
        tool_call: String,
        subscription_event: bool,
        context: SpawnContext,
    },
    IsClosed,
    Close,
    IsSubscriptionEvent,
    IsFreshContext,
    ParentInfo,
    AncestorChain,
    MarkCompaction,
    IsCompaction,
    SpawnOrder,
    SaveSummary {
        summary: String,
        up_to_order: i64,
    },
    LoadSummary {
        up_to_order: Option<i64>,
    },
}

enum Reply {
    History(Vec<InfinityMessage>),
    Bool(bool),
    ParentInfo(Option<(String, String)>),
    Ancestors(Vec<(String, i64)>),
    Order(Option<i64>),
    Summary(Option<(String, i64)>),
    Unit,
}

struct Request {
    thread: String,
    op: Op,
    reply: oneshot::Sender<Result<Reply, ConversationError>>,
}

/// `Send + Sync` handle to the page's conversation-history actor.
#[derive(Clone)]
pub struct IndexedDbConversationStore {
    tx: mpsc::Sender<Request>,
}

impl IndexedDbConversationStore {
    pub async fn open() -> Result<Self, ConversationError> {
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

        ready_rx.await.map_err(|_| ConversationError::ActorGone)??;
        Ok(Self { tx })
    }

    async fn request(&self, thread: &str, op: Op) -> Result<Reply, ConversationError> {
        let (reply, response) = oneshot::channel();
        self.tx
            .send(Request {
                thread: thread.to_string(),
                op,
                reply,
            })
            .await
            .map_err(|_| ConversationError::ActorGone)?;
        response.await.map_err(|_| ConversationError::ActorGone)?
    }
}

async fn open_db() -> Result<idb::Database, ConversationError> {
    let factory = idb::Factory::new().map_err(ConversationError::idb)?;
    let mut request = factory
        .open(DB_NAME, Some(1))
        .map_err(ConversationError::idb)?;
    request.on_upgrade_needed(|event| {
        if let Ok(db) = event.database() {
            // Both object stores are created here; `create_object_store` fails
            // harmlessly if a previous handler already made one.
            let _ = db.create_object_store(STORE, Default::default());
        }
    });
    request.await.map_err(ConversationError::idb)
}

async fn run_actor(db: idb::Database, mut rx: mpsc::Receiver<Request>) {
    while let Some(request) = rx.recv().await {
        let result = apply(&db, &request.thread, request.op).await;
        let _ = request.reply.send(result);
    }
}

async fn load(db: &idb::Database, thread: &str) -> Result<Option<ThreadRecord>, ConversationError> {
    let tx = db
        .transaction(&[STORE], idb::TransactionMode::ReadOnly)
        .map_err(ConversationError::idb)?;
    let store = tx.object_store(STORE).map_err(ConversationError::idb)?;
    let value = store
        .get(JsValue::from(JsString::from(thread)))
        .map_err(ConversationError::idb)?
        .await
        .map_err(ConversationError::idb)?;
    match value {
        Some(value) if !value.is_undefined() && !value.is_null() => {
            serde_wasm_bindgen::from_value(value)
                .map(Some)
                .map_err(|error| ConversationError::Serde(error.to_string()))
        }
        _ => Ok(None),
    }
}

async fn save(
    db: &idb::Database,
    thread: &str,
    record: &ThreadRecord,
) -> Result<(), ConversationError> {
    let tx = db
        .transaction(&[STORE], idb::TransactionMode::ReadWrite)
        .map_err(ConversationError::idb)?;
    let store = tx.object_store(STORE).map_err(ConversationError::idb)?;
    let value = serde_wasm_bindgen::to_value(record)
        .map_err(|error| ConversationError::Serde(error.to_string()))?;
    store
        .put(&value, Some(&JsValue::from(JsString::from(thread))))
        .map_err(ConversationError::idb)?
        .await
        .map_err(ConversationError::idb)?;
    tx.commit()
        .map_err(ConversationError::idb)?
        .await
        .map_err(ConversationError::idb)?;
    Ok(())
}

async fn apply(db: &idb::Database, thread: &str, op: Op) -> Result<Reply, ConversationError> {
    // Reads first: mutations load, change, and save.
    match op {
        Op::Exists => return Ok(Reply::Bool(load(db, thread).await?.is_some())),
        Op::LoadHistory { start, up_to } => {
            let record = load(db, thread).await?.unwrap_or_default();
            let begin = start.unwrap_or(0).max(0) as usize;
            let end = up_to
                .map(|value| value.max(0) as usize)
                .unwrap_or(record.messages.len())
                .min(record.messages.len());
            let begin = begin.min(end);
            return Ok(Reply::History(record.messages[begin..end].to_vec()));
        }
        Op::AncestorChain => {
            let mut chain = Vec::new();
            let mut current = thread.to_string();
            loop {
                let Some(record) = load(db, &current).await? else {
                    break;
                };
                let Some(parent) = record.parent.clone() else {
                    break;
                };
                chain.push((parent.clone(), record.spawn_order.unwrap_or(0)));
                current = parent;
            }
            chain.reverse();
            return Ok(Reply::Ancestors(chain));
        }
        Op::ParentInfo => {
            let record = load(db, thread).await?;
            let info = record.and_then(|record| match (record.parent, record.spawn_tool_call) {
                (Some(parent), Some(tool_call)) => Some((parent, tool_call)),
                _ => None,
            });
            return Ok(Reply::ParentInfo(info));
        }
        Op::LoadSummary { up_to_order } => {
            let record = load(db, thread).await?.unwrap_or_default();
            let found = record
                .summaries
                .iter()
                .rev()
                .find(|(_, order)| up_to_order.map(|limit| *order <= limit).unwrap_or(true))
                .cloned();
            return Ok(Reply::Summary(found));
        }
        other => {
            let mut record = load(db, thread).await?.unwrap_or_default();
            let reply = match other {
                Op::EnsureRoot => {
                    if record.root.is_none() {
                        record.root = Some(thread.to_string());
                        save(db, thread, &record).await?;
                    }
                    Reply::Unit
                }
                Op::Append(messages) => {
                    for (message, id) in messages {
                        record.messages.push(message);
                        record.message_ids.push(id);
                    }
                    save(db, thread, &record).await?;
                    Reply::Unit
                }
                Op::Spawn {
                    new_id,
                    parent,
                    tool_call,
                    subscription_event,
                    context,
                } => {
                    let parent_record = load(db, &parent).await?.unwrap_or_default();
                    let (spawn_order, fresh_context) = match context {
                        SpawnContext::InheritUpTo(order) => (order as i64, false),
                        SpawnContext::Fresh => (0, true),
                    };
                    let child = ThreadRecord {
                        parent: Some(parent.clone()),
                        root: Some(parent_record.root.clone().unwrap_or_else(|| parent.clone())),
                        spawn_order: Some(spawn_order),
                        spawn_tool_call: Some(tool_call),
                        subscription_event,
                        fresh_context,
                        ..Default::default()
                    };
                    save(db, &new_id, &child).await?;
                    Reply::Unit
                }
                Op::IsClosed => Reply::Bool(record.closed),
                Op::Close => {
                    record.closed = true;
                    save(db, thread, &record).await?;
                    Reply::Unit
                }
                Op::IsSubscriptionEvent => Reply::Bool(record.subscription_event),
                Op::IsFreshContext => Reply::Bool(record.fresh_context),
                Op::MarkCompaction => {
                    record.compaction = true;
                    save(db, thread, &record).await?;
                    Reply::Unit
                }
                Op::IsCompaction => Reply::Bool(record.compaction),
                Op::SpawnOrder => Reply::Order(record.spawn_order),
                Op::SaveSummary {
                    summary,
                    up_to_order,
                } => {
                    record.summaries.push((summary, up_to_order));
                    save(db, thread, &record).await?;
                    Reply::Unit
                }
                _ => Reply::Unit,
            };
            Ok(reply)
        }
    }
}

fn key(value: &ThreadId<str>) -> String {
    value.as_ref().to_string()
}

#[async_trait]
impl ConversationStore for IndexedDbConversationStore {
    type Error = ConversationError;

    async fn ensure_root_thread(&self, thread_id: &ThreadId<str>) -> Result<(), Self::Error> {
        self.request(&key(thread_id), Op::EnsureRoot)
            .await
            .map(|_| ())
    }

    async fn thread_exists(&self, thread_id: &ThreadId<str>) -> Result<bool, Self::Error> {
        match self.request(&key(thread_id), Op::Exists).await? {
            Reply::Bool(value) => Ok(value),
            _ => Err(ConversationError::ActorGone),
        }
    }

    async fn load_history_up_to(
        &self,
        session_id: &ThreadId<str>,
        start_from: Option<i64>,
        up_to: Option<i64>,
    ) -> Result<Vec<InfinityMessage>, Self::Error> {
        match self
            .request(
                &key(session_id),
                Op::LoadHistory {
                    start: start_from,
                    up_to,
                },
            )
            .await?
        {
            Reply::History(messages) => Ok(messages),
            _ => Err(ConversationError::ActorGone),
        }
    }

    async fn append_messages(
        &self,
        session_id: &ThreadId<str>,
        messages: Vec<(InfinityMessage, String)>,
    ) -> Result<(), Self::Error> {
        self.request(&key(session_id), Op::Append(messages))
            .await
            .map(|_| ())
    }

    async fn spawn_thread(
        &self,
        parent_thread_id: &ThreadId<str>,
        spawn_tool_call_id: &ToolCallId<str>,
        is_for_subscription_event: bool,
        context: SpawnContext,
    ) -> Result<ThreadId, Self::Error> {
        let new_id = uuid::Uuid::new_v4().to_string();
        self.request(
            &new_id.clone(),
            Op::Spawn {
                new_id: new_id.clone(),
                parent: key(parent_thread_id),
                tool_call: spawn_tool_call_id.as_ref().to_string(),
                subscription_event: is_for_subscription_event,
                context,
            },
        )
        .await
        .map(|_| ThreadId::new(new_id))
    }

    async fn is_thread_closed(&self, thread_id: &ThreadId<str>) -> Result<bool, Self::Error> {
        match self.request(&key(thread_id), Op::IsClosed).await? {
            Reply::Bool(value) => Ok(value),
            _ => Err(ConversationError::ActorGone),
        }
    }

    async fn close_thread(&self, thread_id: &ThreadId<str>) -> Result<(), Self::Error> {
        self.request(&key(thread_id), Op::Close).await.map(|_| ())
    }

    async fn is_subscription_event_thread(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<bool, Self::Error> {
        match self
            .request(&key(thread_id), Op::IsSubscriptionEvent)
            .await?
        {
            Reply::Bool(value) => Ok(value),
            _ => Err(ConversationError::ActorGone),
        }
    }

    async fn is_fresh_context_thread(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<bool, Self::Error> {
        match self.request(&key(thread_id), Op::IsFreshContext).await? {
            Reply::Bool(value) => Ok(value),
            _ => Err(ConversationError::ActorGone),
        }
    }

    async fn get_thread_parent_info(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Option<(ThreadId, ToolCallId)>, Self::Error> {
        match self.request(&key(thread_id), Op::ParentInfo).await? {
            Reply::ParentInfo(info) => {
                Ok(info.map(|(parent, call)| (ThreadId::new(parent), ToolCallId::new(call))))
            }
            _ => Err(ConversationError::ActorGone),
        }
    }

    async fn get_ancestor_chain(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Vec<(ThreadId, i64)>, Self::Error> {
        match self.request(&key(thread_id), Op::AncestorChain).await? {
            Reply::Ancestors(chain) => Ok(chain
                .into_iter()
                .map(|(id, order)| (ThreadId::new(id), order))
                .collect()),
            _ => Err(ConversationError::ActorGone),
        }
    }

    async fn mark_thread_as_compaction(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<(), Self::Error> {
        self.request(&key(thread_id), Op::MarkCompaction)
            .await
            .map(|_| ())
    }

    async fn is_compaction_thread(&self, thread_id: &ThreadId<str>) -> Result<bool, Self::Error> {
        match self.request(&key(thread_id), Op::IsCompaction).await? {
            Reply::Bool(value) => Ok(value),
            _ => Err(ConversationError::ActorGone),
        }
    }

    async fn get_thread_spawn_order(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Option<i64>, Self::Error> {
        match self.request(&key(thread_id), Op::SpawnOrder).await? {
            Reply::Order(order) => Ok(order),
            _ => Err(ConversationError::ActorGone),
        }
    }

    async fn save_compaction_summary(
        &self,
        thread_id: &ThreadId<str>,
        summary: &str,
        up_to_order: i64,
    ) -> Result<(), Self::Error> {
        self.request(
            &key(thread_id),
            Op::SaveSummary {
                summary: summary.to_string(),
                up_to_order,
            },
        )
        .await
        .map(|_| ())
    }

    async fn load_latest_compaction_summary_up_to(
        &self,
        thread_id: &ThreadId<str>,
        up_to_order: Option<i64>,
    ) -> Result<Option<(String, i64)>, Self::Error> {
        match self
            .request(&key(thread_id), Op::LoadSummary { up_to_order })
            .await?
        {
            Reply::Summary(found) => Ok(found),
            _ => Err(ConversationError::ActorGone),
        }
    }
}

//! The one shape every surface renders.
//!
//! Desktop, Android and the browser all read the same event log, so they must not
//! each invent a transcript fold. This module produces a single serialisable view;
//! a golden test pins its JSON, and any renderer that disagrees with it is wrong.
//!
//! Everything here is derived: no method writes, and the view can be rebuilt from
//! the log at any time (that is what makes a pull enough to catch up).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::summary::summarize;
use crate::thread::{Role, queue, transcript, truncations};
use crate::types::{DeviceId, SessionId, SyncEvent};

/// One visible message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewMessage {
    pub event: Uuid,
    /// `user` or `assistant`.
    pub role: String,
    pub text: String,
    /// How many versions of this message exist; >1 means an edit happened.
    pub versions: usize,
    pub reply_to: Option<Uuid>,
}

/// One message waiting for the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewQueued {
    pub event: Uuid,
    pub text: String,
}

/// One delete-from-here marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewTruncation {
    pub id: Uuid,
    pub from: Uuid,
    pub active: bool,
}

/// Everything a client needs to draw one session and decide whether it may write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionView {
    pub session: String,
    pub device: String,
    /// True when another device holds the lease: render, but disable the composer.
    pub read_only: bool,
    pub holder: Option<String>,
    pub fence: u64,
    pub last_seq: u64,
    pub messages: Vec<ViewMessage>,
    pub queue: Vec<ViewQueued>,
    pub truncations: Vec<ViewTruncation>,
}

/// Fold a session's events into the shared view.
pub fn session_view(
    session: &SessionId,
    events: &[SyncEvent],
    device: &DeviceId,
    now_ms: u64,
) -> SessionView {
    let summary = summarize(session, events, device, now_ms);

    let rows = transcript(events);
    let messages = rows
        .into_iter()
        .map(|row| ViewMessage {
            event: row.event_id,
            role: match row.role {
                Role::User => "user".to_owned(),
                Role::Assistant => "assistant".to_owned(),
            },
            text: row.text,
            versions: row.versions,
            reply_to: events
                .iter()
                .find(|event| event.event_id == row.event_id)
                .and_then(|event| {
                    serde_json::from_value::<crate::MessagePayload>(event.payload.clone())
                        .ok()
                        .and_then(|payload| payload.reply_to)
                }),
        })
        .collect();

    SessionView {
        session: session.as_str().to_owned(),
        device: device.as_str().to_owned(),
        read_only: summary.read_only,
        holder: summary.holder.map(|holder| holder.to_string()),
        fence: summary.fence,
        last_seq: summary.last_seq,
        messages,
        queue: queue(events)
            .into_iter()
            .map(|entry| ViewQueued {
                event: entry.event_id,
                text: entry.text,
            })
            .collect(),
        truncations: truncations(events)
            .into_iter()
            .map(|entry| ViewTruncation {
                id: entry.id,
                from: entry.from,
                active: entry.active,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventKind, EventStore, FileEventStore, LeaseGuard, SqliteEventStore, dequeue_event,
        enqueue_event, message_event, now_ms, restore_event, truncate_event,
    };

    fn session() -> SessionId {
        SessionId::new("chat")
    }

    fn device() -> DeviceId {
        DeviceId::new("laptop")
    }

    /// A conversation with every feature the view must represent.
    fn events() -> Vec<SyncEvent> {
        let mut events = Vec::new();
        let question = message_event(
            session(),
            device(),
            1,
            EventKind::UserMessage,
            "what changed?",
            None,
            None,
        );
        let question_id = question.event_id;
        let answer = message_event(
            session(),
            device(),
            2,
            EventKind::AssistantMessage,
            "the log",
            Some(question_id),
            None,
        );
        let edit = message_event(
            session(),
            device(),
            3,
            EventKind::UserMessage,
            "what changed, exactly?",
            None,
            Some(question_id),
        );
        let queued_kept = enqueue_event(session(), device(), 4, "also check the queue", None);
        let queued_dropped = enqueue_event(session(), device(), 5, "never mind", None);
        let dequeued = dequeue_event(session(), device(), 6, queued_dropped.event_id);
        let truncation = truncate_event(session(), device(), 7, answer.event_id);

        events.extend([
            question,
            answer,
            edit,
            queued_kept,
            queued_dropped,
            dequeued,
            truncation.clone(),
        ]);
        events
    }

    async fn append_all<S: EventStore>(store: &S, events: &[SyncEvent]) {
        for event in events {
            store.append(event).await.expect("append");
        }
    }

    async fn seeded<S: EventStore>(store: &S) -> Vec<SyncEvent> {
        let events = events();
        append_all(store, &events).await;
        events
    }

    #[tokio::test]
    async fn both_backends_produce_the_same_view() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = FileEventStore::new(dir.path());
        let sqlite = SqliteEventStore::in_memory().expect("open");
        let shared = events();
        append_all(&file, &shared).await;
        append_all(&sqlite, &shared).await;

        let from_file = file.events_since(&session(), 0).await.expect("file");
        let from_sqlite = sqlite.events_since(&session(), 0).await.expect("sqlite");
        let view_file = session_view(&session(), &from_file, &device(), now_ms());
        let view_sqlite = session_view(&session(), &from_sqlite, &device(), now_ms());

        assert_eq!(view_file, view_sqlite);
    }

    #[tokio::test]
    async fn the_view_reports_the_queue_and_the_deletion() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteEventStore::in_memory().expect("open");
        let events = seeded(&store).await;
        let view = session_view(&session(), &events, &device(), now_ms());

        // The truncation starts at the answer, which is the first visible message
        // after the edit replaced the question, so the visible conversation is
        // empty until the deletion is undone.
        assert!(view.messages.is_empty());
        assert_eq!(view.queue.len(), 1);
        assert_eq!(view.queue[0].text, "also check the queue");
        assert_eq!(view.truncations.len(), 1);
        assert!(view.truncations[0].active);
        assert!(!view.read_only);
    }

    #[tokio::test]
    async fn restoring_brings_the_tail_back_into_the_view() {
        let store = SqliteEventStore::in_memory().expect("open");
        let events = seeded(&store).await;
        let truncation = truncations(&events)[0].id;

        let restored = restore_event(session(), device(), 8, truncation);
        store.append(&restored).await.expect("append restore");
        let events = store.events_since(&session(), 0).await.expect("read");
        let view = session_view(&session(), &events, &device(), now_ms());

        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].text, "what changed, exactly?");
        assert_eq!(view.messages[0].versions, 2);
        assert!(!view.truncations[0].active);
    }

    #[tokio::test]
    async fn the_view_is_a_stable_contract() {
        // A renderer that disagrees with this JSON is wrong, not merely different.
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteEventStore::in_memory().expect("open");
        let question = message_event(
            session(),
            device(),
            1,
            EventKind::UserMessage,
            "hi",
            None,
            None,
        );
        store.append(&question).await.expect("append");

        let events = store.events_since(&session(), 0).await.expect("read");
        let view = session_view(&session(), &events, &device(), now_ms());
        let json = serde_json::to_value(&view).expect("serialize");

        assert_eq!(json["session"], "chat");
        assert_eq!(json["device"], "laptop");
        assert_eq!(json["read_only"], false);
        assert_eq!(json["last_seq"], 1);
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["text"], "hi");
        assert_eq!(json["messages"][0]["versions"], 1);
        assert_eq!(json["queue"], serde_json::json!([]));
        assert_eq!(json["truncations"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn another_device_is_read_only_in_the_view() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteEventStore::in_memory().expect("open");
        let _guard = LeaseGuard::acquire(&store, session(), device(), 60_000)
            .await
            .expect("acquire")
            .expect("free");

        let events = store.events_since(&session(), 0).await.expect("read");
        let view = session_view(&session(), &events, &DeviceId::new("phone"), now_ms());
        assert!(view.read_only);
        assert_eq!(view.holder.as_deref(), Some("laptop"));
        assert_eq!(view.fence, 1);

        let _ = restore_event(session(), device(), 9, Uuid::nil());
    }
}

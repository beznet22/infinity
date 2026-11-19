//! Threading for edits and branches.
//!
//! A message records the message it answers (`reply_to`). Editing a message
//! appends a new version carrying `edited_from` that points at the version it
//! replaces, and keeps the original's `reply_to`, so the fork happens at the
//! edit's parent rather than after the answer to the old text.
//!
//! What is visible follows from that: a superseded version is hidden, and so is
//! anything that replied to it. Both versions still exist in the log, so a UI can
//! offer a switcher over [`versions`] without rewriting history.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{DeviceId, EventKind, SessionId, SyncEvent, now_ms};

/// Payload carried by [`EventKind::UserMessage`] and
/// [`EventKind::AssistantMessage`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessagePayload {
    pub text: String,
    #[serde(default)]
    pub at: u64,
    /// The message this one answers, if any.
    #[serde(default)]
    pub reply_to: Option<Uuid>,
    /// The version this one replaces, when it is an edit.
    #[serde(default)]
    pub edited_from: Option<Uuid>,
    /// Filled from the event itself when read back, never serialized by hand.
    #[serde(skip)]
    pub event_id: Uuid,
}

/// Payload carried by [`EventKind::MessageQueued`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuePayload {
    pub text: String,
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub reply_to: Option<Uuid>,
}

/// Payload carried by [`EventKind::MessageSent`] and
/// [`EventKind::MessageDequeued`]: the queued message they act on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetPayload {
    pub target: Uuid,
}

/// Payload carried by [`EventKind::ConversationTruncated`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TruncatePayload {
    /// The first message to hide; everything after it is hidden too.
    pub from: Uuid,
}

/// Payload carried by [`EventKind::ConversationRestored`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestorePayload {
    /// The truncation event to undo.
    pub truncation: Uuid,
}

/// A message waiting for the agent, not yet part of the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub event_id: Uuid,
    pub text: String,
    pub at: u64,
    pub reply_to: Option<Uuid>,
}

/// A delete-from-here marker. `active` is false once it was restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Truncation {
    pub id: Uuid,
    pub from: Uuid,
    pub active: bool,
}

/// Who wrote a visible message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// One row of the transcript as a client would render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    pub event_id: Uuid,
    pub role: Role,
    pub text: String,
    /// How many versions of this message exist; 1 means never edited.
    pub versions: usize,
}

fn parse(event: &SyncEvent) -> Option<(Role, MessagePayload)> {
    let role = match event.kind {
        EventKind::UserMessage => Role::User,
        EventKind::AssistantMessage => Role::Assistant,
        _ => return None,
    };
    let mut payload = serde_json::from_value::<MessagePayload>(event.payload.clone()).ok()?;
    payload.event_id = event.event_id;
    Some((role, payload))
}

/// Ids that an edit replaced.
fn superseded(events: &[SyncEvent]) -> Vec<Uuid> {
    events
        .iter()
        .filter_map(parse)
        .filter_map(|(_, payload)| payload.edited_from)
        .collect()
}

/// Queued messages that the agent already received.
fn promoted(events: &[SyncEvent]) -> std::collections::HashSet<Uuid> {
    events
        .iter()
        .filter(|event| event.kind == EventKind::MessageSent)
        .filter_map(|event| serde_json::from_value::<TargetPayload>(event.payload.clone()).ok())
        .map(|payload| payload.target)
        .collect()
}

/// Queued messages the user removed.
fn dequeued(events: &[SyncEvent]) -> std::collections::HashSet<Uuid> {
    events
        .iter()
        .filter(|event| event.kind == EventKind::MessageDequeued)
        .filter_map(|event| serde_json::from_value::<TargetPayload>(event.payload.clone()).ok())
        .map(|payload| payload.target)
        .collect()
}

fn messages(events: &[SyncEvent]) -> Vec<(Role, MessagePayload)> {
    let promoted = promoted(events);
    let mut parsed: Vec<(usize, Role, MessagePayload)> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            if event.kind == EventKind::MessageQueued {
                // A queued message joins the transcript only once it was sent.
                if !promoted.contains(&event.event_id) {
                    return None;
                }
                let payload = serde_json::from_value::<QueuePayload>(event.payload.clone()).ok()?;
                return Some((
                    index,
                    Role::User,
                    MessagePayload {
                        text: payload.text,
                        at: payload.at,
                        reply_to: payload.reply_to,
                        edited_from: None,
                        event_id: event.event_id,
                    },
                ));
            }
            parse(event).map(|(role, payload)| (index, role, payload))
        })
        .collect();
    // Log order is the writer's order; `at` breaks ties across devices.
    parsed.sort_by_key(|(index, _, payload)| (payload.at, *index));
    parsed
        .into_iter()
        .map(|(_, role, payload)| (role, payload))
        .collect()
}

/// Messages waiting for the agent: queued, and neither sent nor removed.
///
/// This is the list a UI shows above the composer while a turn is running, in the
/// order the user wrote them.
pub fn queue(events: &[SyncEvent]) -> Vec<QueuedMessage> {
    let sent = promoted(events);
    let dropped = dequeued(events);
    let mut waiting: Vec<(u64, usize, QueuedMessage)> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind == EventKind::MessageQueued)
        .filter(|(_, event)| !sent.contains(&event.event_id) && !dropped.contains(&event.event_id))
        .filter_map(|(index, event)| {
            let payload = serde_json::from_value::<QueuePayload>(event.payload.clone()).ok()?;
            Some((
                payload.at,
                index,
                QueuedMessage {
                    event_id: event.event_id,
                    text: payload.text,
                    at: payload.at,
                    reply_to: payload.reply_to,
                },
            ))
        })
        .collect();
    waiting.sort_by_key(|(at, index, _)| (*at, *index));
    waiting.into_iter().map(|(_, _, message)| message).collect()
}

/// Every delete-from-here marker, including restored ones.
pub fn truncations(events: &[SyncEvent]) -> Vec<Truncation> {
    let restored: std::collections::HashSet<Uuid> = events
        .iter()
        .filter(|event| event.kind == EventKind::ConversationRestored)
        .filter_map(|event| serde_json::from_value::<RestorePayload>(event.payload.clone()).ok())
        .map(|payload| payload.truncation)
        .collect();

    events
        .iter()
        .filter(|event| event.kind == EventKind::ConversationTruncated)
        .filter_map(|event| {
            let payload = serde_json::from_value::<TruncatePayload>(event.payload.clone()).ok()?;
            Some(Truncation {
                id: event.event_id,
                from: payload.from,
                active: !restored.contains(&event.event_id),
            })
        })
        .collect()
}

/// The message with this id, if it is a user or assistant message.
pub fn message(events: &[SyncEvent], id: Uuid) -> Option<MessagePayload> {
    messages(events)
        .into_iter()
        .map(|(_, payload)| payload)
        .find(|payload| payload.event_id == id)
}

/// Every stored version of the message `target`, oldest first.
///
/// Edits form a group: each version points at the one it replaced, so the group
/// is the connected component. Returns an empty vector when `target` is unknown.
pub fn versions(events: &[SyncEvent], target: Uuid) -> Vec<MessagePayload> {
    let all = messages(events);
    let index: std::collections::HashMap<Uuid, &MessagePayload> = all
        .iter()
        .map(|(_, payload)| (payload.event_id, payload))
        .collect();
    let parent_of = |id: Uuid| index.get(&id).and_then(|payload| payload.edited_from);

    let Some(root) = index
        .get(&target)
        .map(|payload| chain_root_of(payload.event_id, parent_of))
    else {
        return Vec::new();
    };

    let members: std::collections::HashSet<Uuid> = all
        .iter()
        .filter(|(_, payload)| chain_root_of(payload.event_id, parent_of) == root)
        .map(|(_, payload)| payload.event_id)
        .collect();

    all.into_iter()
        .map(|(_, payload)| payload)
        .filter(|payload| members.contains(&payload.event_id))
        .collect()
}

fn chain_root_of<F: Fn(Uuid) -> Option<Uuid>>(id: Uuid, parent: F) -> Uuid {
    let mut root = id;
    let mut cursor = parent(id);
    let mut guard = 0;
    while let Some(next) = cursor {
        root = next;
        cursor = parent(next);
        guard += 1;
        if guard > 10_000 {
            break;
        }
    }
    root
}

/// The visible transcript: superseded versions and answers to them are hidden.
pub fn transcript(events: &[SyncEvent]) -> Vec<TranscriptEntry> {
    let gone = superseded(events);
    let all = messages(events);

    let mut hidden: Vec<Uuid> = gone;

    // Deleting from a message hides it and everything after it, so the tail is
    // gone as a block rather than message by message.
    for truncation in truncations(events)
        .into_iter()
        .filter(|truncation| truncation.active)
    {
        if let Some(position) = all
            .iter()
            .position(|(_, payload)| payload.event_id == truncation.from)
        {
            hidden.extend(all[position..].iter().map(|(_, payload)| payload.event_id));
        }
    }

    // A reply to a hidden message is hidden too: the answer belonged to text the
    // user replaced.
    let mut changed = true;
    while changed {
        changed = false;
        for (_, payload) in &all {
            if let Some(parent) = payload.reply_to
                && hidden.contains(&parent)
                && !hidden.contains(&payload.event_id)
            {
                hidden.push(payload.event_id);
                changed = true;
            }
        }
    }

    // Version count is the size of the edit group a node belongs to.
    let index: std::collections::HashMap<Uuid, &MessagePayload> = all
        .iter()
        .map(|(_, payload)| (payload.event_id, payload))
        .collect();
    let parent_of = |id: Uuid| index.get(&id).and_then(|payload| payload.edited_from);
    let mut group_size: std::collections::HashMap<Uuid, usize> = std::collections::HashMap::new();
    for (_, payload) in &all {
        let root = chain_root_of(payload.event_id, parent_of);
        *group_size.entry(root).or_insert(0) += 1;
    }

    all.iter()
        .filter(|(_, payload)| !hidden.contains(&payload.event_id))
        .map(|(role, payload)| {
            let root = chain_root_of(payload.event_id, parent_of);
            TranscriptEntry {
                versions: group_size.get(&root).copied().unwrap_or(1),
                event_id: payload.event_id,
                role: *role,
                text: payload.text.clone(),
            }
        })
        .collect()
}

/// Messages that answer `parent` (`None` = root messages), i.e. the alternatives
/// a branch switcher would list.
pub fn alternatives(events: &[SyncEvent], parent: Option<Uuid>) -> Vec<Uuid> {
    messages(events)
        .into_iter()
        .filter(|(_, payload)| payload.reply_to == parent)
        .map(|(_, payload)| payload.event_id)
        .collect()
}

/// Queue a user message while the agent is busy.
pub fn enqueue_event(
    session: SessionId,
    device: DeviceId,
    seq: u64,
    text: impl Into<String>,
    reply_to: Option<Uuid>,
) -> SyncEvent {
    SyncEvent::new(
        session,
        device,
        seq,
        EventKind::MessageQueued,
        serde_json::to_value(QueuePayload {
            text: text.into(),
            at: now_ms(),
            reply_to,
        })
        .expect("bug: queue payload serializes"),
    )
}

/// The agent received the queued message; it joins the transcript.
pub fn send_event(session: SessionId, device: DeviceId, seq: u64, target: Uuid) -> SyncEvent {
    SyncEvent::new(
        session,
        device,
        seq,
        EventKind::MessageSent,
        serde_json::to_value(TargetPayload { target }).expect("bug: target payload serializes"),
    )
}

/// The user removed the queued message before it was sent.
pub fn dequeue_event(session: SessionId, device: DeviceId, seq: u64, target: Uuid) -> SyncEvent {
    SyncEvent::new(
        session,
        device,
        seq,
        EventKind::MessageDequeued,
        serde_json::to_value(TargetPayload { target }).expect("bug: target payload serializes"),
    )
}

/// Delete the conversation from `from` onward. Reversible via [`restore_event`].
pub fn truncate_event(session: SessionId, device: DeviceId, seq: u64, from: Uuid) -> SyncEvent {
    SyncEvent::new(
        session,
        device,
        seq,
        EventKind::ConversationTruncated,
        serde_json::to_value(TruncatePayload { from }).expect("bug: truncate payload serializes"),
    )
}

/// Undo a truncation; everything it hid becomes visible again.
pub fn restore_event(
    session: SessionId,
    device: DeviceId,
    seq: u64,
    truncation: Uuid,
) -> SyncEvent {
    SyncEvent::new(
        session,
        device,
        seq,
        EventKind::ConversationRestored,
        serde_json::to_value(RestorePayload { truncation })
            .expect("bug: restore payload serializes"),
    )
}

/// Build a message event. `edited_from` makes it an edit of an earlier version.
pub fn message_event(
    session: SessionId,
    device: DeviceId,
    seq: u64,
    kind: EventKind,
    text: impl Into<String>,
    reply_to: Option<Uuid>,
    edited_from: Option<Uuid>,
) -> SyncEvent {
    assert!(
        matches!(kind, EventKind::UserMessage | EventKind::AssistantMessage),
        "bug: message_event only builds message kinds"
    );
    SyncEvent::new(
        session,
        device,
        seq,
        kind,
        serde_json::to_value(MessagePayload {
            text: text.into(),
            at: now_ms(),
            reply_to,
            edited_from,
            event_id: Uuid::nil(),
        })
        .expect("bug: message payload serializes"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceId, EventKind, SessionId, SyncEvent};

    fn device() -> DeviceId {
        DeviceId::new("laptop")
    }

    fn session() -> SessionId {
        SessionId::new("chat")
    }

    fn user(seq: u64, text: &str, reply_to: Option<Uuid>, edited_from: Option<Uuid>) -> SyncEvent {
        message_event(
            session(),
            device(),
            seq,
            EventKind::UserMessage,
            text,
            reply_to,
            edited_from,
        )
    }

    fn assistant(seq: u64, text: &str, reply_to: Option<Uuid>) -> SyncEvent {
        message_event(
            session(),
            device(),
            seq,
            EventKind::AssistantMessage,
            text,
            reply_to,
            None,
        )
    }

    #[test]
    fn a_plain_thread_renders_in_order() {
        let first = user(1, "hi", None, None);
        let id = first.event_id;
        let reply = assistant(2, "hello", Some(id));
        let events = vec![first, reply];

        let rows = transcript(&events);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].text, "hi");
        assert_eq!(rows[0].role, Role::User);
        assert_eq!(rows[1].text, "hello");
        assert_eq!(rows[1].role, Role::Assistant);
        assert_eq!(rows[0].versions, 1);
    }

    #[test]
    fn an_edit_hides_the_old_text_and_its_answer() {
        let original = user(1, "first draft", None, None);
        let original_id = original.event_id;
        let stale_answer = assistant(2, "answer to the draft", Some(original_id));
        let edit = user(3, "edited", None, Some(original_id));
        let edit_id = edit.event_id;
        let fresh_answer = assistant(4, "answer to the edit", Some(edit_id));

        let rows = transcript(&[original, stale_answer, edit, fresh_answer]);
        let texts: Vec<&str> = rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(texts, vec!["edited", "answer to the edit"]);
        assert_eq!(rows[0].versions, 2);
        assert_eq!(rows[0].event_id, edit_id);
    }

    #[test]
    fn an_edit_fork_stays_under_the_same_parent() {
        // Editing the first message of a thread must not detach it from the
        // conversation: the new version keeps the original's parent (here: none).
        let original = user(1, "one", None, None);
        let original_id = original.event_id;
        let edit = user(2, "one, revised", None, Some(original_id));

        assert_eq!(
            alternatives(&[original.clone(), edit.clone()], None).len(),
            2
        );
        let versions = versions(&[original, edit], original_id);
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].text, "one");
        assert_eq!(versions[1].text, "one, revised");
    }

    #[test]
    fn editing_an_answer_hides_only_that_answer() {
        let question = user(1, "why?", None, None);
        let question_id = question.event_id;
        let answer = assistant(2, "because", Some(question_id));
        let answer_id = answer.event_id;
        let improved = message_event(
            session(),
            device(),
            3,
            EventKind::AssistantMessage,
            "because of this",
            Some(question_id),
            Some(answer_id),
        );

        let rows = transcript(&[question, answer, improved]);
        let texts: Vec<&str> = rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(texts, vec!["why?", "because of this"]);
    }

    #[test]
    fn a_queued_message_waits_outside_the_transcript_until_sent() {
        let queued = enqueue_event(session(), device(), 1, "while you work", None);
        let queued_id = queued.event_id;

        // Nothing the agent has not received may appear as conversation.
        assert!(transcript(std::slice::from_ref(&queued)).is_empty());
        assert_eq!(queue(std::slice::from_ref(&queued)).len(), 1);

        let sent = send_event(session(), device(), 2, queued_id);
        let rows = transcript(&[queued, sent.clone()]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "while you work");
        assert!(queue(&[sent]).is_empty());
    }

    #[test]
    fn dequeuing_removes_a_queued_message_for_good() {
        let first = enqueue_event(session(), device(), 1, "drop me", None);
        let second = enqueue_event(session(), device(), 2, "keep me", None);
        let first_id = first.event_id;

        let removed = dequeue_event(session(), device(), 3, first_id);
        let events = vec![first, second.clone(), removed];

        let waiting = queue(&events);
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].event_id, second.event_id);

        // Trying to send a dequeued message cannot resurrect it: it has no send.
        let rows = transcript(&events);
        assert!(rows.is_empty());
    }

    #[test]
    fn queue_order_follows_when_the_user_wrote_them() {
        let first = enqueue_event(session(), device(), 1, "first", None);
        let second = enqueue_event(session(), device(), 2, "second", None);
        // The agent takes the second one first (a tool call finished early).
        let sent_second = send_event(session(), device(), 3, second.event_id);
        let sent_first = send_event(session(), device(), 4, first.event_id);

        let rows = transcript(&[first, second, sent_second, sent_first]);
        let texts: Vec<&str> = rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second"]);
    }

    #[test]
    fn deleting_from_a_message_hides_the_tail_and_restore_brings_it_back() {
        let question = user(1, "q1", None, None);
        let answer = assistant(2, "a1", Some(question.event_id));
        let follow_up = user(3, "q2", None, None);
        let follow_up_id = follow_up.event_id;
        let last = assistant(4, "a2", Some(follow_up_id));

        let truncation = truncate_event(session(), device(), 5, follow_up_id);
        let truncation_id = truncation.event_id;
        let events = vec![question, answer, follow_up, last, truncation];

        let rows = transcript(&events);
        let texts: Vec<&str> = rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(texts, vec!["q1", "a1"]);
        assert_eq!(truncations(&events)[0].active, true);

        let restored = restore_event(session(), device(), 6, truncation_id);
        let events: Vec<SyncEvent> = events.into_iter().chain([restored]).collect();
        let rows = transcript(&events);
        assert_eq!(rows.len(), 4);
        assert!(!truncations(&events)[0].active);
    }

    #[test]
    fn a_truncation_survives_a_reload() {
        // Delete and restore are log entries, so a second device that pulls the
        // log sees the same conversation without any extra protocol.
        let dir = tempfile::tempdir().expect("tempdir");
        let store = crate::FileEventStore::new(dir.path());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            use crate::EventStore;
            let first = user(1, "keep", None, None);
            let second = user(2, "hide", None, None);
            let second_id = second.event_id;
            let truncation = truncate_event(session(), device(), 3, second_id);
            for event in [first, second, truncation] {
                store.append(&event).await.expect("append");
            }

            let events = store.events_since(&session(), 0).await.expect("events");
            let texts: Vec<String> = transcript(&events)
                .into_iter()
                .map(|row| row.text)
                .collect();
            assert_eq!(texts, vec!["keep".to_owned()]);
        });
    }

    #[test]
    fn legacy_messages_without_threading_still_render() {
        let a = user(1, "legacy one", None, None);
        let b = user(2, "legacy two", None, None);
        let rows = transcript(&[a, b]);
        assert_eq!(rows.len(), 2);
    }
}

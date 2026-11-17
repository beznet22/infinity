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

use crate::types::{EventKind, SessionId, SyncEvent, now_ms};

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

fn messages(events: &[SyncEvent]) -> Vec<(Role, MessagePayload)> {
    let mut parsed: Vec<(usize, Role, MessagePayload)> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| parse(event).map(|(role, payload)| (index, role, payload)))
        .collect();
    // Log order is the writer's order; `at` breaks ties across devices.
    parsed.sort_by_key(|(index, _, payload)| (payload.at, *index));
    parsed
        .into_iter()
        .map(|(_, role, payload)| (role, payload))
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

    // A reply to a hidden message is hidden too: the answer belonged to text the
    // user replaced.
    let mut hidden: Vec<Uuid> = gone;
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

/// Build a message event. `edited_from` makes it an edit of an earlier version.
pub fn message_event(
    session: SessionId,
    device: crate::DeviceId,
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
    fn legacy_messages_without_threading_still_render() {
        let a = user(1, "legacy one", None, None);
        let b = user(2, "legacy two", None, None);
        let rows = transcript(&[a, b]);
        assert_eq!(rows.len(), 2);
    }
}

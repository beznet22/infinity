//! Wire types shared by every device and by the relay.
//!
//! The unit of replication is an immutable **event**, never a row or a file.
//! Recording reality as a log is what makes retries idempotent, resuming a
//! session on another device a replay, and conflict handling a policy choice
//! instead of a merge algorithm.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier for an installation (one phone, one laptop, one tab).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identifier for an agent session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What an event represents.
///
/// Chunks are events too: a streamed answer is replayable without inventing a
/// second persistence path for partial output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    UserMessage,
    /// A completed assistant reply, as opposed to a streaming `AssistantChunk`.
    AssistantMessage,
    AssistantChunk,
    ToolCall,
    ToolResult,
    StateChange,
    /// Full state after compaction; lets a new device skip the whole history.
    Snapshot,
    /// Ownership claim for running the session on one device at a time.
    LeaseClaim,
    /// A user message written while the agent is busy; not yet in the transcript.
    MessageQueued,
    /// The queued message was delivered to the agent.
    MessageSent,
    /// The user removed a queued message before it was sent.
    MessageDequeued,
    /// Delete from a message onward; `restore` brings the tail back.
    ConversationTruncated,
    /// Undo a truncation by its event id.
    ConversationRestored,
}

/// One immutable fact about a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncEvent {
    pub session_id: SessionId,
    /// Assigned by the writer. Used by the relay to deduplicate retries.
    pub event_id: Uuid,
    /// Monotonic per (session, device). Ordering across devices comes from the
    /// relay's receipt order, not from this value.
    pub seq: u64,
    pub device_id: DeviceId,
    pub kind: EventKind,
    pub payload: serde_json::Value,
    pub created_at_ms: u64,
    /// Fencing token of the writer's lease, when it had one. A relay rejects
    /// events carrying a fence older than the session's current holder, which
    /// closes the brief overlap that expiry alone permits (D45).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fence: Option<u64>,
}

impl SyncEvent {
    /// Build an event, taking the id from UUIDv7 (time-ordered, so a receiving
    /// database gets a useful index for free).
    pub fn new(
        session_id: SessionId,
        device_id: DeviceId,
        seq: u64,
        kind: EventKind,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            session_id,
            event_id: Uuid::now_v7(),
            seq,
            device_id,
            kind,
            payload,
            created_at_ms: now_ms(),
            fence: None,
        }
    }

    /// Stamp this event with the writer's fencing token.
    pub fn with_fence(mut self, fence: u64) -> Self {
        self.fence = Some(fence);
        self
    }
}

/// An event as the relay stored it.
///
/// `position` is assigned by the relay and is the **only** valid cross-device
/// order: the writer's `seq` is per device, so two devices both write 1, 2, 3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEvent {
    pub position: i64,
    #[serde(flatten)]
    pub event: SyncEvent,
}

/// How far this device has already read a session (relay position).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// Highest relay position this device has read for the session.
    pub last_position: i64,
}

/// Wall-clock milliseconds. Sync ordering never depends on it; leases do.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_ids_sort_by_creation_time() {
        let first = Uuid::now_v7();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = Uuid::now_v7();
        assert!(first < second, "UUIDv7 must be time-ordered");
    }

    #[test]
    fn events_round_trip_through_json() {
        let event = SyncEvent::new(
            SessionId::new("s-1"),
            DeviceId::new("phone"),
            7,
            EventKind::UserMessage,
            serde_json::json!({ "text": "hello" }),
        );
        let encoded = serde_json::to_string(&event).expect("serialize");
        let decoded: SyncEvent = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(event, decoded);
        assert!(encoded.contains("\"user_message\""));
    }
}

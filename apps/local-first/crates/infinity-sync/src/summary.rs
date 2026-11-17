//! Per-session summaries for a thread list and for resuming on another device.
//!
//! A reader needs three things before it may type: what the conversation is, how
//! far the log goes, and whether another device is currently the writer. The
//! summary answers all three without replaying the whole session into memory.

use crate::lease::current_lease;
use crate::thread::transcript;
use crate::types::{DeviceId, SessionId, SyncEvent};

/// What a thread list shows for one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub session: SessionId,
    /// Visible transcript rows, i.e. what the reader would see.
    pub messages: usize,
    /// Highest writer-assigned sequence number in the log.
    pub last_seq: u64,
    /// The device currently allowed to write, if any.
    pub holder: Option<DeviceId>,
    /// The holder's fencing token (0 when nobody holds it).
    pub fence: u64,
    /// True when someone else holds the lease: this device can read, not write.
    pub read_only: bool,
}

/// Summarize one session's events for `device`.
pub fn summarize(
    session: &SessionId,
    events: &[SyncEvent],
    device: &DeviceId,
    now_ms: u64,
) -> SessionSummary {
    let lease = current_lease(events, now_ms);
    let holder = lease.as_ref().map(|lease| lease.owner.clone());
    SessionSummary {
        session: session.clone(),
        messages: transcript(events).len(),
        last_seq: events.iter().map(|event| event.seq).max().unwrap_or(0),
        read_only: holder.as_ref().is_some_and(|owner| owner != device),
        fence: lease.map(|lease| lease.fence).unwrap_or(0),
        holder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventKind, EventStore, FileEventStore, LeaseGuard, message_event, now_ms};

    fn session() -> SessionId {
        SessionId::new("chat")
    }

    fn device(name: &str) -> DeviceId {
        DeviceId::new(name)
    }

    #[tokio::test]
    async fn a_free_session_is_writable_and_counts_messages() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileEventStore::new(dir.path());
        let first = message_event(
            session(),
            device("laptop"),
            1,
            EventKind::UserMessage,
            "hello",
            None,
            None,
        );
        store.append(&first).await.expect("append");

        let events = store.events_since(&session(), 0).await.expect("events");
        let summary = summarize(&session(), &events, &device("phone"), now_ms());
        assert_eq!(summary.messages, 1);
        assert_eq!(summary.last_seq, 1);
        assert_eq!(summary.holder, None);
        assert!(!summary.read_only);
        assert_eq!(summary.fence, 0);
    }

    #[tokio::test]
    async fn another_devices_lease_makes_the_session_read_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileEventStore::new(dir.path());
        let _guard = LeaseGuard::acquire(&store, session(), device("laptop"), 60_000)
            .await
            .expect("acquire")
            .expect("free");

        let events = store.events_since(&session(), 0).await.expect("events");
        let reader = summarize(&session(), &events, &device("phone"), now_ms());
        assert!(reader.read_only);
        assert_eq!(reader.holder.as_ref().map(DeviceId::as_str), Some("laptop"));
        assert_eq!(reader.fence, 1);

        // The holder itself is not read-only.
        let owner = summarize(&session(), &events, &device("laptop"), now_ms());
        assert!(!owner.read_only);
    }
}

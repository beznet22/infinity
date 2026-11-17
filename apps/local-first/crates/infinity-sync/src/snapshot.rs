//! Snapshots: skip the history a new device does not need to replay.

use serde::{Deserialize, Serialize};

use crate::types::{DeviceId, EventKind, SessionId, SyncEvent};

/// Payload carried by [`EventKind::Snapshot`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotPayload {
    /// Whatever the app needs to restore (agent state, counters, cursors).
    pub state: serde_json::Value,
    /// Number of events the snapshot already covers.
    pub covered_events: usize,
}

/// Build a snapshot event for the first `covered_events` events of a session.
pub fn snapshot_event(
    session: SessionId,
    device: DeviceId,
    seq: u64,
    state: serde_json::Value,
    covered_events: usize,
) -> SyncEvent {
    SyncEvent::new(
        session,
        device,
        seq,
        EventKind::Snapshot,
        serde_json::json!(SnapshotPayload {
            state,
            covered_events,
        }),
    )
}

/// The newest snapshot in `events`, with the events that follow it.
///
/// Returns `(state, tail)`. Without a snapshot the state is `None` and the tail
/// is the whole log, so callers have one code path.
pub fn replay(events: &[SyncEvent]) -> (Option<serde_json::Value>, Vec<SyncEvent>) {
    let newest = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind == EventKind::Snapshot)
        .filter_map(|(index, event)| {
            serde_json::from_value::<SnapshotPayload>(event.payload.clone())
                .ok()
                .map(|payload| (index, payload))
        })
        .next_back();

    match newest {
        Some((index, payload)) => (Some(payload.state), events[index + 1..].to_vec()),
        None => (None, events.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> SessionId {
        SessionId::new("s")
    }

    fn device() -> DeviceId {
        DeviceId::new("laptop")
    }

    fn message(seq: u64, text: &str) -> SyncEvent {
        SyncEvent::new(
            session(),
            device(),
            seq,
            EventKind::UserMessage,
            serde_json::json!({ "text": text }),
        )
    }

    #[test]
    fn replay_without_a_snapshot_returns_everything() {
        let events = vec![message(1, "a"), message(2, "b")];
        let (state, tail) = replay(&events);
        assert!(state.is_none());
        assert_eq!(tail.len(), 2);
    }

    #[test]
    fn replay_returns_the_latest_snapshot_and_its_tail() {
        let events = vec![
            message(1, "a"),
            message(2, "b"),
            snapshot_event(session(), device(), 3, serde_json::json!({ "n": 2 }), 2),
            message(4, "c"),
            snapshot_event(session(), device(), 5, serde_json::json!({ "n": 4 }), 4),
            message(6, "d"),
        ];
        let (state, tail) = replay(&events);
        assert_eq!(state, Some(serde_json::json!({ "n": 4 })));
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].seq, 6);
    }

    #[test]
    fn snapshot_payload_round_trips() {
        let event = snapshot_event(session(), device(), 1, serde_json::json!({ "count": 3 }), 0);
        assert_eq!(event.kind, EventKind::Snapshot);
        let payload: SnapshotPayload = serde_json::from_value(event.payload).expect("payload");
        assert_eq!(payload.covered_events, 0);
    }
}

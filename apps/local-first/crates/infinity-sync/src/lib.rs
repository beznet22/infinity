//! Synchronising agent sessions across one user's devices.
//!
//! The model is deliberately not a CRDT. Sessions are single-writer: a phone
//! runs one, then a laptop resumes it. What that needs is an **append-only
//! event log**, a **per-device cursor**, and a **lease** so two devices never
//! execute the same tool call at once — not merge machinery for concurrent
//! edits.
//!
//! ```text
//!  device A ──push(events > cursor)──▶ relay ──▶ durable log
//!  device B ──pull(events > cursor)──▶ relay
//! ```
//!
//! The relay is swappable: the same contract is implemented by a local dev
//! server now and by a Cloudflare Worker (D1) later. Events are immutable and
//! carry a writer-assigned id, so a retried push is deduplicated rather than
//! duplicated.

mod lease;
mod snapshot;
mod store;
mod summary;
mod thread;
mod types;
mod view;

#[cfg(feature = "http")]
pub mod http;

pub use lease::{
    DEFAULT_TTL_MS, LeaseAction, LeaseGuard, LeasePayload, LeaseState, current_lease, lease_event,
    may_write, next_fence,
};
pub use snapshot::{SnapshotPayload, replay, snapshot_event};
#[cfg(feature = "sqlite")]
pub use store::SqliteEventStore;
pub use store::{CursorStore, EventStore, FileEventStore};
pub use summary::{SessionSummary, summarize};
pub use thread::{
    MessagePayload, QueuedMessage, Role, TranscriptEntry, Truncation, alternatives, dequeue_event,
    enqueue_event, message, message_event, queue, restore_event, send_event, transcript,
    truncate_event, truncations, versions,
};
pub use types::{Cursor, DeviceId, EventKind, SessionId, SyncEvent, now_ms};
pub use view::{SessionView, ViewMessage, ViewQueued, ViewTruncation, session_view};

/// Errors raised by the sync layer.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("io error on {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not encode sync state: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("corrupt event line {line:?}: {error}")]
    Corrupt {
        line: String,
        #[source]
        error: serde_json::Error,
    },
    #[error("lease lost: another device claimed the session")]
    LeaseLost,
    #[error("sqlite: {0}")]
    Sqlite(String),
    #[cfg(feature = "http")]
    #[error("transport error: {0}")]
    Transport(#[source] reqwest::Error),
    #[cfg(feature = "http")]
    #[error("relay rejected the request ({status}): {body}")]
    Relay { status: u16, body: String },
}

impl SyncError {
    pub(crate) fn io(path: impl Into<std::path::PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_kinds_are_stable_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&EventKind::ToolResult).expect("serialize"),
            "\"tool_result\""
        );
        let device = DeviceId::new("phone");
        assert_eq!(
            serde_json::to_string(&device).expect("serialize"),
            "\"phone\""
        );
    }
}

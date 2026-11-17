//! Append-only session logs on local disk.
//!
//! This is the desktop/CLI and Android store: one JSONL file per session, so a
//! crashed append can only truncate the last line, never corrupt earlier ones.
//! The browser uses SQLite instead; both are expected to satisfy the same
//! contract, which the tests below pin down.

use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;

use crate::SyncError;
use crate::types::{SessionId, SyncEvent};

/// A local, ordered event log.
///
/// `async fn` in a public trait cannot express an auto-trait bound, which is
/// deliberate here: the browser backend will have to return `!Send` futures
/// (D27), so pinning `Send` now would block it.
#[allow(async_fn_in_trait, reason = "Send would block the browser backend (D27)")]
pub trait EventStore {
    /// Appends an event. Re-appending the same `event_id` is a no-op, which is
    /// what makes a retried push safe.
    async fn append(&self, event: &SyncEvent) -> Result<(), SyncError>;

    /// Events whose `seq` is greater than `after_seq`, in ascending order.
    async fn events_since(
        &self,
        session: &SessionId,
        after_seq: u64,
    ) -> Result<Vec<SyncEvent>, SyncError>;

    /// Highest sequence stored for the session, or 0 when it has none.
    async fn last_seq(&self, session: &SessionId) -> Result<u64, SyncError>;

    /// Sessions with at least one event.
    async fn sessions(&self) -> Result<Vec<SessionId>, SyncError>;
}

/// File-backed [`EventStore`].
#[derive(Debug, Clone)]
pub struct FileEventStore {
    root: PathBuf,
}

impl FileEventStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    fn session_path(&self, session: &SessionId) -> PathBuf {
        self.sessions_dir()
            .join(format!("{}.jsonl", session.as_str()))
    }

    async fn read_events(&self, session: &SessionId) -> Result<Vec<SyncEvent>, SyncError> {
        let path = self.session_path(session);
        let raw = match tokio::fs::read_to_string(&path).await {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(SyncError::io(&path, error)),
        };

        raw.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<SyncEvent>(line).map_err(|error| SyncError::Corrupt {
                    line: line.to_string(),
                    error,
                })
            })
            .collect()
    }
}

impl EventStore for FileEventStore {
    async fn append(&self, event: &SyncEvent) -> Result<(), SyncError> {
        let existing = self.read_events(&event.session_id).await?;
        if existing
            .iter()
            .any(|known| known.event_id == event.event_id)
        {
            return Ok(());
        }

        let dir = self.sessions_dir();
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|error| SyncError::io(&dir, error))?;

        let path = self.session_path(&event.session_id);
        let mut line = serde_json::to_vec(event).map_err(SyncError::Encode)?;
        line.push(b'\n');

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .map_err(|error| SyncError::io(&path, error))?;
        file.write_all(&line)
            .await
            .map_err(|error| SyncError::io(&path, error))?;
        file.flush()
            .await
            .map_err(|error| SyncError::io(&path, error))
    }

    async fn events_since(
        &self,
        session: &SessionId,
        after_seq: u64,
    ) -> Result<Vec<SyncEvent>, SyncError> {
        let mut events = self.read_events(session).await?;
        events.retain(|event| event.seq > after_seq);
        events.sort_by_key(|event| event.seq);
        Ok(events)
    }

    async fn last_seq(&self, session: &SessionId) -> Result<u64, SyncError> {
        Ok(self
            .read_events(session)
            .await?
            .iter()
            .map(|event| event.seq)
            .max()
            .unwrap_or(0))
    }

    async fn sessions(&self) -> Result<Vec<SessionId>, SyncError> {
        let dir = self.sessions_dir();
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(SyncError::io(&dir, error)),
        };

        let mut sessions = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| SyncError::io(&dir, error))?
        {
            let path: PathBuf = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                sessions.push(SessionId::new(stem));
            }
        }
        sessions.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        Ok(sessions)
    }
}

/// Per-device cursors, persisted next to the logs.
#[derive(Debug, Clone)]
pub struct CursorStore {
    path: PathBuf,
}

impl CursorStore {
    pub fn new(root: impl AsRef<Path>, device: &crate::DeviceId) -> Self {
        Self {
            path: root
                .as_ref()
                .join(format!("cursors-{}.json", device.as_str())),
        }
    }

    pub async fn load(&self) -> Result<std::collections::HashMap<String, i64>, SyncError> {
        match tokio::fs::read_to_string(&self.path).await {
            Ok(raw) => serde_json::from_str(&raw).map_err(SyncError::Encode),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
            Err(error) => Err(SyncError::io(&self.path, error)),
        }
    }

    pub async fn set(&self, session: &SessionId, position: i64) -> Result<(), SyncError> {
        let mut cursors = self.load().await?;
        cursors.insert(session.as_str().to_string(), position);
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| SyncError::io(parent, error))?;
        }
        let encoded = serde_json::to_vec_pretty(&cursors).map_err(SyncError::Encode)?;
        tokio::fs::write(&self.path, encoded)
            .await
            .map_err(|error| SyncError::io(&self.path, error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DeviceId, EventKind};

    fn event(session: &str, seq: u64) -> SyncEvent {
        SyncEvent::new(
            SessionId::new(session),
            DeviceId::new("laptop"),
            seq,
            EventKind::UserMessage,
            serde_json::json!({ "seq": seq }),
        )
    }

    #[tokio::test]
    async fn appends_reads_and_resumes_from_a_cursor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileEventStore::new(dir.path());

        for seq in 1..=3 {
            store.append(&event("s", seq)).await.expect("append");
        }

        assert_eq!(store.last_seq(&SessionId::new("s")).await.unwrap(), 3);
        let all = store.events_since(&SessionId::new("s"), 0).await.unwrap();
        assert_eq!(all.len(), 3);
        let tail = store.events_since(&SessionId::new("s"), 2).await.unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].seq, 3);
    }

    #[tokio::test]
    async fn reappending_the_same_event_is_a_no_op() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileEventStore::new(dir.path());
        let event = event("s", 1);

        store.append(&event).await.expect("first");
        store.append(&event).await.expect("retry");
        store.append(&event).await.expect("retry again");

        assert_eq!(
            store
                .events_since(&SessionId::new("s"), 0)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn lists_sessions_and_tracks_cursors_per_device() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileEventStore::new(dir.path());
        store.append(&event("a", 1)).await.unwrap();
        store.append(&event("b", 1)).await.unwrap();

        let sessions = store.sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);

        let cursors = CursorStore::new(dir.path(), &DeviceId::new("laptop"));
        cursors.set(&SessionId::new("a"), 1).await.unwrap();
        let loaded = cursors.load().await.unwrap();
        assert_eq!(loaded.get("a"), Some(&1));
    }
}

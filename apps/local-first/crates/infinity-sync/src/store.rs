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
#[expect(
    async_fn_in_trait,
    reason = "Send would block the browser backend (D27)"
)]
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
                    line: line.to_owned(),
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

/// SQLite-backed [`EventStore`].
///
/// One row per event, one transaction per append: a killed process leaves either
/// the whole event or none of it, which is the property the JSONL store cannot
/// promise. Desktop and Android use this; the browser uses wa-sqlite over OPFS
/// with the same event shape.
#[cfg(feature = "sqlite")]
#[derive(Debug)]
pub struct SqliteEventStore {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

#[cfg(feature = "sqlite")]
impl SqliteEventStore {
    /// Open (or create) a database file and apply the schema.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SyncError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| SyncError::io(parent, error))?;
        }
        let conn = rusqlite::Connection::open(path).map_err(sqlite_error)?;
        // WAL gives readers a consistent view while a writer appends; FULL sync
        // is the durability half: the commit reaches the disk before we return.
        let _: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .map_err(sqlite_error)?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(sqlite_error)?;
        Self::init(conn)
    }

    /// In-memory database, for tests and throwaway probes.
    pub fn in_memory() -> Result<Self, SyncError> {
        let conn = rusqlite::Connection::open_in_memory().map_err(sqlite_error)?;
        Self::init(conn)
    }

    fn init(conn: rusqlite::Connection) -> Result<Self, SyncError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                 session_id    TEXT    NOT NULL,
                 event_id      TEXT    NOT NULL,
                 seq           INTEGER NOT NULL,
                 device_id     TEXT    NOT NULL,
                 kind          TEXT    NOT NULL,
                 payload       TEXT    NOT NULL,
                 created_at_ms INTEGER NOT NULL,
                 fence         INTEGER,
                 PRIMARY KEY (session_id, event_id)
             );
             CREATE INDEX IF NOT EXISTS idx_events_session_seq
                 ON events (session_id, seq);",
        )
        .map_err(sqlite_error)?;
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, SyncError> {
        self.conn
            .lock()
            .map_err(|_| SyncError::Sqlite("connection mutex poisoned".to_owned()))
    }

    /// Append inside one explicit transaction, so a crash cannot half-write.
    fn append_tx(&self, event: &SyncEvent) -> Result<usize, SyncError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(sqlite_error)?;
        let changed = tx
            .execute(
                "INSERT OR IGNORE INTO events
                     (session_id, event_id, seq, device_id, kind, payload, created_at_ms, fence)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    event.session_id.as_str(),
                    event.event_id.to_string(),
                    event.seq as i64,
                    event.device_id.as_str(),
                    serde_json::to_string(&event.kind).map_err(SyncError::Encode)?,
                    serde_json::to_string(&event.payload).map_err(SyncError::Encode)?,
                    event.created_at_ms as i64,
                    event.fence.map(|fence| fence as i64),
                ],
            )
            .map_err(sqlite_error)?;
        tx.commit().map_err(sqlite_error)?;
        Ok(changed)
    }
}

#[cfg(feature = "sqlite")]
fn sqlite_error(error: rusqlite::Error) -> SyncError {
    SyncError::Sqlite(error.to_string())
}

#[cfg(feature = "sqlite")]
impl EventStore for SqliteEventStore {
    async fn append(&self, event: &SyncEvent) -> Result<(), SyncError> {
        self.append_tx(event).map(|_| ())
    }

    async fn events_since(
        &self,
        session: &SessionId,
        after_seq: u64,
    ) -> Result<Vec<SyncEvent>, SyncError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(
                "SELECT session_id, event_id, seq, device_id, kind, payload, created_at_ms, fence
                   FROM events
                  WHERE session_id = ?1 AND seq > ?2
                  ORDER BY seq ASC",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                rusqlite::params![session.as_str(), after_seq as i64],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                    ))
                },
            )
            .map_err(sqlite_error)?;

        let mut events = Vec::new();
        for row in rows {
            let (session_id, event_id, seq, device_id, kind, payload, created_at_ms, fence) =
                row.map_err(sqlite_error)?;
            let event_id = uuid::Uuid::parse_str(&event_id)
                .map_err(|error| SyncError::Sqlite(format!("event id: {error}")))?;
            let kind = serde_json::from_str(&kind).map_err(SyncError::Encode)?;
            let payload = serde_json::from_str(&payload).map_err(SyncError::Encode)?;
            events.push(SyncEvent {
                session_id: SessionId::new(session_id),
                event_id,
                seq: seq as u64,
                device_id: crate::DeviceId::new(device_id),
                kind,
                payload,
                created_at_ms: created_at_ms as u64,
                fence: fence.map(|fence| fence as u64),
            });
        }
        Ok(events)
    }

    async fn last_seq(&self, session: &SessionId) -> Result<u64, SyncError> {
        let conn = self.lock()?;
        let highest: Option<i64> = conn
            .query_row(
                "SELECT MAX(seq) FROM events WHERE session_id = ?1",
                rusqlite::params![session.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        Ok(highest.unwrap_or(0) as u64)
    }

    async fn sessions(&self) -> Result<Vec<SessionId>, SyncError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare("SELECT DISTINCT session_id FROM events ORDER BY session_id")
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sqlite_error)?;
        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(SessionId::new(row.map_err(sqlite_error)?));
        }
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
        cursors.insert(session.as_str().to_owned(), position);
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

        assert_eq!(
            store
                .last_seq(&SessionId::new("s"))
                .await
                .expect("test fixture"),
            3
        );
        let all = store
            .events_since(&SessionId::new("s"), 0)
            .await
            .expect("test fixture");
        assert_eq!(all.len(), 3);
        let tail = store
            .events_since(&SessionId::new("s"), 2)
            .await
            .expect("test fixture");
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
                .expect("test fixture")
                .len(),
            1
        );
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn sqlite_satisfies_the_same_contract() {
        let store = SqliteEventStore::in_memory().expect("open");
        for seq in 1..=3 {
            store.append(&event("s", seq)).await.expect("append");
        }
        // Idempotent by event id, exactly like the file store.
        let first = event("s", 1);
        store.append(&first).await.expect("retry");
        store.append(&first).await.expect("retry again");

        assert_eq!(store.last_seq(&SessionId::new("s")).await.expect("last"), 3);
        assert_eq!(
            store
                .events_since(&SessionId::new("s"), 2)
                .await
                .expect("tail")
                .len(),
            1
        );
        assert_eq!(store.sessions().await.expect("sessions").len(), 1);
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn an_uncommitted_write_is_invisible_after_a_crash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("events.sqlite");
        {
            let store = SqliteEventStore::open(&path).expect("open");
            store.append(&event("s", 1)).await.expect("durable");

            // A process killed mid-append looks like an open transaction that
            // never commits. The next opener must not see the partial row.
            {
                let conn = store.lock().expect("lock");
                conn.execute_batch(
                    "BEGIN;
                     INSERT INTO events
                         (session_id, event_id, seq, device_id, kind, payload, created_at_ms, fence)
                     VALUES ('s', 'half-written', 2, 'laptop', '\"UserMessage\"', '{}', 0, NULL);",
                )
                .expect("uncommitted insert");
            }
            drop(store);
        }

        let reopened = SqliteEventStore::open(&path).expect("reopen");
        let events = reopened
            .events_since(&SessionId::new("s"), 0)
            .await
            .expect("read");
        assert_eq!(events.len(), 1, "the half-written event must not survive");
        assert_eq!(events[0].seq, 1);
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn both_backends_build_the_same_conversation() {
        use crate::{EventKind, message_event, transcript};

        let dir = tempfile::tempdir().expect("tempdir");
        let file = FileEventStore::new(dir.path());
        let sqlite = SqliteEventStore::in_memory().expect("open");

        let question = message_event(
            SessionId::new("chat"),
            DeviceId::new("laptop"),
            1,
            EventKind::UserMessage,
            "hello",
            None,
            None,
        )
        .with_fence(1);
        let answer = message_event(
            SessionId::new("chat"),
            DeviceId::new("laptop"),
            2,
            EventKind::AssistantMessage,
            "hi",
            Some(question.event_id),
            None,
        )
        .with_fence(1);

        for event in [&question, &answer] {
            file.append(event).await.expect("file append");
            sqlite.append(event).await.expect("sqlite append");
        }

        let from_file = file
            .events_since(&SessionId::new("chat"), 0)
            .await
            .expect("file read");
        let from_sqlite = sqlite
            .events_since(&SessionId::new("chat"), 0)
            .await
            .expect("sqlite read");

        assert_eq!(
            from_file, from_sqlite,
            "same events, same fence, same order"
        );
        assert_eq!(transcript(&from_file), transcript(&from_sqlite));
        assert_eq!(from_sqlite[0].fence, Some(1));
    }

    #[tokio::test]
    async fn lists_sessions_and_tracks_cursors_per_device() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileEventStore::new(dir.path());
        store.append(&event("a", 1)).await.expect("test fixture");
        store.append(&event("b", 1)).await.expect("test fixture");

        let sessions = store.sessions().await.expect("test fixture");
        assert_eq!(sessions.len(), 2);

        let cursors = CursorStore::new(dir.path(), &DeviceId::new("laptop"));
        cursors
            .set(&SessionId::new("a"), 1)
            .await
            .expect("test fixture");
        let loaded = cursors.load().await.expect("test fixture");
        assert_eq!(loaded.get("a"), Some(&1));
    }
}

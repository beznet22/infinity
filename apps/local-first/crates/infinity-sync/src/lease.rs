//! Session leases: one device writes a session at a time.
//!
//! Leases are **events**, not coordinator state: a claim, renewal or release is
//! appended to the same log everything else uses, so every device derives the
//! same answer from the same history. That keeps the relay dumb (D31) and means
//! an offline device still knows who owns a session as of its last sync.

use serde::{Deserialize, Serialize};

use crate::types::{DeviceId, EventKind, SessionId, SyncEvent};

/// Default lease length. Long enough to cover a turn, short enough that a dead
/// device does not block the session for long.
pub const DEFAULT_TTL_MS: u64 = 60_000;

/// What a lease event does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseAction {
    Claim,
    Renew,
    Release,
}

/// Payload carried by [`EventKind::LeaseClaim`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeasePayload {
    pub owner: DeviceId,
    pub action: LeaseAction,
    /// Wall-clock expiry. Only meaningful for `claim` and `renew`.
    pub expires_at_ms: u64,
    /// Monotonic per session. Every claim or renewal increments it; a writer
    /// holding an older fence has been superseded.
    #[serde(default)]
    pub fence: u64,
}

/// Who holds the session right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseState {
    pub owner: DeviceId,
    pub expires_at_ms: u64,
    /// The token the current holder must stamp on its writes.
    pub fence: u64,
}

impl LeaseState {
    pub fn is_active(&self, now_ms: u64) -> bool {
        self.expires_at_ms > now_ms
    }
}

/// Build a lease event. The caller owns sequencing and appending.
pub fn lease_event(
    session: SessionId,
    device: DeviceId,
    seq: u64,
    action: LeaseAction,
    ttl_ms: u64,
    now_ms: u64,
    fence: u64,
) -> SyncEvent {
    let expires_at_ms = match action {
        LeaseAction::Release => 0,
        LeaseAction::Claim | LeaseAction::Renew => now_ms.saturating_add(ttl_ms),
    };
    SyncEvent::new(
        session,
        device.clone(),
        seq,
        EventKind::LeaseClaim,
        serde_json::json!(LeasePayload {
            owner: device,
            action,
            expires_at_ms,
            fence,
        }),
    )
    .with_fence(fence)
}

/// The next fence for a session: one more than any seen so far.
pub fn next_fence(events: &[SyncEvent]) -> u64 {
    events
        .iter()
        .filter(|event| event.kind == EventKind::LeaseClaim)
        .filter_map(|event| serde_json::from_value::<LeasePayload>(event.payload.clone()).ok())
        .map(|payload| payload.fence)
        .max()
        .unwrap_or(0)
        + 1
}

/// The current holder as of `now_ms`, derived from the log alone.
pub fn current_lease(events: &[SyncEvent], now_ms: u64) -> Option<LeaseState> {
    let mut state: Option<LeaseState> = None;
    for event in events
        .iter()
        .filter(|event| event.kind == EventKind::LeaseClaim)
    {
        let Ok(payload) = serde_json::from_value::<LeasePayload>(event.payload.clone()) else {
            continue;
        };
        match payload.action {
            LeaseAction::Release => state = None,
            LeaseAction::Claim | LeaseAction::Renew => {
                state = if payload.expires_at_ms > now_ms {
                    Some(LeaseState {
                        owner: payload.owner,
                        expires_at_ms: payload.expires_at_ms,
                        fence: payload.fence,
                    })
                } else {
                    None
                };
            }
        }
    }
    state
}

/// Whether `device` may act on the session: nobody holds it, or it holds it.
pub fn may_write(events: &[SyncEvent], device: &DeviceId, now_ms: u64) -> bool {
    match current_lease(events, now_ms) {
        None => true,
        Some(lease) => lease.owner == *device,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str) -> DeviceId {
        DeviceId::new(name)
    }

    fn session() -> SessionId {
        SessionId::new("s")
    }

    #[test]
    fn claim_renew_expire_release() {
        let laptop = device("laptop");
        let phone = device("phone");

        let claim = lease_event(
            session(),
            laptop.clone(),
            1,
            LeaseAction::Claim,
            1_000,
            100,
            1,
        );
        let events = vec![claim];
        assert_eq!(
            current_lease(&events, 200).map(|lease| lease.owner),
            Some(laptop.clone())
        );
        assert!(may_write(&events, &laptop, 200));
        assert!(!may_write(&events, &phone, 200));

        assert_eq!(current_lease(&events, 2_000), None);
        assert!(may_write(&events, &phone, 2_000));

        let renew = lease_event(
            session(),
            laptop.clone(),
            2,
            LeaseAction::Renew,
            1_000,
            500,
            2,
        );
        let renewed = vec![renew];
        assert_eq!(
            current_lease(&renewed, 1_200).map(|lease| lease.owner),
            Some(laptop.clone())
        );
        assert_eq!(current_lease(&renewed, 1_600), None);

        let release = lease_event(session(), laptop, 3, LeaseAction::Release, 1_000, 700, 2);
        assert_eq!(current_lease(&[release], 800), None);
    }

    #[test]
    fn the_last_claim_wins() {
        let laptop = device("laptop");
        let phone = device("phone");
        let events = vec![
            lease_event(session(), laptop, 1, LeaseAction::Claim, 5_000, 100, 1),
            lease_event(
                session(),
                phone.clone(),
                1,
                LeaseAction::Claim,
                5_000,
                200,
                2,
            ),
        ];
        assert_eq!(current_lease(&events, 300).map(|l| l.owner), Some(phone));
    }

    #[test]
    fn unknown_payloads_are_ignored() {
        let broken = SyncEvent::new(
            session(),
            device("x"),
            1,
            EventKind::LeaseClaim,
            serde_json::json!({ "nonsense": true }),
        );
        assert_eq!(current_lease(&[broken], 0), None);
    }
}

/// A held lease, for use around one turn.
///
/// Hosts acquire before starting a turn (`None` means another device owns the
/// session, so start nothing), renew while a long turn is in flight, and release
/// when finished. Dropping the guard does **not** release: release is an event,
/// and events need an await.
pub struct LeaseGuard {
    session: SessionId,
    device: DeviceId,
    ttl_ms: u64,
    fence: u64,
}

impl LeaseGuard {
    /// Claim the session for `device`, or report that someone else holds it.
    pub async fn acquire<S: crate::EventStore>(
        store: &S,
        session: SessionId,
        device: DeviceId,
        ttl_ms: u64,
    ) -> Result<Option<Self>, crate::SyncError> {
        let now = crate::now_ms();
        let events = store.events_since(&session, 0).await?;
        if !may_write(&events, &device, now) {
            return Ok(None);
        }

        let seq = store.last_seq(&session).await? + 1;
        let fence = next_fence(&events);
        let event = lease_event(
            session.clone(),
            device.clone(),
            seq,
            LeaseAction::Claim,
            ttl_ms,
            now,
            fence,
        );
        store.append(&event).await?;
        Ok(Some(Self {
            session,
            device,
            ttl_ms,
            fence,
        }))
    }

    pub fn device(&self) -> &DeviceId {
        &self.device
    }

    /// The token this holder stamps on its writes.
    pub fn fence(&self) -> u64 {
        self.fence
    }

    /// Extend the lease from the turn's own heartbeat.
    pub async fn renew<S: crate::EventStore>(&mut self, store: &S) -> Result<(), crate::SyncError> {
        let events = store.events_since(&self.session, 0).await?;
        let expected = self.fence + 1;

        // If anyone claimed since our last renewal, the next fence is past ours:
        // we were superseded, so we must not write anything more.
        if next_fence(&events) != expected {
            return Err(crate::SyncError::LeaseLost);
        }

        let seq = store.last_seq(&self.session).await? + 1;
        let event = lease_event(
            self.session.clone(),
            self.device.clone(),
            seq,
            LeaseAction::Renew,
            self.ttl_ms,
            crate::now_ms(),
            expected,
        );
        store.append(&event).await?;
        self.fence = expected;
        Ok(())
    }

    /// Give the session back so another device can take it immediately.
    pub async fn release<S: crate::EventStore>(self, store: &S) -> Result<(), crate::SyncError> {
        let seq = store.last_seq(&self.session).await? + 1;
        let event = lease_event(
            self.session.clone(),
            self.device.clone(),
            seq,
            LeaseAction::Release,
            self.ttl_ms,
            crate::now_ms(),
            self.fence,
        );
        store.append(&event).await
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;
    use crate::{EventStore, FileEventStore};

    fn session() -> SessionId {
        SessionId::new("guard")
    }

    #[tokio::test]
    async fn only_one_device_holds_the_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileEventStore::new(dir.path());
        let laptop = DeviceId::new("laptop");
        let phone = DeviceId::new("phone");

        let guard = LeaseGuard::acquire(&store, session(), laptop.clone(), 60_000)
            .await
            .expect("acquire")
            .expect("free session");
        assert_eq!(guard.device(), &laptop);

        let denied = LeaseGuard::acquire(&store, session(), phone.clone(), 60_000)
            .await
            .expect("acquire");
        assert!(denied.is_none(), "the second device must not start a turn");

        guard.release(&store).await.expect("release");
        let phone_guard = LeaseGuard::acquire(&store, session(), phone, 60_000)
            .await
            .expect("acquire");
        assert!(phone_guard.is_some());
    }

    #[tokio::test]
    async fn an_expired_lease_can_be_taken() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileEventStore::new(dir.path());

        let _guard = LeaseGuard::acquire(&store, session(), DeviceId::new("laptop"), 0)
            .await
            .expect("acquire")
            .expect("free session");

        let taken = LeaseGuard::acquire(&store, session(), DeviceId::new("phone"), 60_000)
            .await
            .expect("acquire");
        assert!(taken.is_some(), "an expired lease must not block a turn");
    }

    #[tokio::test]
    async fn a_superseded_holder_cannot_renew() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileEventStore::new(dir.path());
        let laptop = DeviceId::new("laptop");
        let phone = DeviceId::new("phone");

        // Laptop claims, then lets the lease lapse (zero TTL) so the phone can.
        let mut laptop_guard = LeaseGuard::acquire(&store, session(), laptop, 0)
            .await
            .expect("acquire")
            .expect("free session");
        let phone_guard = LeaseGuard::acquire(&store, session(), phone, 60_000)
            .await
            .expect("acquire")
            .expect("expired lease is free");
        assert_eq!(phone_guard.fence(), laptop_guard.fence() + 1);

        // The laptop no longer holds the session: its renewal must be refused
        // rather than silently writing a claim it lost.
        let error = laptop_guard
            .renew(&store)
            .await
            .expect_err("renew must fail");
        assert!(matches!(error, crate::SyncError::LeaseLost));
    }

    #[tokio::test]
    async fn renewal_extends_the_hold() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileEventStore::new(dir.path());
        let laptop = DeviceId::new("laptop");
        let phone = DeviceId::new("phone");

        let mut guard = LeaseGuard::acquire(&store, session(), laptop.clone(), 1_000)
            .await
            .expect("acquire")
            .expect("free session");
        guard.renew(&store).await.expect("renew");

        let events = store.events_since(&session(), 0).await.expect("events");
        assert_eq!(
            current_lease(&events, crate::now_ms()).map(|lease| lease.owner),
            Some(laptop)
        );

        let denied = LeaseGuard::acquire(&store, session(), phone, 1_000)
            .await
            .expect("acquire");
        assert!(denied.is_none());
    }
}

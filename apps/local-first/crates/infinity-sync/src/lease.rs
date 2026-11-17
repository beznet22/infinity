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
}

/// Who holds the session right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseState {
    pub owner: DeviceId,
    pub expires_at_ms: u64,
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
        }),
    )
}

/// The current holder as of `now_ms`, derived from the log alone.
pub fn current_lease(events: &[SyncEvent], now_ms: u64) -> Option<LeaseState> {
    let mut state: Option<LeaseState> = None;
    for event in events.iter().filter(|event| event.kind == EventKind::LeaseClaim) {
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

        let claim = lease_event(session(), laptop.clone(), 1, LeaseAction::Claim, 1_000, 100);
        let events = vec![claim];
        assert_eq!(
            current_lease(&events, 200).map(|lease| lease.owner),
            Some(laptop.clone())
        );
        assert!(may_write(&events, &laptop, 200));
        assert!(!may_write(&events, &phone, 200));

        assert_eq!(current_lease(&events, 2_000), None);
        assert!(may_write(&events, &phone, 2_000));

        let renew = lease_event(session(), laptop.clone(), 2, LeaseAction::Renew, 1_000, 500);
        let renewed = vec![renew];
        assert_eq!(
            current_lease(&renewed, 1_200).map(|lease| lease.owner),
            Some(laptop.clone())
        );
        assert_eq!(current_lease(&renewed, 1_600), None);

        let release = lease_event(session(), laptop, 3, LeaseAction::Release, 1_000, 700);
        assert_eq!(current_lease(&[release], 800), None);
    }

    #[test]
    fn the_last_claim_wins() {
        let laptop = device("laptop");
        let phone = device("phone");
        let events = vec![
            lease_event(session(), laptop, 1, LeaseAction::Claim, 5_000, 100),
            lease_event(session(), phone.clone(), 1, LeaseAction::Claim, 5_000, 200),
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

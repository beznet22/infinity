//! Emit a deterministic event log plus the `SessionView` the Rust fold produces.
//!
//! The browser port must reproduce this exact view (see `browser/view/test.mjs`),
//! which is what keeps one contract across two languages (D61).

use infinity_sync::{
    DeviceId, EventKind, SessionId, SyncEvent, dequeue_event, enqueue_event, message_event,
    now_ms, session_view, truncate_event,
};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn main() {
    let session = SessionId::new("chat");
    let device = DeviceId::new("laptop");

    // Survives the truncation below, so the parity check covers a visible row.
    let u0 = message_event(session.clone(), device.clone(), 8, EventKind::UserMessage, "hello", None, None);
    let u0 = SyncEvent { event_id: id(8), ..u0 };
    let u1 = message_event(session.clone(), device.clone(), 1, EventKind::UserMessage, "what changed?", None, None);
    let u1 = SyncEvent { event_id: id(1), ..u1 };
    let a1 = message_event(session.clone(), device.clone(), 2, EventKind::AssistantMessage, "the log", Some(u1.event_id), None);
    let a1 = SyncEvent { event_id: id(2), ..a1 };
    let u1b = message_event(session.clone(), device.clone(), 3, EventKind::UserMessage, "what changed, exactly?", None, Some(u1.event_id));
    let u1b = SyncEvent { event_id: id(3), ..u1b };
    let q1 = enqueue_event(session.clone(), device.clone(), 4, "also check the queue", None);
    let q1 = SyncEvent { event_id: id(4), ..q1 };
    let q2 = enqueue_event(session.clone(), device.clone(), 5, "never mind", None);
    let q2 = SyncEvent { event_id: id(5), ..q2 };
    let dq = dequeue_event(session.clone(), device.clone(), 6, q2.event_id);
    let dq = SyncEvent { event_id: id(6), ..dq };
    let tr = truncate_event(session.clone(), device.clone(), 7, a1.event_id);
    let tr = SyncEvent { event_id: id(7), ..tr };

    let events = vec![u0, u1, a1, u1b, q1, q2, dq, tr];
    let view = session_view(&session, &events, &device, now_ms());
    let document = serde_json::json!({
        "session": session.as_str(),
        "device": device.as_str(),
        "events": events,
        "view": view,
    });
    println!("{}", serde_json::to_string_pretty(&document).expect("serialize"));
}

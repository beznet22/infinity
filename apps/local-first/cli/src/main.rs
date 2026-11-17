//! One device in the sync end-to-end test.
//!
//! ```text
//! sync-device --root DIR --device ID [--relay URL] write --session S --text "..."
//! sync-device --root DIR --device ID --relay URL push
//! sync-device --root DIR --device ID --relay URL pull  --session S
//! sync-device --root DIR --device ID sessions
//! ```
//!
//! Each device owns its own `--root`, which is exactly how the multi-device test
//! simulates a phone and a laptop.

use std::path::PathBuf;

use infinity_sync::http::HttpTransport;
use infinity_sync::{
    CursorStore, DeviceId, EventKind, EventStore, FileEventStore, LeaseAction, LeaseGuard,
    SessionId, SyncEvent, current_lease, lease_event, next_fence, now_ms, replay, snapshot_event,
};

struct Args {
    root: PathBuf,
    device: DeviceId,
    relay: Option<String>,
    command: String,
    session: Option<SessionId>,
    text: Option<String>,
    ttl_ms: Option<u64>,
    state: Option<String>,
}

fn parse() -> Result<Args, String> {
    let mut root = None;
    let mut device = None;
    let mut relay = None;
    let mut command = None;
    let mut session = None;
    let mut text = None;
    let mut ttl_ms = None;
    let mut state = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut take = |name: &str| -> Result<String, String> {
            args.next().ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--root" => root = Some(PathBuf::from(take("--root")?)),
            "--device" => device = Some(DeviceId::new(take("--device")?)),
            "--relay" => relay = Some(take("--relay")?),
            "--session" => session = Some(SessionId::new(take("--session")?)),
            "--text" => text = Some(take("--text")?),
            "--ttl" => {
                ttl_ms = Some(
                    take("--ttl")?
                        .parse()
                        .map_err(|_| "--ttl must be milliseconds".to_owned())?,
                )
            }
            "--state" => state = Some(take("--state")?),
            other if command.is_none() => command = Some(other.to_owned()),
            other => return Err(format!("unexpected argument {other}")),
        }
    }

    Ok(Args {
        root: root.ok_or("--root is required")?,
        device: device.ok_or("--device is required")?,
        relay,
        command: command.ok_or("a command is required")?,
        session,
        text,
        ttl_ms,
        state,
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    let args = parse()?;
    let store = FileEventStore::new(&args.root);
    let transport = match &args.relay {
        Some(url) => Some(HttpTransport::new(url.clone()).map_err(|e| e.to_string())?),
        None => None,
    };

    match args.command.as_str() {
        "write" => {
            let session = args.session.clone().ok_or("write needs --session")?;
            let text = args.text.clone().ok_or("write needs --text")?;
            let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
            // Stamp the writer's fencing token when it holds the lease; the relay
            // drops writes that carry an older one.
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            let fence = current_lease(&events, now_ms())
                .filter(|lease| lease.owner == args.device)
                .map(|lease| lease.fence);
            let mut event = SyncEvent::new(
                session.clone(),
                args.device.clone(),
                seq,
                EventKind::UserMessage,
                serde_json::json!({ "text": text, "at": now_ms() }),
            );
            if let Some(fence) = fence {
                event = event.with_fence(fence);
            }
            store.append(&event).await.map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({ "session": session.as_str(), "seq": seq, "event": event.event_id })
            );
        }
        "push" => {
            let transport = transport.ok_or("push needs --relay")?;
            let mut pushed = 0usize;
            let sessions = store.sessions().await.map_err(|e| e.to_string())?;
            for session in sessions {
                let events = store
                    .events_since(&session, 0)
                    .await
                    .map_err(|e| e.to_string())?;
                let ack = transport.push(&events).await.map_err(|e| e.to_string())?;
                pushed += ack.accepted;
            }
            println!("{}", serde_json::json!({ "accepted": pushed }));
        }
        "pull" => {
            let transport = transport.ok_or("pull needs --relay")?;
            let session = args.session.clone().ok_or("pull needs --session")?;
            let cursors = CursorStore::new(&args.root, &args.device);
            let after = cursors
                .load()
                .await
                .map_err(|e| e.to_string())?
                .get(session.as_str())
                .copied()
                .unwrap_or(0);

            let stored = transport
                .pull(&session, after)
                .await
                .map_err(|e| e.to_string())?;
            let mut highest = after;
            for item in &stored {
                store.append(&item.event).await.map_err(|e| e.to_string())?;
                highest = highest.max(item.position);
            }
            cursors
                .set(&session, highest)
                .await
                .map_err(|e| e.to_string())?;

            println!(
                "{}",
                serde_json::json!({ "session": session.as_str(), "fetched": stored.len(), "position": highest, "events": store.events_since(&session, 0).await.map_err(|e| e.to_string())?.len() })
            );
        }
        "lease" => {
            // Default action is a claim; `LocalLeaseAction` is derived from the
            // presence of --ttl only, so the demo stays explicit and small.
            let session = args.session.clone().ok_or("lease needs --session")?;
            let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            let event = lease_event(
                session.clone(),
                args.device.clone(),
                seq,
                LeaseAction::Claim,
                args.ttl_ms.unwrap_or(infinity_sync::DEFAULT_TTL_MS),
                now_ms(),
                next_fence(&events),
            );
            store.append(&event).await.map_err(|e| e.to_string())?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            let holder = current_lease(&events, now_ms()).map(|lease| lease.owner.to_string());
            println!(
                "{}",
                serde_json::json!({ "session": session.as_str(), "seq": seq, "holder": holder })
            );
        }
        "guard" => {
            // Acquire-or-refuse, exactly what a host does before starting a turn.
            let session = args.session.clone().ok_or("guard needs --session")?;
            let ttl = args.ttl_ms.unwrap_or(infinity_sync::DEFAULT_TTL_MS);
            let acquired = LeaseGuard::acquire(&store, session.clone(), args.device.clone(), ttl)
                .await
                .map_err(|e| e.to_string())?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            let holder = current_lease(&events, now_ms()).map(|lease| lease.owner.to_string());
            println!(
                "{}",
                serde_json::json!({
                    "acquired": acquired.is_some(),
                    "device": args.device.as_str(),
                    "holder": holder,
                })
            );
        }
        "unguard" => {
            let session = args.session.clone().ok_or("unguard needs --session")?;
            let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            let event = lease_event(
                session.clone(),
                args.device.clone(),
                seq,
                LeaseAction::Release,
                args.ttl_ms.unwrap_or(infinity_sync::DEFAULT_TTL_MS),
                now_ms(),
                current_lease(&events, now_ms())
                    .map(|lease| lease.fence)
                    .unwrap_or(0),
            );
            store.append(&event).await.map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({ "session": session.as_str(), "released": true })
            );
        }
        "snapshot" => {
            let session = args.session.clone().ok_or("snapshot needs --session")?;
            let state: serde_json::Value = serde_json::from_str(
                args.state
                    .as_deref()
                    .ok_or("snapshot needs --state (JSON)")?,
            )
            .map_err(|error| format!("--state must be JSON: {error}"))?;
            let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
            let covered = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?
                .len();
            let event = snapshot_event(session.clone(), args.device.clone(), seq, state, covered);
            store.append(&event).await.map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({ "session": session.as_str(), "seq": seq, "covered": covered })
            );
        }
        "replay" => {
            let session = args.session.clone().ok_or("replay needs --session")?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            let (state, tail) = replay(&events);
            println!(
                "{}",
                serde_json::json!({
                    "session": session.as_str(),
                    "has_snapshot": state.is_some(),
                    "tail": tail.len(),
                })
            );
        }
        "sessions" => {
            let sessions = store.sessions().await.map_err(|e| e.to_string())?;
            let names: Vec<&str> = sessions.iter().map(|s| s.as_str()).collect();
            println!("{}", serde_json::json!({ "sessions": names }));
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

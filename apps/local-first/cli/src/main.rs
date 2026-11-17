//! One device in the sync end-to-end test.
//!
//! ```text
//! sync-device --root DIR --device ID [--relay URL] write --session S --text "..."
//! sync-device --root DIR --device ID --relay URL push
//! sync-device --root DIR --device ID --relay URL pull  --session S
//! sync-device --root DIR --device ID sessions
//! sync-device --root DIR --device ID write --session S --text "..." [--assistant --reply-to E]
//! sync-device --root DIR --device ID edit  --session S --event E --text "..."
//! sync-device --root DIR --device ID thread --session S
//! sync-device --root DIR --device ID enqueue --session S --text "..."   # while a turn runs
//! sync-device --root DIR --device ID dequeue --session S --event E
//! sync-device --root DIR --device ID send    --session S --event E
//! sync-device --root DIR --device ID queue   --session S
//! sync-device --root DIR --device ID truncate --session S --event E      # delete from here
//! sync-device --root DIR --device ID restore  --session S --truncation T
//! ```
//!
//! Each device owns its own `--root`, which is exactly how the multi-device test
//! simulates a phone and a laptop.

use std::path::PathBuf;

use infinity_sync::http::HttpTransport;
use infinity_sync::{
    CursorStore, DeviceId, EventKind, EventStore, FileEventStore, LeaseAction, LeaseGuard, Role,
    SessionId, SyncEvent, current_lease, dequeue_event, enqueue_event, lease_event, message,
    message_event, next_fence, now_ms, queue, replay, restore_event, send_event, snapshot_event,
    summarize, transcript, truncate_event, truncations, versions,
};
use uuid::Uuid;

struct Args {
    root: PathBuf,
    device: DeviceId,
    relay: Option<String>,
    command: String,
    session: Option<SessionId>,
    text: Option<String>,
    ttl_ms: Option<u64>,
    state: Option<String>,
    event: Option<Uuid>,
    reply_to: Option<Uuid>,
    truncation: Option<Uuid>,
    interval: u64,
    iterations: u64,
    assistant: bool,
}

/// Stamp the writer's fencing token when it currently holds the session.
async fn stamp_fence<S: EventStore>(
    store: &S,
    session: &SessionId,
    device: &DeviceId,
    event: SyncEvent,
) -> Result<SyncEvent, infinity_sync::SyncError> {
    let events = store.events_since(session, 0).await?;
    Ok(match current_lease(&events, now_ms()) {
        Some(lease) if &lease.owner == device => event.with_fence(lease.fence),
        _ => event,
    })
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
    let mut event = None;
    let mut reply_to = None;
    let mut truncation = None;
    let mut interval = 5u64;
    let mut iterations = 0u64;
    let mut assistant = false;

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
            "--event" => {
                event = Some(
                    take("--event")?
                        .parse()
                        .map_err(|_| "--event must be a UUID".to_owned())?,
                )
            }
            "--reply-to" => {
                reply_to = Some(
                    take("--reply-to")?
                        .parse()
                        .map_err(|_| "--reply-to must be a UUID".to_owned())?,
                )
            }
            "--truncation" => {
                truncation = Some(
                    take("--truncation")?
                        .parse()
                        .map_err(|_| "--truncation must be a UUID".to_owned())?,
                )
            }
            "--interval" => {
                interval = take("--interval")?
                    .parse()
                    .map_err(|_| "--interval must be seconds".to_owned())?
            }
            "--iterations" => {
                iterations = take("--iterations")?
                    .parse()
                    .map_err(|_| "--iterations must be a number".to_owned())?
            }
            "--assistant" => assistant = true,
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
        event,
        reply_to,
        truncation,
        interval,
        iterations,
        assistant,
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
            let kind = if args.assistant {
                EventKind::AssistantMessage
            } else {
                EventKind::UserMessage
            };
            let event = message_event(
                session.clone(),
                args.device.clone(),
                seq,
                kind,
                text,
                args.reply_to,
                None,
            );
            let event = stamp_fence(&store, &session, &args.device, event)
                .await
                .map_err(|e| e.to_string())?;
            store.append(&event).await.map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({ "session": session.as_str(), "seq": seq, "event": event.event_id, "reply_to": args.reply_to })
            );
        }
        "edit" => {
            // Editing forks the thread at the edited message: the new version
            // replaces the old text but keeps its parent, so the answers to the
            // old text do not carry over.
            let session = args.session.clone().ok_or("edit needs --session")?;
            let text = args.text.clone().ok_or("edit needs --text")?;
            let target = args
                .event
                .ok_or("edit needs --event (the message to replace)")?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            let original = message(&events, target).ok_or("no message with that --event id")?;
            let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
            let event = message_event(
                session.clone(),
                args.device.clone(),
                seq,
                EventKind::UserMessage,
                text,
                original.reply_to,
                Some(target),
            );
            let event = stamp_fence(&store, &session, &args.device, event)
                .await
                .map_err(|e| e.to_string())?;
            store.append(&event).await.map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({
                    "session": session.as_str(),
                    "replaces": target,
                    "event": event.event_id,
                    "versions": versions(&events, target).len() + 1,
                })
            );
        }
        "thread" => {
            let session = args.session.clone().ok_or("thread needs --session")?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            let rows: Vec<serde_json::Value> = transcript(&events)
                .into_iter()
                .map(|row| {
                    serde_json::json!({
                        "role": match row.role { Role::User => "user", Role::Assistant => "assistant" },
                        "text": row.text,
                        "versions": row.versions,
                        "event": row.event_id,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({ "session": session.as_str(), "messages": rows })
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
        "inbox" => {
            let sessions = store.sessions().await.map_err(|e| e.to_string())?;
            let mut rows = Vec::new();
            for session in sessions {
                let events = store
                    .events_since(&session, 0)
                    .await
                    .map_err(|e| e.to_string())?;
                let summary = summarize(&session, &events, &args.device, now_ms());
                rows.push(serde_json::json!({
                    "session": summary.session.as_str(),
                    "messages": summary.messages,
                    "last_seq": summary.last_seq,
                    "holder": summary.holder.as_ref().map(|h| h.to_string()),
                    "read_only": summary.read_only,
                }));
            }
            println!(
                "{}",
                serde_json::json!({ "device": args.device.as_str(), "sessions": rows })
            );
        }
        "resume" => {
            // What another device sees: the transcript, how far the log goes, and
            // whether this device may write.
            let session = args.session.clone().ok_or("resume needs --session")?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            let summary = summarize(&session, &events, &args.device, now_ms());
            let rows: Vec<serde_json::Value> = transcript(&events)
                .into_iter()
                .map(|row| {
                    serde_json::json!({
                        "role": match row.role { Role::User => "user", Role::Assistant => "assistant" },
                        "text": row.text,
                        "versions": row.versions,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "session": summary.session.as_str(),
                    "device": args.device.as_str(),
                    "messages": summary.messages,
                    "last_seq": summary.last_seq,
                    "holder": summary.holder.as_ref().map(|h| h.to_string()),
                    "read_only": summary.read_only,
                    "transcript": rows,
                })
            );
        }
        "enqueue" => {
            let session = args.session.clone().ok_or("enqueue needs --session")?;
            let text = args.text.clone().ok_or("enqueue needs --text")?;
            let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
            let event = enqueue_event(
                session.clone(),
                args.device.clone(),
                seq,
                text,
                args.reply_to,
            );
            let event = stamp_fence(&store, &session, &args.device, event)
                .await
                .map_err(|e| e.to_string())?;
            store.append(&event).await.map_err(|e| e.to_string())?;
            let waiting = queue(
                &store
                    .events_since(&session, 0)
                    .await
                    .map_err(|e| e.to_string())?,
            );
            println!(
                "{}",
                serde_json::json!({ "queued": event.event_id, "waiting": waiting.len() })
            );
        }
        "dequeue" => {
            let session = args.session.clone().ok_or("dequeue needs --session")?;
            let target = args.event.ok_or("dequeue needs --event")?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            if !queue(&events).iter().any(|entry| entry.event_id == target) {
                return Err("that message is not queued".to_owned());
            }
            let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
            let event = dequeue_event(session.clone(), args.device.clone(), seq, target);
            let event = stamp_fence(&store, &session, &args.device, event)
                .await
                .map_err(|e| e.to_string())?;
            store.append(&event).await.map_err(|e| e.to_string())?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({ "dequeued": target, "waiting": queue(&events).len() })
            );
        }
        "send" => {
            let session = args.session.clone().ok_or("send needs --session")?;
            let target = args.event.ok_or("send needs --event")?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            if !queue(&events).iter().any(|entry| entry.event_id == target) {
                return Err("that message is not queued".to_owned());
            }
            let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
            let event = send_event(session.clone(), args.device.clone(), seq, target);
            let event = stamp_fence(&store, &session, &args.device, event)
                .await
                .map_err(|e| e.to_string())?;
            store.append(&event).await.map_err(|e| e.to_string())?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({ "sent": target, "messages": transcript(&events).len(), "waiting": queue(&events).len() })
            );
        }
        "queue" => {
            let session = args.session.clone().ok_or("queue needs --session")?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            let waiting: Vec<serde_json::Value> = queue(&events)
                .into_iter()
                .map(|entry| serde_json::json!({ "event": entry.event_id, "text": entry.text }))
                .collect();
            println!(
                "{}",
                serde_json::json!({ "session": session.as_str(), "waiting": waiting })
            );
        }
        "truncate" => {
            // Delete from a message onward. The event stays in the log; `restore`
            // brings the tail back, and another device sees the same deletion.
            let session = args.session.clone().ok_or("truncate needs --session")?;
            let from = args
                .event
                .ok_or("truncate needs --event (the first message to delete)")?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            if !transcript(&events).iter().any(|row| row.event_id == from) {
                return Err("no visible message with that --event id".to_owned());
            }
            let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
            let event = truncate_event(session.clone(), args.device.clone(), seq, from);
            let event = stamp_fence(&store, &session, &args.device, event)
                .await
                .map_err(|e| e.to_string())?;
            store.append(&event).await.map_err(|e| e.to_string())?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({ "truncation": event.event_id, "from": from, "visible": transcript(&events).len() })
            );
        }
        "restore" => {
            let session = args.session.clone().ok_or("restore needs --session")?;
            let truncation = args.truncation.ok_or("restore needs --truncation")?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            if !truncations(&events)
                .iter()
                .any(|entry| entry.id == truncation && entry.active)
            {
                return Err("no active truncation with that id".to_owned());
            }
            let seq = store.last_seq(&session).await.map_err(|e| e.to_string())? + 1;
            let event = restore_event(session.clone(), args.device.clone(), seq, truncation);
            let event = stamp_fence(&store, &session, &args.device, event)
                .await
                .map_err(|e| e.to_string())?;
            store.append(&event).await.map_err(|e| e.to_string())?;
            let events = store
                .events_since(&session, 0)
                .await
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({ "restored": truncation, "visible": transcript(&events).len() })
            );
        }
        "watch" => {
            // Polling, not push: the relay has no channel to the client, so the
            // client asks again. Good enough to see another device's writes
            // arrive; a real push channel is a separate deployment decision.
            let transport = transport.ok_or("watch needs --relay")?;
            let session = args.session.clone().ok_or("watch needs --session")?;
            let cursors = CursorStore::new(&args.root, &args.device);
            let mut rounds = 0u64;
            loop {
                rounds += 1;
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
                let mut kinds = Vec::new();
                for item in &stored {
                    store.append(&item.event).await.map_err(|e| e.to_string())?;
                    highest = highest.max(item.position);
                    kinds.push(format!("{:?}", item.event.kind));
                }
                if !stored.is_empty() {
                    cursors
                        .set(&session, highest)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                println!(
                    "{}",
                    serde_json::json!({
                        "round": rounds,
                        "fetched": stored.len(),
                        "position": highest,
                        "kinds": kinds,
                    })
                );
                if args.iterations != 0 && rounds >= args.iterations {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(args.interval.max(1))).await;
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

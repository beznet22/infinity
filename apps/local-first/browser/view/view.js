// The browser's copy of the Rust fold in `infinity-sync::view`.
//
// Two implementations exist because the Rust crate cannot compile for wasm (tokio
// and rusqlite are native-only). The parity test against a Rust-generated golden
// fixture is what keeps them honest: see `test.mjs` and D61.

const USER = "user_message";
const ASSISTANT = "assistant_message";
const MESSAGE_KINDS = new Set([USER, ASSISTANT]);

function payloadOf(event) {
  return event.payload ?? {};
}

function parseMessage(event) {
  if (!MESSAGE_KINDS.has(event.kind)) return null;
  const payload = payloadOf(event);
  return {
    event_id: event.event_id,
    role: event.kind === ASSISTANT ? "assistant" : "user",
    text: String(payload.text ?? ""),
    at: Number(payload.at ?? 0),
    reply_to: payload.reply_to ?? null,
    edited_from: payload.edited_from ?? null,
  };
}

function promoted(events) {
  const set = new Set();
  for (const event of events) {
    if (event.kind === "message_sent") set.add(payloadOf(event).target);
  }
  return set;
}

function dequeued(events) {
  const set = new Set();
  for (const event of events) {
    if (event.kind === "message_dequeued") set.add(payloadOf(event).target);
  }
  return set;
}

/** Messages in transcript order, including queued ones the agent received. */
function messages(events) {
  const sent = promoted(events);
  const parsed = [];
  events.forEach((event, index) => {
    if (event.kind === "message_queued") {
      if (!sent.has(event.event_id)) return;
      const payload = payloadOf(event);
      parsed.push({
        index,
        event_id: event.event_id,
        role: "user",
        text: String(payload.text ?? ""),
        at: Number(payload.at ?? 0),
        reply_to: payload.reply_to ?? null,
        edited_from: null,
      });
      return;
    }
    const message = parseMessage(event);
    if (message) parsed.push({ index, ...message });
  });
  parsed.sort((a, b) => a.at - b.at || a.index - b.index);
  return parsed;
}

function superseeded(events) {
  const gone = new Set();
  for (const event of messages(events)) {
    if (event.edited_from) gone.add(event.edited_from);
  }
  return gone;
}

function truncations(events) {
  const restored = new Set();
  for (const event of events) {
    if (event.kind === "conversation_restored") restored.add(payloadOf(event).truncation);
  }
  return events
    .filter((event) => event.kind === "conversation_truncated")
    .map((event) => ({
      id: event.event_id,
      from: payloadOf(event).from,
      active: !restored.has(event.event_id),
    }));
}

/** Oldest version of the edit chain containing `id`. */
function chainRoot(id, byId) {
  let root = id;
  let cursor = byId.get(id)?.edited_from ?? null;
  let guard = 0;
  while (cursor && byId.has(cursor) && guard < 10_000) {
    root = cursor;
    cursor = byId.get(cursor).edited_from ?? null;
    guard += 1;
  }
  return root;
}

export function queueOf(events) {
  const sent = promoted(events);
  const dropped = dequeued(events);
  return events
    .map((event, index) => ({ event, index }))
    .filter(({ event }) => event.kind === "message_queued")
    .filter(({ event }) => !sent.has(event.event_id) && !dropped.has(event.event_id))
    .map(({ event, index }) => ({
      event: event.event_id,
      text: String(payloadOf(event).text ?? ""),
      at: Number(payloadOf(event).at ?? 0),
      index,
    }))
    .sort((a, b) => a.at - b.at || a.index - b.index)
    .map(({ event, text }) => ({ event, text }));
}

export function transcriptOf(events) {
  const all = messages(events);
  const hidden = superseeded(events);

  for (const truncation of truncations(events).filter((entry) => entry.active)) {
    const position = all.findIndex((message) => message.event_id === truncation.from);
    if (position >= 0) {
      for (const message of all.slice(position)) hidden.add(message.event_id);
    }
  }

  // A reply to a hidden message is hidden too.
  let changed = true;
  while (changed) {
    changed = false;
    for (const message of all) {
      if (message.reply_to && hidden.has(message.reply_to) && !hidden.has(message.event_id)) {
        hidden.add(message.event_id);
        changed = true;
      }
    }
  }

  const byId = new Map(all.map((message) => [message.event_id, message]));
  const groupSize = new Map();
  for (const message of all) {
    const root = chainRoot(message.event_id, byId);
    groupSize.set(root, (groupSize.get(root) ?? 0) + 1);
  }

  return all
    .filter((message) => !hidden.has(message.event_id))
    .map((message) => ({
      event: message.event_id,
      role: message.role,
      text: message.text,
      versions: groupSize.get(chainRoot(message.event_id, byId)) ?? 1,
      reply_to: message.reply_to,
    }));
}

export function currentLease(events, nowMs) {
  let state = null;
  for (const event of events) {
    if (event.kind !== "lease_claim") continue;
    const payload = payloadOf(event);
    if (payload.action === "release") {
      state = null;
    } else if (Number(payload.expires_at_ms) > nowMs) {
      state = {
        owner: payload.owner,
        expires_at_ms: Number(payload.expires_at_ms),
        fence: Number(payload.fence ?? 0),
      };
    } else {
      state = null;
    }
  }
  return state;
}

/** Mirror of `session_view` (D54). */
export function sessionView(session, device, events, nowMs) {
  const lease = currentLease(events, nowMs);
  const rows = transcriptOf(events);
  return {
    session,
    device,
    read_only: Boolean(lease && lease.owner !== device),
    holder: lease ? lease.owner : null,
    fence: lease ? lease.fence : 0,
    last_seq: events.reduce((highest, event) => Math.max(highest, Number(event.seq ?? 0)), 0),
    messages: rows,
    queue: queueOf(events),
    truncations: truncations(events),
  };
}

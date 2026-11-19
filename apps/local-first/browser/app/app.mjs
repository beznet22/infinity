import { renderThreads } from "../renderer/renderer.js";
import { sessionView } from "../view/view.js";

const SESSION = "browser-chat";
const DEVICE = "browser";
const app = document.getElementById("app");
const status = document.getElementById("status");

const worker = new Worker(new URL("./db-worker.mjs", import.meta.url), { type: "module" });
const pending = new Map();
let nextId = 1;
let events = [];
let active = SESSION;

worker.onmessage = (event) => {
  const message = event.data;
  if (message.type === "ready") {
    refresh();
    return;
  }
  const resolve = pending.get(message.id);
  if (!resolve) return;
  pending.delete(message.id);
  resolve(message);
};

function ask(message) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, (reply) => (reply.error ? reject(new Error(reply.error)) : resolve(reply)));
    worker.postMessage({ ...message, id });
  });
}

const uuid = () => crypto.randomUUID();

function base(kind, payload) {
  const seq = events.reduce((max, event) => Math.max(max, Number(event.seq ?? 0)), 0) + 1;
  return {
    session_id: SESSION,
    event_id: uuid(),
    seq,
    device_id: DEVICE,
    kind,
    payload,
    created_at_ms: Date.now(),
    fence: null,
  };
}

async function append(event) {
  const reply = await ask({ type: "append", event });
  events = reply.events;
  draw();
}

async function refresh() {
  try {
    const reply = await ask({ type: "load" });
    events = reply.events;
    draw();
  } catch (error) {
    app.textContent = `error: ${error.message}`;
  }
}

function draw() {
  const now = Date.now();
  const view = sessionView(SESSION, DEVICE, events.filter((e) => e.session_id === SESSION), now);
  status.textContent = `${events.length} events in OPFS · ${view.messages.length} visible · ${view.queue.length} queued`;
  renderThreads(
    app,
    { sessions: [view], active },
    {
      onSelect: (session) => {
        active = session;
        draw();
      },
      onCommand: async (command) => {
        try {
          await apply(command);
        } catch (error) {
          status.textContent = `error: ${error.message}`;
        }
      },
    },
  );
}

async function apply(command) {
  switch (command.type) {
    case "enqueue":
      return append(base("message_queued", { text: command.text, at: Date.now(), reply_to: null }));
    case "send":
      return append(
        base("user_message", { text: command.text, at: Date.now(), reply_to: null, edited_from: null }),
      );
    case "dequeue":
      return append(base("message_dequeued", { target: command.event }));
    case "edit": {
      const original = events.find((event) => event.event_id === command.event);
      const replyTo = original?.payload?.reply_to ?? null;
      return append(
        base("user_message", {
          text: command.text,
          at: Date.now(),
          reply_to: replyTo,
          edited_from: command.event,
        }),
      );
    }
    case "restore":
      return append(base("conversation_restored", { truncation: command.truncation }));
    default:
      return undefined;
  }
}

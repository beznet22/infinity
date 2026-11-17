// One renderer for every surface.
//
// It is a pure function of the session views plus a command callback: it never
// fetches, never writes and never invents state. Android and the browser differ
// only in where the views and commands come from (D54, D59).

const el = (tag, props = {}, children = []) => {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    // `dataset` is read-only, so it must be filled rather than assigned; the rest
    // are normal reflected properties.
    if (key === "dataset") Object.assign(node.dataset, value);
    else if (key in node) node[key] = value;
    else node.setAttribute(key, value);
  }
  for (const child of [].concat(children)) {
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
};

function roleLabel(role) {
  return role === "assistant" ? "assistant" : "you";
}

function renderTranscript(message, onCommand) {
  const row = el("li", { className: `message message--${message.role}`, dataset: { event: message.event } });
  row.append(
    el("span", { className: "role" }, roleLabel(message.role)),
    el("span", { className: "text" }, message.text),
  );
  if (message.versions > 1) {
    // A version badge is the branch switcher's anchor: the host opens the list.
    const badge = el("button", { className: "versions", textContent: `${message.versions} versions` });
    badge.onclick = () => onCommand({ type: "versions", event: message.event });
    row.append(badge);
  }
  if (message.role === "user") {
    const edit = el("button", { className: "edit", textContent: "edit" });
    edit.onclick = () => onCommand({ type: "edit", event: message.event, text: message.text });
    row.append(edit);
  }
  return row;
}

function renderQueueItem(entry, onCommand) {
  const row = el("li", { className: "queued", dataset: { event: entry.event } });
  row.append(el("span", { textContent: entry.text }));
  const remove = el("button", { className: "dequeue", textContent: "remove", title: "remove from queue" });
  remove.onclick = () => onCommand({ type: "dequeue", event: entry.event });
  row.append(remove);
  return row;
}

/**
 * Render the thread list and the active conversation.
 *
 * @param {HTMLElement} root
 * @param {{sessions: Array, active: string}} state  views from `session_view()`
 * @param {{onCommand: (command: object) => void, onSelect?: (session: string) => void}} handlers
 */
export function renderThreads(root, state, handlers) {
  const onCommand = handlers.onCommand;
  const active = state.sessions.find((view) => view.session === state.active) ?? state.sessions[0];
  root.replaceChildren();

  const list = el("ul", { className: "threads", "data-testid": "threads" });
  for (const view of state.sessions) {
    const item = el("li", {
      className: view.session === active?.session ? "thread thread--active" : "thread",
      dataset: { session: view.session },
    });
    item.append(
      el("span", { className: "name", textContent: view.session }),
      el("span", { className: "count", textContent: String(view.messages.length) }),
    );
    item.onclick = () => handlers.onSelect?.(view.session);
    list.append(item);
  }
  root.append(list);

  if (!active) {
    root.append(el("p", { className: "empty", textContent: "no sessions yet" }));
    return;
  }

  const header = el("header", { "data-testid": "header" });
  header.append(el("h2", { textContent: active.session }));
  if (active.read_only) {
    // Read-anywhere: another device owns the writer slot.
    header.append(
      el("p", {
        className: "banner",
        "data-testid": "banner",
        textContent: `${active.holder ?? "another device"} is writing — read-only`,
      }),
    );
  }
  root.append(header);

  if (active.truncations.some((entry) => entry.active)) {
    const bar = el("div", { className: "truncations", "data-testid": "truncations" });
    for (const entry of active.truncations.filter((candidate) => candidate.active)) {
      const undo = el("button", { className: "restore", textContent: "restore deleted messages" });
      undo.onclick = () => onCommand({ type: "restore", truncation: entry.id });
      bar.append(undo);
    }
    root.append(bar);
  }

  const transcript = el("ol", { className: "transcript", "data-testid": "transcript" });
  for (const message of active.messages) transcript.append(renderTranscript(message, onCommand));
  if (active.messages.length === 0) {
    transcript.append(el("li", { className: "empty", textContent: "nothing visible — messages were deleted" }));
  }
  root.append(transcript);

  if (active.queue.length > 0) {
    const tray = el("div", { className: "queue", "data-testid": "queue" });
    tray.append(el("h3", { textContent: `queued (${active.queue.length})` }));
    const items = el("ul");
    for (const entry of active.queue) items.append(renderQueueItem(entry, onCommand));
    tray.append(items);
    root.append(tray);
  }

  const composer = el("form", { className: "composer", "data-testid": "composer" });
  const input = el("textarea", {
    placeholder: active.read_only ? "read-only on this device" : "message the agent",
    rows: 2,
    "data-testid": "composer-input",
  });
  input.disabled = active.read_only;
  const queue = el("button", { type: "button", textContent: "queue", "data-testid": "queue-button" });
  queue.disabled = active.read_only;
  queue.onclick = () => {
    if (input.value.trim() === "") return;
    onCommand({ type: "enqueue", session: active.session, text: input.value });
    input.value = "";
  };
  const send = el("button", {
    type: "submit",
    textContent: "send",
    "data-testid": "send-button",
  });
  send.disabled = active.read_only;
  composer.append(input, queue, send);
  composer.onsubmit = (event) => {
    event.preventDefault();
    if (input.value.trim() === "") return;
    onCommand({ type: "send", session: active.session, text: input.value });
    input.value = "";
  };
  root.append(composer);
}

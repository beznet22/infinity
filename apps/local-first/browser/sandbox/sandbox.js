// Host side: one Worker per tool, terminated on timeout.
export function runTool(bytes, timeoutMs = 500) {
  return new Promise((resolve) => {
    const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
    const id = crypto.randomUUID();
    let settled = false;

    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      worker.terminate();
      resolve(result);
    };

    const timer = setTimeout(
      () => finish({ error: `timeout: terminated after ${timeoutMs}ms` }),
      timeoutMs,
    );
    worker.onmessage = (event) => {
      if (event.data.id === id) finish(event.data);
    };
    worker.onerror = (event) => finish({ error: `worker error: ${event.message}` });
    worker.postMessage({ id, bytes });
  });
}

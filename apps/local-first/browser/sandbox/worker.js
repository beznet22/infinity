// Runs one tool. The only import it may have is `env.memory`, which the host
// supplies with a maximum, so a tool cannot grow memory past 16 pages (1 MiB).
const MEMORY_PAGES_MAX = 16;

self.onmessage = async (event) => {
  const { id, bytes } = event.data;
  try {
    const module = await WebAssembly.compile(bytes);
    const imports = WebAssembly.Module.imports(module);

    const denied = imports.filter(
      (entry) => !(entry.module === "env" && entry.name === "memory" && entry.kind === "memory"),
    );
    if (denied.length > 0) {
      self.postMessage({
        id,
        error: `denied: ${denied.map((entry) => `${entry.module}.${entry.name}`).join(", ")}`,
      });
      return;
    }
    if (!imports.some((entry) => entry.module === "env" && entry.name === "memory")) {
      self.postMessage({
        id,
        error: "denied: the tool must import env.memory so the host can bound it",
      });
      return;
    }

    const memory = new WebAssembly.Memory({ initial: 1, maximum: MEMORY_PAGES_MAX });
    // With a compiled Module, instantiate resolves to the Instance itself.
    const instance = await WebAssembly.instantiate(module, { env: { memory } });
    if (typeof instance.exports.run !== "function") {
      self.postMessage({ id, error: "denied: no run() export" });
      return;
    }
    self.postMessage({ id, value: instance.exports.run() });
  } catch (error) {
    self.postMessage({ id, error: `trap: ${error}` });
  }
};

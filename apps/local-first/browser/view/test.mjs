// Parity: the JS fold must equal the Rust fold's `view` for the same events.
import { readFileSync } from "node:fs";
import { sessionView } from "./view.js";

const fixture = JSON.parse(readFileSync(new URL("./fixtures/view-golden.json", import.meta.url)));

// Rust used `now_ms()` when it produced the fixture; the lease-free scenario means
// the value only affects expiry checks, and there are none. Use a fixed clock so
// the test is deterministic.
const now = 1_000_000_000_000;
const actual = sessionView(fixture.session, fixture.device, fixture.events, now);

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

const expected = canonical(fixture.view);
const got = canonical(actual);
if (JSON.stringify(expected) !== JSON.stringify(got)) {
  console.error("view parity FAILED");
  console.error("expected:", JSON.stringify(expected, null, 2));
  console.error("actual:  ", JSON.stringify(got, null, 2));
  process.exit(1);
}
console.log("view parity OK:", actual.messages.length, "messages,", actual.queue.length, "queued");

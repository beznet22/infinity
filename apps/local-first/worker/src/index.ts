/**
 * Cloudflare Worker relay for the local-first app.
 *
 * Three calls, deliberately dumb: push events (idempotent), pull events after a
 * position, list sessions. D1 is the durable log; the same schema runs locally
 * under `wrangler dev`.
 */

interface Env {
  DB: D1Database;
  /** Required for /v1/* when set; configured as a secret, never committed. */
  RELAY_TOKEN?: string;
}

interface SyncEvent {
  session_id: string;
  event_id: string;
  seq: number;
  device_id: string;
  kind: string;
  payload: unknown;
  created_at_ms: number;
  /** Fencing token of the writer's lease (0 when it never held one). */
  fence?: number;
}

// The browser and the Worker are different origins, so every response needs CORS.
const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "GET,POST,OPTIONS",
  "access-control-allow-headers": "content-type",
};

const json = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { "content-type": "application/json", ...CORS },
  });

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS });
    }

    if (url.pathname === "/health") {
      return json({ ok: true });
    }

    // Without a token the relay is an open write endpoint, which is why the last
    // deployment was deleted (D34). When RELAY_TOKEN is set, every sync call must
    // carry it; when it is absent the relay refuses to serve sync at all.
    if (url.pathname.startsWith("/v1/")) {
      const expected = env.RELAY_TOKEN;
      if (!expected) {
        return json({ error: "relay token is not configured" }, 503);
      }
      const provided = request.headers.get("authorization") ?? "";
      if (provided !== `Bearer ${expected}`) {
        return json({ error: "unauthorized" }, 401);
      }
    }

    if (url.pathname === "/v1/sync/push" && request.method === "POST") {
      const body = (await request.json()) as { events?: SyncEvent[] };
      const events = body.events ?? [];
      if (!Array.isArray(events)) {
        return json({ error: "events must be an array" }, 400);
      }

      let accepted = 0;
      let stale = 0;
      let lastSeq = 0;
      for (const event of events) {
        const fence = Number(event.fence ?? 0);

        // One statement does the check and the write. D1 serialises writes, and
        // the trigger advances `sessions.current_fence` inside the same
        // transaction, so a lower fence cannot slip in between a read and a write.
        const result = await env.DB.prepare(
          `INSERT INTO events
             (session_id, event_id, seq, device_id, kind, payload, created_at_ms, fence)
           SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
            WHERE ?8 >= COALESCE(
                    (SELECT current_fence FROM sessions WHERE session_id = ?1), 0)
           ON CONFLICT (session_id, event_id) DO NOTHING`,
        )
          .bind(
            event.session_id,
            event.event_id,
            event.seq,
            event.device_id,
            event.kind,
            JSON.stringify(event.payload ?? null),
            event.created_at_ms,
            fence,
          )
          .run();
        // The trigger's session upsert also counts in `meta.changes`, so the
        // verdict comes from the row itself: present means accepted (a retry is
        // harmless), absent means the fence check refused it.
        const stored = await env.DB.prepare(
          `SELECT 1 FROM events WHERE session_id = ?1 AND event_id = ?2`,
        )
          .bind(event.session_id, event.event_id)
          .first();
        if (stored) {
          accepted += 1;
          lastSeq = Math.max(lastSeq, event.seq);
        } else {
          stale += 1;
        }
      }

      return json({ accepted, stale, last_seq: lastSeq });
    }

    if (url.pathname === "/v1/sync/pull" && request.method === "GET") {
      const session = url.searchParams.get("session");
      const after = Number(url.searchParams.get("after") ?? "0");
      if (!session) {
        return json({ error: "session is required" }, 400);
      }

      const { results } = await env.DB.prepare(
        `SELECT position, session_id, event_id, seq, device_id, kind, payload, created_at_ms, fence
           FROM events
          WHERE session_id = ? AND position > ?
          ORDER BY position ASC`,
      )
        .bind(session, after)
        .all<Record<string, unknown>>();

      const events = (results ?? []).map((row) => ({
        ...row,
        payload: JSON.parse(String(row.payload)),
      }));
      return json(events);
    }

    if (url.pathname === "/v1/sync/sessions" && request.method === "GET") {
      const { results } = await env.DB.prepare(
        `SELECT DISTINCT session_id FROM events ORDER BY session_id`,
      ).all<{ session_id: string }>();
      return json((results ?? []).map((row) => row.session_id));
    }

    return json({ error: "not found" }, 404);
  },
};

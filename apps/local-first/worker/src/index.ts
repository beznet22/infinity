/**
 * Cloudflare Worker relay for the local-first app.
 *
 * Three calls, deliberately dumb: push events (idempotent), pull events after a
 * position, list sessions. D1 is the durable log; the same schema runs locally
 * under `wrangler dev`.
 */

interface Env {
  DB: D1Database;
}

interface SyncEvent {
  session_id: string;
  event_id: string;
  seq: number;
  device_id: string;
  kind: string;
  payload: unknown;
  created_at_ms: number;
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

    if (url.pathname === "/v1/sync/push" && request.method === "POST") {
      const body = (await request.json()) as { events?: SyncEvent[] };
      const events = body.events ?? [];
      if (!Array.isArray(events)) {
        return json({ error: "events must be an array" }, 400);
      }

      let accepted = 0;
      let lastSeq = 0;
      for (const event of events) {
        // Idempotent by (session_id, event_id): a retried push is a no-op.
        const result = await env.DB.prepare(
          `INSERT INTO events
             (session_id, event_id, seq, device_id, kind, payload, created_at_ms)
           VALUES (?, ?, ?, ?, ?, ?, ?)
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
          )
          .run();
        accepted += result.meta.changes ?? 0;
        lastSeq = Math.max(lastSeq, event.seq);
      }

      return json({ accepted, last_seq: lastSeq });
    }

    if (url.pathname === "/v1/sync/pull" && request.method === "GET") {
      const session = url.searchParams.get("session");
      const after = Number(url.searchParams.get("after") ?? "0");
      if (!session) {
        return json({ error: "session is required" }, 400);
      }

      const { results } = await env.DB.prepare(
        `SELECT position, session_id, event_id, seq, device_id, kind, payload, created_at_ms
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

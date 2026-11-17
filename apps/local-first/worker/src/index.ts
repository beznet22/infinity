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

        // Fencing: once a session has a newer holder, events stamped with an
        // older token are dropped. Expiry alone leaves a window where a stalled
        // writer and the new holder both believe they may write.
        const row = await env.DB.prepare(
          `SELECT current_fence FROM sessions WHERE session_id = ?`,
        )
          .bind(event.session_id)
          .first<{ current_fence: number }>();
        const currentFence = Number(row?.current_fence ?? 0);
        if (fence < currentFence) {
          stale += 1;
          continue;
        }

        // Idempotent by (session_id, event_id): a retried push is a no-op.
        const result = await env.DB.prepare(
          `INSERT INTO events
             (session_id, event_id, seq, device_id, kind, payload, created_at_ms, fence)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)
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
        accepted += result.meta.changes ?? 0;
        lastSeq = Math.max(lastSeq, event.seq);

        if (fence > currentFence) {
          await env.DB.prepare(
            `INSERT INTO sessions (session_id, current_fence) VALUES (?, ?)
             ON CONFLICT (session_id) DO UPDATE SET current_fence = excluded.current_fence`,
          )
            .bind(event.session_id, fence)
            .run();
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

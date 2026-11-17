#!/usr/bin/env python3
"""Minimal OpenAI-compatible CORS proxy for local browser tests.

Providers that do not send CORS headers (e.g. the Kimchi endpoint) cannot be
called from a page directly. This proxy forwards `/v1/chat/completions` to the
configured upstream, injects the key server-side (so it never reaches the
browser), and adds CORS headers.

    KIMCHI_API_KEY=... python3 dev-cors-proxy.py

This is a *test* stand-in for the relay (see the Worker archetype), not a
production component.
"""
import json
import os
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

UPSTREAM = "https://llm.kimchi.dev/openai/v1/chat/completions"
KEY = os.environ.get("KIMCHI_API_KEY", "")
PORT = 8125

CORS = {
    "access-control-allow-origin": "*",
    "access-control-allow-methods": "POST,OPTIONS",
    "access-control-allow-headers": "content-type,authorization",
}


class Handler(BaseHTTPRequestHandler):
    def _cors(self):
        for k, v in CORS.items():
            self.send_header(k, v)

    def do_OPTIONS(self):
        self.send_response(204)
        self._cors()
        self.end_headers()

    def do_POST(self):
        length = int(self.headers.get("content-length") or 0)
        body = self.rfile.read(length) if length else b"{}"
        request = urllib.request.Request(
            UPSTREAM,
            data=body,
            headers={
                "content-type": "application/json",
                "authorization": f"Bearer {KEY}",
                # Cloudflare fronts some providers and rejects Python's default
                # agent with error 1010.
                "user-agent": "infinity-local-proxy/0.1",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                payload = response.read()
                status = response.status
        except urllib.error.HTTPError as error:
            payload = error.read()
            status = error.code
        except Exception as error:  # network failure
            payload = json.dumps({"error": {"message": str(error)}}).encode()
            status = 502

        self.send_response(status)
        self.send_header("content-type", "application/json")
        self._cors()
        self.send_header("content-length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args):
        pass


with ThreadingHTTPServer(("127.0.0.1", PORT), Handler) as server:
    server.serve_forever()

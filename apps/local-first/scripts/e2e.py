#!/usr/bin/env python3
"""Cross-platform end-to-end check: desktop CLI, browser OPFS, Android (optional).

Desktop and browser run everywhere; the Android leg needs a device:

    ADB_DEVICE=192.168.3.249:35969 python3 scripts/e2e.py

The three legs assert the same properties where they apply: a message queued
while busy leaves the transcript alone, sending promotes it, and a deletion from
a point is reversible.
"""

import asyncio
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / "target" / "debug" / "local-sync"
CHROME = shutil.which("google-chrome") or shutil.which("chromium")


def run(*args, **kwargs):
    return subprocess.run(args, capture_output=True, text=True, **kwargs)


def check(condition, message):
    if not condition:
        raise AssertionError(message)


def desktop():
    root = Path(tempfile.mkdtemp(prefix="e2e-desktop-"))
    base = [str(CLI), "--root", str(root), "--device", "laptop"]

    def call(*args):
        result = run(*base, *args)
        check(result.returncode == 0, f"{args}: {result.stderr.strip()}")
        return json.loads(result.stdout.strip().splitlines()[-1])

    call("write", "--session", "e2e", "--text", "first")
    queued = call("enqueue", "--session", "e2e", "--text", "while busy")
    view = call("view", "--session", "e2e")
    check(len(view["messages"]) == 1, "queued message must stay out of the transcript")
    check(len(view["queue"]) == 1, "queued message must appear in the tray")

    call("send", "--session", "e2e", "--event", queued["queued"])
    view = call("view", "--session", "e2e")
    check(len(view["messages"]) == 2, "sent message must join the transcript")
    check(view["queue"] == [], "queue must empty after send")

    first = view["messages"][0]["event"]
    truncation = call("truncate", "--session", "e2e", "--event", first)
    view = call("view", "--session", "e2e")
    check(view["messages"] == [], "delete-from-here must hide the tail")

    call("restore", "--session", "e2e", "--truncation", truncation["truncation"])
    view = call("view", "--session", "e2e")
    check(len(view["messages"]) == 2, "restore must bring the tail back")
    return {"messages": len(view["messages"]), "queue": len(view["queue"])}


async def browser_leg(url, port):
    import websockets

    expression = r"""(async () => {
      const deadline = Date.now() + 12000;
      const q = (s) => document.querySelector(s);
      while (!q('[data-testid="composer-input"]') && Date.now() < deadline)
        await new Promise((r) => setTimeout(r, 100));
      if (!q('[data-testid="composer-input"]')) return JSON.stringify({ error: document.body.innerText.slice(0, 200) });
      q('[data-testid="composer-input"]').value = 'queued by e2e';
      q('[data-testid="queue-button"]').click();
      await new Promise((r) => setTimeout(r, 1000));
      return JSON.stringify({ queue: document.querySelectorAll('.queued').length, status: q('#status').textContent });
    })()"""

    observe = r"""(async () => {
      const deadline = Date.now() + 12000;
      const q = (s) => document.querySelector(s);
      while (!q('[data-testid="composer-input"]') && Date.now() < deadline)
        await new Promise((r) => setTimeout(r, 100));
      return JSON.stringify({ queue: document.querySelectorAll('.queued').length, status: q('#status')?.textContent });
    })()"""

    request = urllib.request.Request(f"http://127.0.0.1:{port}/json/new?{url}", method="PUT")
    with urllib.request.urlopen(request) as response:
        target = json.load(response)

    async with websockets.connect(target["webSocketDebuggerUrl"], max_size=2**22) as ws:
        await ws.send(json.dumps({"id": 1, "method": "Runtime.enable"}))
        await ws.recv()

        async def evaluate(ident, source=expression):
            await ws.send(json.dumps({"id": ident, "method": "Runtime.evaluate",
                "params": {"expression": source, "awaitPromise": True, "returnByValue": True}}))
            while True:
                message = json.loads(await ws.recv())
                if message.get("id") == ident:
                    return json.loads(message["result"]["result"]["value"])

        first = await evaluate(2)
        check("error" not in first, f"browser did not render: {first}")
        check(first["queue"] >= 1, "browser queue must show the message")

        # Reload: the entry must come back from OPFS.
        await ws.send(json.dumps({"id": 3, "method": "Page.reload", "params": {"ignoreCache": True}}))
        await asyncio.sleep(2)
        second = await evaluate(4, observe)
        check(second["queue"] == first["queue"], f"reload lost the queue: {second}")
        return {"queue": second["queue"], "status": second["status"]}


def browser():
    def reachable():
        try:
            urllib.request.urlopen("http://127.0.0.1:8123/app/", timeout=1)
            return True
        except OSError:
            return False

    # Reuse a server that is already up (a developer's shell), else start one.
    server = None
    if not reachable():
        server = subprocess.Popen([sys.executable, "serve.py"], cwd=ROOT / "browser")
        deadline = time.time() + 15
        while time.time() < deadline and not reachable():
            time.sleep(0.2)
        if not reachable():
            server.terminate()
            raise AssertionError("browser server did not start on 8123")
    chrome_port = 9405
    chrome = subprocess.Popen(
        [CHROME, "--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
         f"--remote-debugging-port={chrome_port}", f"--user-data-dir={tempfile.mkdtemp(prefix='e2e-chrome-')}",
         "about:blank"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    try:
        deadline = time.time() + 20
        while time.time() < deadline:
            try:
                urllib.request.urlopen("http://127.0.0.1:9405/json/version", timeout=1)
                break
            except OSError:
                time.sleep(0.3)
        # Wait for the static server, not just Chrome.
        server_deadline = time.time() + 15
        while time.time() < server_deadline:
            try:
                urllib.request.urlopen("http://127.0.0.1:8123/app/", timeout=1)
                break
            except OSError:
                time.sleep(0.2)
        return asyncio.run(browser_leg("http://127.0.0.1:8123/app/", chrome_port))
    finally:
        chrome.terminate()
        if server is not None:
            server.terminate()


def android():
    device = os.environ.get("ADB_DEVICE")
    if not device:
        return {"skipped": "set ADB_DEVICE to run the device leg"}
    result = run("adb", "-s", device, "shell", "run-as", "run.hydro.infinityhybrid", "cat", "probe.json")
    check(result.returncode == 0 and "sandbox" in result.stdout, f"device probe missing: {result.stderr}")
    return {"probe": json.loads(result.stdout)}


def main():
    results = {}
    for name, leg in (("desktop", desktop), ("browser", browser), ("android", android)):
        results[name] = leg()
        print(f"{name}: {json.dumps(results[name])}", flush=True)
    print("E2E PASS")


if __name__ == "__main__":
    main()

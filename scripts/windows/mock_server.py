#!/usr/bin/env python3
"""Loopback-only OpenAI/SSE fault fixtures. Never logs prompts or authorization."""
from __future__ import annotations

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import threading
import time

TEXT = "公开测试译文：智能体。😀"
MAX_BODY = 128 * 1024


class Server(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, address=("127.0.0.1", 0)):
        super().__init__(address, Handler)
        self.guard = threading.Lock()
        self.slots = threading.BoundedSemaphore(8)
        self.requests = 0
        self.active = 0
        self.peak_active = 0


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def send_json(self, status, data):
        body = json.dumps(data, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            self.send_json(200, {"ready": True})
        elif self.path == "/stats":
            with self.server.guard:
                count = self.server.requests
                active, peak = self.server.active, self.server.peak_active
            self.send_json(200, {"requests": count, "active": active, "peak_active": peak})
        else:
            self.send_json(404, {"error": "unknown fixture"})

    def do_POST(self):
        try:
            length = int(self.headers.get("Content-Length", "-1"))
        except ValueError:
            length = -1
        if length < 0 or length > MAX_BODY:
            self.close_connection = True
            self.send_json(413, {"error": "bounded fixture request required"})
            return
        self.connection.settimeout(5)
        try:
            body = self.rfile.read(length)
            if len(body) != length:
                raise ValueError("short request")
            json.loads(body)
        except (ValueError, OSError):
            self.close_connection = True
            self.send_json(400, {"error": "invalid JSON"})
            return
        if not self.server.slots.acquire(blocking=False):
            self.send_json(503, {"error": "fixture busy"})
            return
        try:
            with self.server.guard:
                self.server.requests += 1
                self.server.active += 1
                self.server.peak_active = max(self.server.peak_active, self.server.active)
            self.serve_fixture()
        except (BrokenPipeError, ConnectionResetError, TimeoutError):
            pass
        finally:
            with self.server.guard:
                self.server.active -= 1
            self.server.slots.release()

    def serve_fixture(self):
        name = self.path.strip("/")
        if name.startswith("http/"):
            try:
                code = int(name.split("/")[1])
            except (ValueError, IndexError):
                code = 404
            if code not in (401, 403, 429, 500):
                code = 404
            # Public sentinel verifies that real clients redact error echoes.
            self.send_json(code, {"error": "BEEK_PUBLIC_ERROR_SENTINEL" + "x" * (70 * 1024)})
            return
        if name == "nonstream":
            self.send_json(200, {"choices": [{"message": {"role": "assistant", "content": TEXT}}]})
            return
        if name not in {"sse/normal", "sse/fragmented", "sse/disconnect", "sse/slow", "sse/empty", "sse/oversized-frame", "sse/oversized-output"}:
            self.send_json(404, {"error": "unknown fixture"})
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True

        def chunk(text):
            return ("data: " + json.dumps({"choices": [{"delta": {"content": text}}]}, ensure_ascii=False) + "\n\n").encode("utf-8")

        if name == "sse/oversized-frame":
            self.wfile.write(chunk("x" * (70 * 1024)))
        elif name == "sse/oversized-output":
            for _ in range(70):
                self.wfile.write(chunk("x" * 4096))
        elif name == "sse/fragmented":
            # Single-byte writes split JSON tokens and the multi-byte Chinese/emoji.
            for byte in chunk(TEXT):
                self.wfile.write(bytes([byte]))
                self.wfile.flush()
                time.sleep(0.001)
        elif name == "sse/disconnect":
            self.wfile.write(chunk("partial"))
            self.wfile.flush()
            return  # No [DONE], intentionally incomplete.
        elif name == "sse/slow":
            for _ in range(65):
                self.wfile.write(chunk("."))
                self.wfile.flush()
                time.sleep(1)
        elif name != "sse/empty":
            for text in ("公开测试译文：", "智能体。", "😀"):
                self.wfile.write(chunk(text))
                self.wfile.flush()
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--ready-file", type=Path)
    args = parser.parse_args()
    server = Server(("127.0.0.1", args.port))
    metadata = {"base_url": f"http://127.0.0.1:{server.server_port}", "test_key": "public-fixture-key"}
    if args.ready_file:
        args.ready_file.parent.mkdir(parents=True, exist_ok=True)
        args.ready_file.write_text(json.dumps(metadata) + "\n", encoding="utf-8")
    print(json.dumps(metadata), flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

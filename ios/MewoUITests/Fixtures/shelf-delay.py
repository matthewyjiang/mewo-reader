#!/usr/bin/env python3
"""Hold GET /v1/library until /control/release. Cover and version answer immediately.

LibraryRefreshUITests talks to 127.0.0.1:8765. Start this on the Mac before that
test; the test skips when nothing is listening.
"""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOST = "127.0.0.1"
PORT = 8765

lock = threading.Lock()
released = threading.Event()
library_requests = 0
waiting = 0

BOOK = {
    "id": "44444444-4444-4444-4444-444444444444",
    "title": "A Timeline of My Own",
    "author": "Mewo",
    "handle": "mewo",
    "importedAt": 1,
    "postCount": 12,
    "progressIndex": 0,
    "isSample": False,
    "mine": True,
    "shared": False,
    "ownerUsername": "ada",
}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/v1/library":
            self.library()
        elif path == "/v1/version":
            self.json_ok({"version": "0.0.0", "commit": "delay"})
        elif path.startswith("/v1/books/") and path.endswith("/cover"):
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
        elif path == "/control/status":
            with lock:
                body = {
                    "released": released.is_set(),
                    "library_requests": library_requests,
                    "waiting": waiting,
                }
            self.json_ok(body)
        else:
            self.send_error(404)

    def do_POST(self):
        path = self.path.split("?", 1)[0]
        if path == "/control/hold":
            released.clear()
            self.json_ok({"released": False})
        elif path == "/control/release":
            released.set()
            self.json_ok({"released": True})
        else:
            self.send_error(404)

    def library(self):
        global library_requests, waiting
        with lock:
            library_requests += 1
            waiting += 1
            n = library_requests
        print(f"library wait #{n}", flush=True)
        released.wait()
        with lock:
            waiting -= 1
        print(f"library send #{n}", flush=True)
        self.json_ok({"books": [BOOK]})

    def json_ok(self, payload):
        raw = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def log_message(self, fmt, *args):
        print("http", self.address_string(), fmt % args, flush=True)


if __name__ == "__main__":
    print(f"listening on {HOST}:{PORT}", flush=True)
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()

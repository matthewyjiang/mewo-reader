#!/usr/bin/env python3
"""Hosted social fixture for screenshot walks. GET /v1/feed waits until /control/release.

SocialScreenshotTests talks to 127.0.0.1:8766. Start this on the Mac before that
test; the test skips when nothing is listening.
"""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

HOST = "127.0.0.1"
PORT = 8766

lock = threading.Lock()
released = threading.Event()
feed_requests = 0

BOOK = {
    "id": "44444444-4444-4444-4444-444444444444",
    "title": "A Timeline of My Own",
    "author": "Mewo",
    "handle": "mewo",
    "importedAt": 1,
    "postCount": 2,
    "progressIndex": 0,
    "isSample": False,
    "mine": True,
    "shared": True,
    "ownerUsername": "ada",
}

POST = {"id": "1", "kind": "Body", "text": "The first sentence is the one you remember.", "chapter": "One"}
HEADING = {"id": "0", "kind": "Heading", "text": "Chapter One", "chapter": "One"}
COMMENT = {"id": "k", "username": "ada", "text": "Still true.", "createdAt": 1710000000000, "mine": True}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        parsed = urlparse(self.path)
        path = parsed.path
        query = parse_qs(parsed.query)
        if path == "/v1/library":
            self.json_ok({"books": [BOOK]})
        elif path == "/v1/version":
            self.json_ok({"version": "0.0.0", "commit": "social"})
        elif path == "/v1/feed":
            self.feed()
        elif path == "/v1/search":
            self.search(query.get("type", ["accounts"])[0], query.get("q", [""])[0])
        elif path == "/v1/users/ada":
            self.json_ok({
                "username": "ada",
                "joinedAt": 1710000000000,
                "postCount": 2,
                "repostCount": 1,
                "mine": True,
            })
        elif path == "/v1/users/bea":
            self.json_ok({
                "username": "bea",
                "joinedAt": 1710000000000,
                "postCount": 1,
                "repostCount": 1,
                "mine": False,
            })
        elif path == "/v1/users/ada/posts":
            self.json_ok({"items": [{"book": BOOK, "post": HEADING, "comment": COMMENT}], "nextCursor": None})
        elif path == "/v1/users/ada/reposts":
            self.json_ok({"items": [{"book": BOOK, "post": POST, "repostedAt": 1710000000000}], "nextCursor": None})
        elif path == "/v1/users/ada/books":
            self.json_ok({"items": [BOOK]})
        elif path.endswith("/posts") and path.startswith("/v1/books/"):
            self.json_ok({"posts": [POST, HEADING]})
        elif path.endswith("/likes") and path.startswith("/v1/books/"):
            self.json_ok({"ids": []})
        elif path.endswith("/comments") and "/comments/" not in path and path.startswith("/v1/books/"):
            self.json_ok({"counts": {"1": 1}, "mine": ["1"], "reposts": {"1": 3}, "reposted": ["1"]})
        elif "/comments/" in path:
            self.json_ok({"comments": [COMMENT]})
        elif path.startswith("/v1/books/") and path.endswith("/cover"):
            self.send_response(204)
            self.send_header("Content-Length", "0")
            self.end_headers()
        elif path.startswith("/v1/books/"):
            self.json_ok(BOOK)
        elif path == "/control/status":
            with lock:
                body = {"released": released.is_set(), "feed_requests": feed_requests}
            self.json_ok(body)
        else:
            self.send_error(404)

    def do_POST(self):
        path = urlparse(self.path).path
        if path == "/control/hold":
            released.clear()
            self.json_ok({"released": False})
        elif path == "/control/release":
            released.set()
            self.json_ok({"released": True})
        else:
            self.send_error(404)

    def do_PUT(self):
        if self.path.endswith("/repost"):
            self.json_ok({"reposts": 4, "reposted": True})
        else:
            self.send_error(404)

    def do_DELETE(self):
        if self.path.endswith("/repost"):
            self.json_ok({"reposts": 2, "reposted": False})
        else:
            self.send_error(404)

    def do_PATCH(self):
        if self.path.startswith("/v1/books/"):
            self.json_ok(BOOK)
        else:
            self.send_error(404)

    def feed(self):
        global feed_requests
        with lock:
            feed_requests += 1
            n = feed_requests
        print(f"feed wait #{n}", flush=True)
        released.wait()
        print(f"feed send #{n}", flush=True)
        self.json_ok({
            "items": [
                {
                    "type": "repost",
                    "actor": {"username": "bea"},
                    "book": BOOK,
                    "post": POST,
                    "at": 2,
                },
                {
                    "type": "comment",
                    "actor": {"username": "ada"},
                    "book": BOOK,
                    "post": HEADING,
                    "comment": COMMENT,
                    "at": 1,
                },
            ],
            "nextCursor": None,
        })

    def search(self, kind, query):
        needle = query.lower()
        if kind == "books":
            haystack = " ".join([
                BOOK["title"], BOOK["author"], BOOK["handle"], BOOK["ownerUsername"],
            ]).lower()
            items = [BOOK] if needle in haystack else []
        elif kind == "posts":
            haystack = f"{POST['text']} {BOOK['ownerUsername']}".lower()
            items = [{"book": BOOK, "post": POST}] if needle in haystack else []
        else:
            accounts = [{"username": "ada", "mine": True}, {"username": "bea", "mine": False}]
            items = [account for account in accounts if needle in account["username"].lower()]
        self.json_ok({"type": kind, "items": items, "nextCursor": None})

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

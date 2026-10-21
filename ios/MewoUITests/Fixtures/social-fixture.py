#!/usr/bin/env python3
"""Hosted social fixture for screenshot walks. GET /v1/feed waits until /control/release.

SocialScreenshotTests and CatalogScreenshotTests talk to 127.0.0.1:8766. Start
this on the Mac before those tests; each test fails when nothing is listening.
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
fail_search = False
next_request = 0

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

def request_row(olid, title, author, year, state, progress=0, error=None, book_id=None):
    return {
        "id": f"req-{olid}",
        "workKey": f"/works/{olid}",
        "title": title,
        "author": author,
        "year": year,
        "coverId": None,
        "state": state,
        "progress": progress,
        "error": error,
        "bookId": book_id,
        "requester": "ada",
        "createdAt": 10,
        "updatedAt": 11,
        "follows": None,
    }


def work_row(olid, title, author, year, request=None, on_server=None):
    return {
        "workKey": f"/works/{olid}",
        "title": title,
        "author": author,
        "year": year,
        "coverId": None,
        "request": request,
        "onServer": on_server,
    }


# OL1 stays req-1 so older walks that cancel or retry that id still match.
DUNE_REQUEST = request_row("OL1W", "Dune", "Frank Herbert", 1965, "requested")
DUNE_REQUEST["id"] = "req-1"

SHARED_BY_BEA = {
    "bookId": "77777777-7777-7777-7777-777777777777",
    "ownerUsername": "bea",
    "mine": False,
    "shared": True,
    "held": False,
}

WORKS = [
    work_row("OL1W", "Dune", "Frank Herbert", 1965, request=DUNE_REQUEST),
    work_row("OL2W", "The Left Hand of Darkness", "Ursula K. Le Guin", 1969),
    work_row(
        "OL3W", "Foundation", "Isaac Asimov", 1951,
        request=request_row("OL3W", "Foundation", "Isaac Asimov", 1951, "downloading", progress=42),
    ),
    work_row(
        "OL4W", "Emma", "Jane Austen", 1815,
        request=request_row("OL4W", "Emma", "Jane Austen", 1815, "imported", book_id=BOOK["id"]),
    ),
    work_row(
        "OL5W", "Pale Fire", "Vladimir Nabokov", 1962,
        request=request_row("OL5W", "Pale Fire", "Vladimir Nabokov", 1962, "imported", book_id=None),
    ),
    work_row(
        "OL6W", "The Dispossessed", "Ursula K. Le Guin", 1974,
        request=request_row(
            "OL6W", "The Dispossessed", "Ursula K. Le Guin", 1974, "failed", error="No release found.",
        ),
    ),
    work_row("OL7W", "Kindred", "Octavia E. Butler", 1979, on_server=SHARED_BY_BEA),
    work_row(
        "OL8W", "Parable of the Sower", "Octavia E. Butler", 1993,
        request=request_row("OL8W", "Parable of the Sower", "Octavia E. Butler", 1993, "canceled"),
    ),
]
WORK_BY_OLID = {item["workKey"].rsplit("/", 1)[-1]: item for item in WORKS}


def phase_of(request):
    state = request["state"]
    if state == "downloading":
        return "importing" if request["progress"] >= 100 else "downloading"
    if state == "requested":
        return "searching"
    return state


def work_detail(item):
    request = item["request"]
    detail_request = None
    if request is not None:
        detail_request = {
            **request,
            "attempts": 1,
            "maxAttempts": 4,
            "nextCheckAt": None,
            "releaseTitle": None,
            "releaseSize": None,
            "phase": phase_of(request),
        }
    live = request is not None and request["state"] in ("requested", "downloading")
    return {
        "workKey": item["workKey"],
        "info": {
            "canonicalKey": item["workKey"],
            "title": item["title"],
            "subtitle": None,
            "authors": [item["author"]],
            "year": item["year"],
            "coverId": None,
            "description": "A desert planet. A boy. A spice.",
            "subjects": ["Science fiction", "Arrakis", "Deserts", "Sand", "Politics"],
            "awards": ["Hugo Award"],
            "series": ["Dune"],
            "editionCount": 12,
            "pagesMedian": 412,
            "languages": ["eng"],
            "hasAcceptedEdition": True,
            "rating": {"average": 4.3, "count": 10},
        },
        "infoError": None,
        "status": {
            "request": detail_request,
            "onServer": item["onServer"],
            "active": 1 if live else 0,
            "cap": 5,
            "otherRequests": [],
        },
    }


def find_request(req_id):
    for item in WORKS:
        request = item["request"]
        if request is not None and request["id"] == req_id:
            return request
    return None


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
        elif path == "/v1/me":
            self.json_ok({"username": "ada", "isAdmin": False, "canRequest": True})
        elif path == "/v1/catalog/search":
            self.catalog_search(query.get("q", [""])[0])
        elif path.startswith("/v1/catalog/works/"):
            olid = path.rsplit("/", 1)[-1]
            item = WORK_BY_OLID.get(olid)
            if item is None:
                self.send_error(404)
            else:
                self.json_ok(work_detail(item))
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
                body = {
                    "released": released.is_set(),
                    "feed_requests": feed_requests,
                    "fail_search": fail_search,
                }
            self.json_ok(body)
        else:
            self.send_error(404)

    def do_POST(self):
        path = urlparse(self.path).path
        body = self.read_json()
        if path == "/control/hold":
            released.clear()
            self.json_ok({"released": False})
        elif path == "/control/release":
            released.set()
            self.json_ok({"released": True})
        elif path == "/control/fail-search":
            self.toggle_fail_search()
        elif path == "/v1/requests":
            self.new_request(body)
        elif path.startswith("/v1/requests/") and path.endswith("/retry"):
            req_id = path[len("/v1/requests/"):-len("/retry")]
            self.retry_request(req_id)
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

    def catalog_search(self, query):
        with lock:
            failing = fail_search
        if failing:
            self.json_status(500, {"error": "Couldn't search the catalog right now."})
            return
        needle = query.strip().lower()
        items = list(WORKS)
        if needle:
            items = [
                item for item in items
                if needle in item["title"].lower() or needle in item["author"].lower()
            ]
        self.json_ok({"items": items, "nextPage": None})

    def toggle_fail_search(self):
        global fail_search
        with lock:
            fail_search = not fail_search
            state = fail_search
        self.json_ok({"failSearch": state})

    def new_request(self, body):
        global next_request
        with lock:
            next_request += 1
            n = next_request
        self.json_status(201, {
            "id": f"req-new-{n}",
            "workKey": body.get("workKey") or "/works/OL0W",
            "title": body.get("title") or "",
            "author": body.get("author") or "",
            "year": body.get("year"),
            "coverId": body.get("coverId"),
            "state": "requested",
            "progress": 0,
            "error": None,
            "bookId": None,
            "requester": "ada",
            "createdAt": 12,
            "updatedAt": 12,
            "follows": None,
        })

    def retry_request(self, req_id):
        current = find_request(req_id)
        if current is None:
            self.json_status(404, {"error": "That book is no longer in the catalog."})
            return
        self.json_ok({
            **current,
            "state": "requested",
            "progress": 0,
            "error": None,
            "bookId": None,
        })

    def read_json(self):
        length = int(self.headers.get("Content-Length", "0") or 0)
        raw = self.rfile.read(length) if length else b""
        if not raw:
            return {}
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return {}

    def json_ok(self, payload):
        self.json_status(200, payload)

    def json_status(self, status, payload):
        raw = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def log_message(self, fmt, *args):
        print("http", self.address_string(), fmt % args, flush=True)


if __name__ == "__main__":
    print(f"listening on {HOST}:{PORT}", flush=True)
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()

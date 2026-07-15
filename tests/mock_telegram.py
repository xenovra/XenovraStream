#!/usr/bin/env python3
"""Minimal stand-in for the Telegram Bot API, enough to exercise XenovraStream.

Implements exactly the four calls the app makes:
  POST /bot<token>/sendDocument   (multipart)  -> stores bytes, returns file_id
  GET  /bot<token>/getFile?file_id=..          -> returns file_path
  GET  /file/bot<token>/<file_path>            -> returns the bytes
  POST /bot<token>/deleteMessage               -> drops them
"""
import cgi
import json
import os
import re
import sys
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

STORE = {}          # file_id -> bytes
MSG_TO_FILE = {}    # message_id -> file_id
LOCK = threading.Lock()
COUNTER = [1000]


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def _json(self, payload, code=200):
        body = json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        path = urlparse(self.path).path

        if path.endswith("/sendDocument"):
            ctype, pdict = cgi.parse_header(self.headers.get("Content-Type", ""))
            pdict["boundary"] = pdict["boundary"].encode()
            pdict["CONTENT-LENGTH"] = int(self.headers.get("Content-Length", 0))
            fields = cgi.parse_multipart(self.rfile, pdict)

            doc = fields.get("document", [b""])[0]
            file_id = uuid.uuid4().hex
            with LOCK:
                COUNTER[0] += 1
                message_id = COUNTER[0]
                STORE[file_id] = doc
                MSG_TO_FILE[message_id] = file_id

            return self._json({
                "ok": True,
                "result": {"message_id": message_id, "document": {"file_id": file_id}},
            })

        if path.endswith("/deleteMessage"):
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length).decode()
            params = parse_qs(body)
            message_id = int(params.get("message_id", ["0"])[0])
            with LOCK:
                file_id = MSG_TO_FILE.pop(message_id, None)
                if file_id:
                    STORE.pop(file_id, None)
            return self._json({"ok": True, "result": True})

        self._json({"ok": False}, 404)

    def do_GET(self):
        parsed = urlparse(self.path)
        path = parsed.path

        if path.endswith("/getFile"):
            file_id = parse_qs(parsed.query).get("file_id", [""])[0]
            with LOCK:
                exists = file_id in STORE
            if not exists:
                return self._json({"ok": False, "description": "file not found"}, 404)
            # Real Telegram returns an opaque path; using the id keeps it simple.
            return self._json({"ok": True, "result": {"file_path": file_id}})

        m = re.match(r"^/file/bot[^/]+/(.+)$", path)
        if m:
            file_id = m.group(1)
            with LOCK:
                data = STORE.get(file_id)
            if data is None:
                self.send_response(404)
                self.end_headers()
                return
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return

        if path == "/__stats":
            with LOCK:
                return self._json({
                    "files": len(STORE),
                    "bytes": sum(len(v) for v in STORE.values()),
                })

        self._json({"ok": False}, 404)


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8081
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()

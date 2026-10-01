#!/usr/bin/env python3
"""Fetch gateway that carries a real browser's TLS fingerprint.

Sources behind Cloudflare reject every ordinary HTTP client from a datacenter IP
with a 403 JS-challenge, whatever headers it sends, because the TLS ClientHello
and HTTP/2 settings are fingerprinted too. Measured from a GitHub runner: plain
requests with a full browser header set get 403 on all five Cloudflare-fronted
sources; browser fingerprint impersonation gets 200 with byte counts matching a
residential connection.

This lives in its own process because the checker is pinned to Rust 1.88 (wasmer
5.x does not link on newer toolchains) while the Rust impersonation crates
require 1.98. Rather than fight that, the checker forwards each request here.

Listens on loopback only and speaks one endpoint:
    POST /fetch  {"url", "method", "headers": {}, "body_b64", "timeout"}
    ->           {"status", "url", "headers": {}, "body_b64"}
"""

import base64
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

try:
    from curl_cffi import requests
except ImportError:
    sys.exit("fetcher.py needs curl_cffi: pip install curl_cffi")

# Must track a current browser: a stale fingerprint is worse than none. Measured
# from a GitHub runner, chrome131 gets 403 where the current profile gets 200.
IMPERSONATE = os.environ.get("BUNY_IMPERSONATE", "chrome")

# One session, so Cloudflare clearance cookies are reused across requests.
SESSION = requests.Session(impersonate=IMPERSONATE)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):  # keep the checker's output readable
        pass

    def do_POST(self):
        if self.path != "/fetch":
            self.send_error(404)
            return
        try:
            length = int(self.headers.get("Content-Length", 0))
            req = json.loads(self.rfile.read(length) or b"{}")
        except (ValueError, TypeError) as e:
            self.send_error(400, f"bad request: {e}")
            return

        url = req.get("url")
        if not url:
            self.send_error(400, "missing url")
            return

        body = req.get("body_b64")
        try:
            response = SESSION.request(
                req.get("method", "GET"),
                url,
                headers=req.get("headers") or {},
                data=base64.b64decode(body) if body else None,
                timeout=req.get("timeout") or 30,
                allow_redirects=True,
            )
        except Exception as e:  # noqa: BLE001 - any failure is a failed fetch
            payload = {"error": f"{type(e).__name__}: {e}"}
            self._respond(502, payload)
            return

        self._respond(
            200,
            {
                "status": response.status_code,
                "url": str(response.url),
                "headers": {k: v for k, v in response.headers.items()},
                "body_b64": base64.b64encode(response.content).decode("ascii"),
            },
        )

    def _respond(self, code, payload):
        data = json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def main():
    port = int(os.environ.get("BUNY_FETCH_PORT", "8099"))
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"fetch gateway on http://127.0.0.1:{port} impersonating {IMPERSONATE}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()

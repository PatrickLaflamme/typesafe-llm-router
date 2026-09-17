#!/usr/bin/env python3
"""Offline Unity AI Gateway stand-in for Databricks demo recordings.

Serves OpenAI-compatible chat completions at:
  POST /ai-gateway/mlflow/v1/chat/completions
  POST /ai-gateway/openai/v1/chat/completions

Returns a short-classify label ("billing") so the OUTPUT beat matches the
b_short_classify fixture. No secrets; binds localhost only.
"""

from __future__ import annotations

import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOST = os.environ.get("MOCK_GATEWAY_BIND", "127.0.0.1")
PORT = int(os.environ.get("MOCK_GATEWAY_PORT", "18765"))
LABEL = os.environ.get("MOCK_GATEWAY_LABEL", "billing")

MLFLOW = "/ai-gateway/mlflow/v1/chat/completions"
OPENAI = "/ai-gateway/openai/v1/chat/completions"


class Handler(BaseHTTPRequestHandler):
    server_version = "MockDatabricksGateway/0.1"

    def log_message(self, fmt: str, *args) -> None:
        sys.stderr.write("[mock-gateway] " + (fmt % args) + "\n")

    def do_POST(self) -> None:  # noqa: N802
        if self.path not in (MLFLOW, OPENAI):
            self.send_error(404, f"unknown path {self.path}")
            return
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length) if length else b"{}"
        try:
            body = json.loads(raw.decode("utf-8") or "{}")
        except json.JSONDecodeError:
            self.send_error(400, "invalid json")
            return

        model = body.get("model", "unknown")
        auth = self.headers.get("Authorization", "")
        if not auth.startswith("Bearer "):
            self._json(401, {"error": "missing Bearer token"})
            return

        # Echo shape used by DatabricksAiGatewayProvider::complete.
        payload = {
            "id": "chatcmpl-mock-offline-demo",
            "object": "chat.completion",
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": LABEL},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 42, "completion_tokens": 1, "total_tokens": 43},
        }
        self._json(200, payload)

    def do_GET(self) -> None:  # noqa: N802
        if self.path in ("/", "/healthz"):
            self._json(200, {"ok": True, "paths": [MLFLOW, OPENAI]})
            return
        self.send_error(404)

    def _json(self, status: int, obj: dict) -> None:
        data = json.dumps(obj).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def main() -> None:
    httpd = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"mock Databricks AI Gateway on http://{HOST}:{PORT}", flush=True)
    print(f"  POST {MLFLOW}", flush=True)
    print(f"  POST {OPENAI}", flush=True)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nshutting down", flush=True)


if __name__ == "__main__":
    main()

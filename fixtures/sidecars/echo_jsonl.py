#!/usr/bin/env python3
"""Deterministic JSONL echo sidecar for subprocess transport tests."""

import json
import sys

for raw in sys.stdin:
    line = raw.strip()
    if not line:
        continue
    try:
        envelope = json.loads(line)
    except json.JSONDecodeError:
        continue

    if not isinstance(envelope, dict):
        continue

    payload = envelope.get("payload")
    if not isinstance(payload, dict):
        continue

    message = payload.get("message")
    if not isinstance(message, str):
        continue

    output = dict(envelope)
    output["topic"] = "echo.output"
    output["source"] = "echo_subprocess"
    output["payload"] = {
        "message": message,
        "echoed": True,
    }
    output["schema_hash"] = None

    sys.stdout.write(json.dumps(output, separators=(",", ":")) + "\n")
    sys.stdout.flush()

#!/usr/bin/env bash
# Live Cursor ModelSource morning demo helper.
# Requires CURSOR_API_KEY and: npm i @cursor/sdk
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -z "${CURSOR_API_KEY:-}" ]]; then
  echo "error: CURSOR_API_KEY is not set" >&2
  echo "Add it to the environment (Cloud Agent secrets or local export), then re-run." >&2
  echo "Do not invent or commit secrets." >&2
  exit 1
fi

if ! node -e "import('@cursor/sdk')" 2>/dev/null; then
  echo "note: @cursor/sdk not found — installing locally (not committed)" >&2
  npm i @cursor/sdk
fi

exec cargo run --quiet -- demo \
  --session examples/a_e/b_short_classify.json \
  --model-source cursor

#!/usr/bin/env bash
# Live Databricks AI Gateway smoke (lab machine only).
#
# Requires a prior `databricks auth login --host https://<workspace-url>`
# (optional --profile lab / DATABRICKS_CONFIG_PROFILE). Never commits secrets.
#
# Refuses to run without a nonempty DATABRICKS_HOST from `databricks auth env`.
# Never prints access tokens.
#
# Offline / CI: this script is not for GitHub Actions. Use stub:
#   cargo run -- demo --session examples/a_e/b_short_classify.json --model-provider stub
# Mock Databricks CLI/gateway scripts are not on main; do not expect MOCK=1 here.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CLI="${DATABRICKS_CLI:-databricks}"
PROFILE_ARGS=()
if [[ -n "${DATABRICKS_CONFIG_PROFILE:-}" ]]; then
  PROFILE_ARGS=(-p "$DATABRICKS_CONFIG_PROFILE")
fi

SESSION="${SMOKE_SESSION:-examples/a_e/b_short_classify_databricks.json}"

redact_token_json() {
  # Strip access_token values from JSON-ish CLI output so probing never leaks secrets.
  sed -E 's/("access_token"[[:space:]]*:[[:space:]]*")[^"]*"/\1<redacted>"/g'
}

require_cli() {
  if ! command -v "$CLI" >/dev/null 2>&1 && [[ ! -x "$CLI" ]]; then
    echo "error: Databricks CLI not found (looked for: $CLI)" >&2
    echo "hint: install the CLI, or set DATABRICKS_CLI to its path" >&2
    echo "docs: docs/databricks-live.md" >&2
    exit 1
  fi
}

require_auth_host() {
  local env_json host
  if ! env_json="$("$CLI" auth env "${PROFILE_ARGS[@]}" 2>/dev/null)"; then
    echo "error: missing Databricks CLI auth — run:" >&2
    echo "  databricks auth login --host https://<workspace-url>" >&2
    echo "optional: --profile lab / export DATABRICKS_CONFIG_PROFILE=lab" >&2
    exit 1
  fi

  host="$(
    printf '%s' "$env_json" | python3 -c '
import json, sys
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(2)
env = data.get("env") or {}
host = (env.get("DATABRICKS_HOST") or "").strip()
if not host:
    sys.exit(1)
print(host)
' 2>/dev/null
  )" || {
    echo "error: \`databricks auth env\` did not yield a nonempty DATABRICKS_HOST" >&2
    echo "hint: databricks auth login --host https://<workspace-url>" >&2
    exit 1
  }

  # Show host shape only (lab may use a real host locally — never echo tokens).
  echo "ok: DATABRICKS_HOST is set (${#host} chars)"
}

optional_token_probe() {
  # Optional sanity: prove token refresh works without printing the secret.
  if ! token_json="$("$CLI" auth token "${PROFILE_ARGS[@]}" 2>/dev/null)"; then
    echo "warn: \`databricks auth token\` failed — login may be expired" >&2
    return 0
  fi
  if printf '%s' "$token_json" | grep -q '"access_token"[[:space:]]*:[[:space:]]*"[^"][^"]*"'; then
    echo "ok: auth token present (redacted):"
    printf '%s\n' "$token_json" | redact_token_json
  else
    echo "warn: auth token response missing access_token — re-run auth login" >&2
  fi
}

require_session() {
  if [[ ! -f "$SESSION" ]]; then
    echo "error: session fixture not found: $SESSION" >&2
    echo "hint: use examples/a_e/b_short_classify_databricks.json" >&2
    echo "      (stock b_short_classify.json uses a Cursor current_model id)" >&2
    exit 1
  fi
}

main() {
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    cat <<'EOF'
Usage: bash scripts/smoke_databricks.sh

Live Databricks ModelProvider smoke (three-beat demo path).

Prerequisites:
  - Databricks CLI on PATH (or DATABRICKS_CLI)
  - databricks auth login --host https://<workspace-url>
  - optional: DATABRICKS_CONFIG_PROFILE=lab

Env:
  SMOKE_SESSION   session JSON (default: examples/a_e/b_short_classify_databricks.json)
  DATABRICKS_CLI / DATABRICKS_CONFIG_PROFILE — same as the Rust provider

Offline: use stub instead (no MOCK=1 path on main):
  cargo run -- demo --session examples/a_e/b_short_classify.json --model-provider stub

See docs/databricks-live.md and docs/demos.md §3.
EOF
    exit 0
  fi

  require_cli
  require_auth_host
  optional_token_probe
  require_session

  echo "=== smoke: demo --model-provider databricks ==="
  cargo run --quiet -- demo \
    --session "$SESSION" \
    --model-provider databricks
}

main "$@"

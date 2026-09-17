#!/usr/bin/env bash
# Offline stand-in for the Databricks CLI auth surface used by DatabricksAiGatewayProvider.
#
# Emulates:
#   databricks auth env    → DATABRICKS_HOST (points at local mock gateway)
#   databricks auth token  → access_token + expiry (fake; never a real secret)
#
# Other subcommands print a helpful message. No real tokens or workspace hosts.
set -euo pipefail

HOST="${MOCK_DATABRICKS_HOST:-http://127.0.0.1:18765}"
# Deliberately fake — never a live credential. Safe to show in recordings.
TOKEN="${MOCK_DATABRICKS_TOKEN:-mock-databricks-cli-token-offline-demo}"
EXPIRY="${MOCK_DATABRICKS_EXPIRY:-2099-01-01T00:00:00Z}"
PROFILE="${DATABRICKS_CONFIG_PROFILE:-DEFAULT}"

cmd1="${1:-}"
cmd2="${2:-}"

if [[ "$cmd1" == "auth" && "$cmd2" == "env" ]]; then
  # Mimic `databricks auth env` JSON shape (env.DATABRICKS_HOST).
  cat <<EOF
{
  "env": {
    "DATABRICKS_HOST": "${HOST}",
    "DATABRICKS_CONFIG_PROFILE": "${PROFILE}"
  }
}
EOF
  exit 0
fi

if [[ "$cmd1" == "auth" && "$cmd2" == "token" ]]; then
  # Mimic `databricks auth token` JSON shape.
  cat <<EOF
{
  "access_token": "${TOKEN}",
  "token_type": "Bearer",
  "expiry": "${EXPIRY}"
}
EOF
  exit 0
fi

if [[ "$cmd1" == "auth" && "$cmd2" == "login" ]]; then
  echo "mock databricks: auth login skipped (offline demo)" >&2
  echo "live: databricks auth login --host https://<workspace-url>" >&2
  exit 0
fi

if [[ "$cmd1" == "--version" || "$cmd1" == "version" ]]; then
  echo "Databricks CLI v0.0.0-mock-offline"
  exit 0
fi

echo "mock databricks CLI: unsupported args: $*" >&2
echo "supported: auth env | auth token | auth login | --version" >&2
exit 1

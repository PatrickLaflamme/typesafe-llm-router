#!/usr/bin/env bash
# Paced Databricks AI Gateway demo for terminal recording (offline / mocked).
#
# Story:
#   INPUT (short-classify) → Choice among Databricks list_models() ids (+ prices)
#   → ModelProvider.complete via CLI auth path → OUTPUT (selected_model + model_output)
#
# Prerequisites (started by this script when MOCK=1):
#   - scripts/mock_databricks_gateway.py on :18765
#   - DATABRICKS_CLI=scripts/mock_databricks_cli.sh
#
# Live mode (MOCK=0): requires real `databricks auth login` + workspace Gateway.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BIN="${ROUTER_BIN:-$ROOT/target/release/typesafe-llm-router}"
SESSION="${DEMO_SESSION:-examples/a_e/b_short_classify_databricks.json}"
MOCK="${MOCK:-1}"
LINE_DELAY="${DEMO_LINE_DELAY:-0.28}"
HOLD_SHORT="${DEMO_HOLD_SHORT:-2.0}"
HOLD_MED="${DEMO_HOLD_MED:-3.5}"
HOLD_LONG="${DEMO_HOLD_LONG:-6}"

GATEWAY_PID=""
cleanup() {
  if [[ -n "${GATEWAY_PID}" ]] && kill -0 "$GATEWAY_PID" 2>/dev/null; then
    kill "$GATEWAY_PID" 2>/dev/null || true
    wait "$GATEWAY_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

hold() { sleep "$1"; }

slow_print() {
  local line
  while IFS= read -r line || [[ -n "$line" ]]; do
    printf '%s\n' "$line"
    case "$line" in
      *'selected_model:'*|*'model_output:'*) hold "$HOLD_LONG" ;;
      *'chosen_model ='*|*'primary_reason:'*|*'list_models'*) hold "$HOLD_MED" ;;
      '=== INPUT ==='|'=== PROCESS ==='|'=== OUTPUT ==='|*'OUTCOME META'*) hold "$HOLD_SHORT" ;;
      '') hold 0.12 ;;
      *) hold "$LINE_DELAY" ;;
    esac
  done
}

banner() {
  echo
  echo "╔══════════════════════════════════════════════════════════════════╗"
  printf '║  %-64s║\n' "$1"
  echo "╚══════════════════════════════════════════════════════════════════╝"
  echo
}

if [[ ! -x "$BIN" ]]; then
  echo "building release binary…"
  cargo build --release -q
fi

clear || true
hold 1
banner "typesafe-llm-router — Databricks AI Gateway demo"

if [[ "$MOCK" == "1" ]]; then
  echo "  Mode: OFFLINE mock (CLI path + Gateway shape; no live workspace)"
  hold 1.2
  echo "  Auth story (what Patrick runs for real):"
  hold 0.8
  echo "    \$ databricks auth login --host https://<workspace-url>"
  hold 1.5
  echo "    \$ databricks auth env     # → DATABRICKS_HOST"
  hold 1.2
  echo "    \$ databricks auth token   # → access_token + expiry"
  hold "$HOLD_MED"

  export MOCK_DATABRICKS_HOST="http://127.0.0.1:18765"
  export DATABRICKS_CLI="$ROOT/scripts/mock_databricks_cli.sh"
  chmod +x "$DATABRICKS_CLI"
  python3 "$ROOT/scripts/mock_databricks_gateway.py" >/tmp/mock-databricks-gateway.log 2>&1 &
  GATEWAY_PID=$!
  for _ in $(seq 1 30); do
    if curl -sf "http://127.0.0.1:18765/healthz" >/dev/null 2>&1; then
      break
    fi
    sleep 0.1
  done
  echo
  echo "  Mock Gateway: http://127.0.0.1:18765  (mlflow chat completions)"
  echo "  Mock CLI:     \$DATABRICKS_CLI → auth env / auth token"
  hold "$HOLD_SHORT"
else
  echo "  Mode: LIVE Databricks CLI session"
  hold 1
  if ! command -v databricks >/dev/null 2>&1; then
    echo "error: databricks CLI not on PATH" >&2
    exit 1
  fi
  echo "  Checking CLI credentials…"
  databricks auth env >/dev/null
  databricks auth token >/dev/null
  echo "  OK — host + token from CLI"
  hold "$HOLD_SHORT"
fi

echo
banner "CLI auth probe (provider uses these under the hood)"
hold "$HOLD_SHORT"
echo "\$ \"\$DATABRICKS_CLI\" auth env"
hold 0.6
"$DATABRICKS_CLI" auth env 2>&1 | slow_print
hold "$HOLD_MED"
echo
echo "\$ \"\$DATABRICKS_CLI\" auth token   # token redacted in demo narration"
hold 0.6
# Never print a live token into the recording. Mock token is intentionally fake.
"$DATABRICKS_CLI" auth token | python3 -c '
import json, sys
obj = json.load(sys.stdin)
tok = obj.get("access_token", "")
obj["access_token"] = (tok[:8] + "…" + tok[-4:]) if len(tok) > 16 else "<redacted>"
print(json.dumps(obj, indent=2))
'
hold "$HOLD_MED"

echo
banner "Default list_models() catalog (ids + baked-in \$/MTok)"
hold "$HOLD_SHORT"
cat <<'EOF' | slow_print
  id                                         tier         in $/M   out $/M
  system.ai.databricks-gpt-oss-120b          T-small      0.10     0.40
  system.ai.claude-sonnet-4-5                T-mid        3.00    15.00
  system.ai.claude-opus-4-6                  T-frontier  15.00    75.00
EOF
hold "$HOLD_MED"

echo
banner "Route short-classify → Databricks ModelProvider"
hold "$HOLD_SHORT"
echo "Session: $SESSION"
hold 0.8
echo "Command:"
hold 0.5
echo "  cargo run -- demo --session $SESSION --model-provider databricks"
hold "$HOLD_MED"
echo
echo "Also equivalent:"
hold 0.4
echo "  cargo run -- route --session $SESSION --model-provider databricks --execute"
hold "$HOLD_SHORT"
echo

"$BIN" demo --session "$SESSION" --model-provider databricks 2>&1 | slow_print

hold "$HOLD_LONG"
banner "RESULT — Patrick lock check"
echo "  Choice allowlist  = Databricks list_models() ids (with prices on ModelInfo)"
hold 1.5
echo "  chosen_model      = id passed to ModelProvider.complete (no remap)"
hold 1.5
echo "  Score             = async / pending (not on hot path)"
hold "$HOLD_MED"
echo
if [[ "$MOCK" == "1" ]]; then
  echo "  Offline mock complete. For live Gateway:"
  hold 1
  echo "    databricks auth login --host https://<workspace-url>"
  hold 1.2
  echo "    cargo run -- demo --session $SESSION --model-provider databricks"
  hold 1.2
  echo "    # or:  … route … --model-provider databricks --execute"
fi
hold 4
echo
echo "Done."
hold 3

#!/usr/bin/env bash
# Escalation demo for screen recording: trivial → complex, T-small → T-frontier.
# Paced for humans: line-by-line output + long holds on key beats.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/release/typesafe-llm-router"
cd "$ROOT"

# Seconds between ordinary lines / after important lines.
LINE_DELAY="${DEMO_LINE_DELAY:-0.35}"
HOLD_SHORT="${DEMO_HOLD_SHORT:-2.5}"
HOLD_MED="${DEMO_HOLD_MED:-4}"
HOLD_LONG="${DEMO_HOLD_LONG:-7}"

hold() { sleep "$1"; }

# Print stdin slowly; linger longer on decision / section headers.
slow_print() {
  local line
  while IFS= read -r line || [[ -n "$line" ]]; do
    printf '%s\n' "$line"
    case "$line" in
      *'selected_model:'*) hold "$HOLD_LONG" ;;
      *'primary_reason:'*|*'chosen_model ='*) hold "$HOLD_MED" ;;
      '=== INPUT ==='|'=== PROCESS ==='|'=== OUTPUT ==='|*'TURN '*|*'RESULT'*) hold "$HOLD_SHORT" ;;
      '') hold 0.15 ;;
      *) hold "$LINE_DELAY" ;;
    esac
  done
}

banner() {
  echo
  echo "╔══════════════════════════════════════════════════════════════╗"
  printf '║  %-60s║\n' "$1"
  echo "╚══════════════════════════════════════════════════════════════╝"
  echo
}

clear || true
hold 1
banner "typesafe-llm-router — paced routing demo"
echo "  Story:"
hold 0.8
echo "    1) Trivial prompt      → cheap model  (T-small)"
hold 1.2
echo "    2) Complex follow-up  → frontier model (T-frontier)"
hold "$HOLD_MED"

# ── Turn 1 ──────────────────────────────────────────────────────────
banner "TURN 1 of 2 — trivial classify prompt"
hold "$HOLD_SHORT"
echo "User prompt:"
hold 0.6
echo "  \"I was charged twice for my annual plan. Please fix.\""
hold "$HOLD_MED"
echo
echo "Routing… (expect composer-2.5 / T-small)"
hold "$HOLD_SHORT"
echo
"$BIN" demo --session examples/demo/01_trivial_prompt.json --model-source stub 2>&1 | slow_print
hold "$HOLD_LONG"
echo
echo ">>> Pause: Turn 1 stayed on the cheap tier (T-small)."
hold "$HOLD_LONG"

# ── Turn 2 ──────────────────────────────────────────────────────────
banner "TURN 2 of 2 — same chat, super-complex follow-up"
hold "$HOLD_SHORT"
echo "Still on current_model: composer-2.5"
hold 1.2
echo "New user follow-up (hard / long-reason):"
hold 0.8
echo "  Design a multi-region billing reconciliation system…"
hold 1.0
echo "  CAP tradeoffs, sagas/outbox, threat model, rollback plan."
hold "$HOLD_MED"
echo
echo "Routing… (expect UPGRADE → grok-4.5 / T-frontier)"
hold "$HOLD_SHORT"
echo
"$BIN" demo --session examples/demo/02_complex_followup.json --model-source stub 2>&1 | slow_print
hold "$HOLD_LONG"
echo
echo ">>> Pause: Turn 2 upgraded for quality (T-frontier)."
hold "$HOLD_LONG"

# ── Summary ─────────────────────────────────────────────────────────
banner "RESULT — model switched up a tier"
echo "  Turn 1  →  composer-2.5   [T-small]"
hold 2
echo "  Turn 2  →  grok-4.5       [T-frontier]   ← upgraded"
hold "$HOLD_LONG"
echo
echo "Done. Demo complete."
hold 5

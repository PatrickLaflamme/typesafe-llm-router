#!/usr/bin/env bash
# Escalation demo for screen recording: trivial → complex, T-small → T-frontier.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/release/typesafe-llm-router"
cd "$ROOT"

clear
printf '\n'
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  typesafe-llm-router — live routing demo                     ║"
echo "║  Turn 1: trivial prompt  → expect T-small                    ║"
echo "║  Turn 2: complex follow-up → expect T-frontier upgrade       ║"
echo "╚══════════════════════════════════════════════════════════════╝"
sleep 2

echo
echo "────────────────────────────────────────────────────────────────"
echo "  TURN 1 — trivial classify prompt"
echo "────────────────────────────────────────────────────────────────"
sleep 1
"$BIN" demo --session examples/demo/01_trivial_prompt.json --model-source stub
sleep 3

echo
echo "────────────────────────────────────────────────────────────────"
echo "  TURN 2 — same chat, super-complex follow-up"
echo "  (current_model still composer-2.5 → router should UPGRADE)"
echo "────────────────────────────────────────────────────────────────"
sleep 1
"$BIN" demo --session examples/demo/02_complex_followup.json --model-source stub
sleep 2

echo
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  RESULT                                                      ║"
echo "║  Turn 1 selected: composer-2.5 [T-small]                     ║"
echo "║  Turn 2 selected: grok-4.5     [T-frontier]  ← upgraded      ║"
echo "╚══════════════════════════════════════════════════════════════╝"
sleep 4

#!/usr/bin/env bash
set -euo pipefail
cd /workspace
export PATH="/workspace/target/debug:$PATH"
clear
printf '\n'
echo '================================================================'
echo '  Morning demo — typesafe-llm-router (stub ModelSource)'
echo '================================================================'
printf '\n'
echo '$ cargo run -- demo --session examples/a_e/b_short_classify.json --model-source stub'
printf '\n'
sleep 1.5
typesafe-llm-router demo --session examples/a_e/b_short_classify.json --model-source stub
printf '\n'
echo '---'
echo 'Replay note: Choice allowlist from ModelSource.list_models (Patrick lock).'
echo 'selected_model must be a ModelSource id (composer-2.5), not gpt-4o-mini.'
printf '\n'
exec bash

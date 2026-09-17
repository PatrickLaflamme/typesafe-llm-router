# Databricks AI Gateway demo

Terminal recording of the typesafe-llm-router pipeline against the **Databricks
Unity AI Gateway** ModelProvider:

```text
INPUT → Choice among list_models() ids (+ baked-in $/MTok)
      → --model-provider databricks --execute
      → selected_model + model_output
      → Score async (pending)
```

## Artifacts

| File | What |
| --- | --- |
| [`databricks-ai-gateway-demo.mp4`](./databricks-ai-gateway-demo.mp4) | Paced terminal demo (~84s) |
| [`databricks-ai-gateway-demo.cast`](./databricks-ai-gateway-demo.cast) | asciinema source (`asciinema play …`) |

**Mode recorded here: OFFLINE mock.** This environment has no Databricks CLI
login / workspace. The recording still exercises the real CLI auth surface
(`auth env` / `auth token`) and the OpenAI-compatible Gateway request/response
shape via localhost stand-ins. No live tokens or workspace hosts appear in the
artifacts.

## Captured OUTPUT (from this recording)

```text
selected_model: system.ai.databricks-gpt-oss-120b [T-small]
primary_reason: continue_current
model_output:
billing
```

Allowlist presented to Choice (from `DatabricksAiGatewayProvider::list_models`):

| id | tier_hint | price_input $/MTok | price_output $/MTok |
| --- | --- | ---: | ---: |
| `system.ai.databricks-gpt-oss-120b` | T-small | 0.10 | 0.40 |
| `system.ai.claude-sonnet-4-5` | T-mid | 3.00 | 15.00 |
| `system.ai.claude-opus-4-6` | T-frontier | 15.00 | 75.00 |

Patrick lock checks visible in the video:

- Choice allowlist = provider `list_models()` ids (with prices on `ModelInfo`)
- `chosen_model` == id passed to `ModelProvider.complete` (no remap)
- Score status `Pending` (async; not on hot path)

## Session fixture

Uses the short-classify ticket from `examples/a_e/b_short_classify.json`, with
`current_model` set to a Databricks catalog id (the stock fixture’s
`composer-2.5` is a Cursor id and is rejected by `allowlist_from_provider`):

```text
examples/a_e/b_short_classify_databricks.json
```

## Replay the offline mock locally

```bash
# terminal 1 — mock Gateway (mlflow + openai chat completions)
python3 scripts/mock_databricks_gateway.py

# terminal 2 — point the provider at the mock CLI + localhost host
export DATABRICKS_CLI="$PWD/scripts/mock_databricks_cli.sh"
export MOCK_DATABRICKS_HOST="http://127.0.0.1:18765"

cargo run --release -- demo \
  --session examples/a_e/b_short_classify_databricks.json \
  --model-provider databricks

# equivalent execute path
cargo run --release -- route \
  --session examples/a_e/b_short_classify_databricks.json \
  --model-provider databricks --execute
```

Or one-shot paced script (starts mock Gateway for you):

```bash
bash scripts/record_databricks_demo.sh
# MOCK=0 bash scripts/record_databricks_demo.sh   # live CLI (see below)
```

### Env vars used by the mock

| Variable | Purpose |
| --- | --- |
| `DATABRICKS_CLI` | Path to `scripts/mock_databricks_cli.sh` |
| `MOCK_DATABRICKS_HOST` | Host returned by mock `auth env` (default `http://127.0.0.1:18765`) |
| `MOCK_DATABRICKS_TOKEN` | Fake token returned by mock `auth token` (safe placeholder) |
| `MOCK_GATEWAY_PORT` | Mock Gateway bind port (default `18765`) |
| `MOCK_GATEWAY_LABEL` | Completion content (default `billing`) |

Optional provider overrides (same as live):

| Variable | Purpose |
| --- | --- |
| `DATABRICKS_CONFIG_PROFILE` | CLI profile (`-p`) |
| `DATABRICKS_AI_GATEWAY_MODELS` | Override catalog (JSON `ModelInfo[]` or comma-separated FQNs) |
| `DATABRICKS_MODEL_PROVIDER_SERVICE` | Switch to `/ai-gateway/openai/v1` + header |

## What Patrick must run after live login

No tokens are invented here. After authenticating against a real workspace:

```bash
# 1) Install Databricks CLI if needed, then login (U2M)
databricks auth login --host https://<workspace-url>
# optional named profile:
# databricks auth login --host https://<workspace-url> --profile lab
# export DATABRICKS_CONFIG_PROFILE=lab

# 2) Sanity-check what the provider will read
databricks auth env      # must include DATABRICKS_HOST
databricks auth token    # access_token + expiry (do not commit)

# 3) Ensure model services in the default catalog exist on the workspace,
#    or override:
# export DATABRICKS_AI_GATEWAY_MODELS='system.ai.claude-sonnet-4-5,…'

# 4) Morning demo / execute (Typesafe Choice stays stub unless TYPESAFE_API_KEY is set)
cargo run --release -- demo \
  --session examples/a_e/b_short_classify_databricks.json \
  --model-provider databricks

cargo run --release -- route \
  --session examples/a_e/b_short_classify_databricks.json \
  --model-provider databricks --execute
```

Expected live OUTPUT shape (model id depends on Choice among your catalog):

```text
=== OUTPUT ===
selected_model: <one of list_models ids> [T-…]
primary_reason: …
model_output:
<gateway completion text>
```

## Re-record

```bash
cargo build --release
DEMO_LINE_DELAY=0.32 DEMO_HOLD_SHORT=2.2 DEMO_HOLD_MED=3.5 DEMO_HOLD_LONG=5.5 \
  asciinema rec recordings/databricks-ai-gateway-demo.cast --overwrite \
  --command "bash scripts/record_databricks_demo.sh"

# GIF → mp4 (agg from https://github.com/asciinema/agg)
agg --cols 100 --rows 36 --font-size 16 \
  recordings/databricks-ai-gateway-demo.cast /tmp/databricks-ai-gateway-demo.gif
ffmpeg -y -i /tmp/databricks-ai-gateway-demo.gif \
  -movflags +faststart -pix_fmt yuv420p \
  -vf "scale=trunc(iw/2)*2:trunc(ih/2)*2" \
  recordings/databricks-ai-gateway-demo.mp4
```

## Design refs

- Patrick lock: `docs/model-provider.md`
- Provider impl: `src/model_provider/databricks_ai_gateway.rs`
- Base branch: `cursor/model-provider-databricks-ci-b121` (draft PR #6)

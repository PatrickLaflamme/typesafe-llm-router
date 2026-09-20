# Demos (copy-paste)

Offline stub works with no keys. Cursor and Databricks live paths need local auth —
never commit secrets. Deeper design: [model-provider](model-provider.md),
[score-feedback-loop](score-feedback-loop.md). Live Databricks checklist:
[databricks-live](databricks-live.md).

## 1. Stub morning recording (offline)

No network. Score stays pending on the hot path (drain later if you want).

```bash
cargo run -- demo --session examples/a_e/b_short_classify.json --model-provider stub
```

Equivalent route form:

```bash
cargo run -- route --session examples/a_e/b_short_classify.json --model-provider stub --execute --verbose
```

## 2. Cursor live (`CURSOR_API_KEY` required)

Install the Node sidecar dependency once (pinned in root `package.json`):

```bash
npm i
# or: npm i @cursor/sdk
export CURSOR_API_KEY=…    # never commit; see .env.example
cargo run -- route --session examples/a_e/b_short_classify.json --model-provider cursor --execute
```

Without `CURSOR_API_KEY`, the Cursor provider fails fast with a missing-key error.
Sidecar: `scripts/cursor_agent_complete.mjs` (ESM `import fs` — do not switch to `require`).

## 3. Databricks live (CLI checklist)

Lab-only live smoke against Unity AI Gateway. **Do not** put a real workspace host,
token, or `.databrickscfg` in git / PR / CHANGELOG. Use placeholders:
`https://<workspace-url>` or `https://adb-xxxxxxxxxxxx.azuredatabricks.net`.
Short pointer: [databricks-live.md](databricks-live.md).

### Prerequisites

1. Install the [Databricks CLI](https://docs.databricks.com/aws/en/dev-tools/cli/install)
   (`databricks` on `PATH`, or set `DATABRICKS_CLI`).
2. A workspace where the model FQNs you need are enabled for AI Gateway.

### Login

```bash
databricks auth login --host https://<workspace-url>
# optional named profile:
# databricks auth login --host https://<workspace-url> --profile lab
# export DATABRICKS_CONFIG_PROFILE=lab
```

### Sanity (never commit token output)

```bash
databricks auth env     # expect nonempty DATABRICKS_HOST in the JSON env block
databricks auth token   # proves refresh works — redacted locally; never paste into git
```

### Smoke commands

Use the Databricks short-classify fixture (`current_model` is a Databricks catalog
id). The stock `b_short_classify.json` uses `composer-2.5` (Cursor) and will fail
allowlist checks under `--model-provider databricks`.

```bash
# one-shot script (refuses to run without auth host; never prints access tokens)
bash scripts/smoke_databricks.sh

# equivalent three-beat demo
cargo run -- demo \
  --session examples/a_e/b_short_classify_databricks.json \
  --model-provider databricks

# route form
cargo run -- route \
  --session examples/a_e/b_short_classify_databricks.json \
  --model-provider databricks --execute
```

If you prefer not to use the dedicated fixture, copy
`examples/a_e/b_short_classify.json` and set `current_model` to a Databricks
catalog id from `list_models()` (e.g. `system.ai.databricks-gpt-oss-120b`).

### Expected OUTPUT beats

Terminal shows **INPUT → PROCESS → OUTPUT**. On success, OUTPUT includes:

- `selected_model` — Databricks FQN (same id Choice chose / `complete` received)
- `model_output` — completion text

**Score stays pending / async** (`scores_status: Pending`); drain later if desired:

```bash
cargo run -- drain-score-queue
```

### Common failures

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `missing Databricks CLI auth` / empty `DATABRICKS_HOST` | Not logged in | `databricks auth login --host https://<workspace-url>` |
| `auth token` empty / CLI error after idle | Expired U2M session | Re-run `auth login` (optional `--profile lab`) |
| HTTP 4xx mentioning model / not found / not enabled | Model FQN not enabled on this workspace | Enable the FQN in AI Gateway, or override via `DATABRICKS_AI_GATEWAY_MODELS` |
| `current_model … is not in the ModelProvider allowlist` | Fixture still has a Cursor id | Use `b_short_classify_databricks.json` (or set a Databricks `current_model`) |

### Offline / CI

**Do not** add a live Databricks GitHub Actions job. Offline:

```bash
cargo run -- demo --session examples/a_e/b_short_classify.json --model-provider stub
```

Integration coverage uses an in-process mock transport (no live workspace) — see
`tests/integration_model_provider.rs` and [model-provider.md](model-provider.md).
Mock CLI/gateway scripts are **not** on `main`; there is no `MOCK=1` smoke path here.

## Async Score (optional drain)

After any demo/route with execute, outcomes enqueue under `.router-data/score-queue/`:

```bash
cargo run -- drain-score-queue
```

`--model-source` is a documented alias of `--model-provider` only.

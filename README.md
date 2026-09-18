# typesafe-llm-router

Very low-latency **Rust library + CLI**: System One **Choice** routes a session →
**ModelProvider** executes → return decision + `model_output`. **Score is async**
(never on the hot path).

**R&D only.** Docs: [decision-rules](docs/decision-rules.md) ·
[score-feedback-loop](docs/score-feedback-loop.md) · [model-provider](docs/model-provider.md).

## Design lock (Patrick VISION LOCKED · 2026-09-17)

Choice allowlist + token prices come from `ModelProvider.list_models()`.
`RouterDecision.chosen_model` **is** the id passed to `ModelProvider.complete`
(no gpt-4o-mini → composer remap). Score stays async / off the hot path.
No secrets in repo.

> Naming: prefer **`ModelProvider`** everywhere. CLI flag `--model-provider`
> (`--model-source` is a visible alias only).

## Production path (demo → live)

```text
ModelProvider.list_models() → Choice → ModelProvider.complete → decision + model_output
                                                                  └→ async Score (queue)
```

### 1. Stub (offline, CI / morning recording)

```bash
cargo run -- demo --session examples/a_e/b_short_classify.json --model-provider stub
# or:
cargo run -- route --session examples/a_e/b_short_classify.json --stub --execute --verbose
```

Terminal shows three beats: **INPUT** → **PROCESS** (`list_models` → Choice →
`complete` → score pending) → **OUTPUT** (`selected_model` + `model_output`).

### 2. Cursor execute (live ModelProvider)

```bash
npm i @cursor/sdk          # once — Node sidecar scripts/cursor_agent_complete.mjs
export CURSOR_API_KEY=…    # never commit
cargo run -- route --session examples/a_e/b_short_classify.json --model-provider cursor --execute
```

### 3. Databricks AI Gateway (CLI auth)

```bash
databricks auth login --host https://<workspace-url>
# optional: export DATABRICKS_CONFIG_PROFILE=lab
cargo run -- route --session examples/a_e/b_short_classify.json --model-provider databricks --execute
```

Host + token/expiry come from `databricks auth env` / `databricks auth token`
(see [model-provider](docs/model-provider.md)).

### Async Score

Hot path never waits on Score. Outcomes enqueue under `.router-data/score-queue/`;
drain later:

```bash
cargo run -- drain-score-queue
```

## Quick smoke (A–E)

```bash
cargo run -- route --session examples/a_e/a_routine_code_fix.json --stub --execute
for f in examples/a_e/*.json; do cargo run --quiet -- route --session "$f" --stub --execute; done
```

## Tests / CI

```bash
cargo test                  # unit + integration + e2e (stub; no live keys)
cargo clippy -- -D warnings
cargo fmt --check
```

GitHub Actions: `.github/workflows/ci.yml` runs fmt, clippy, unit, integration, and e2e jobs.

## Env (no secrets in repo)

| Variable | Purpose |
| --- | --- |
| `TYPESAFE_API_KEY` | Live Choice / Score |
| `TYPESAFE_BASE_URL` | Optional Typesafe base URL |
| `CURSOR_API_KEY` | Live Cursor ModelProvider (`--model-provider cursor`) |
| `CURSOR_AGENT_HELPER` | Optional override for `scripts/cursor_agent_complete.mjs` |
| `DATABRICKS_CONFIG_PROFILE` | Optional CLI profile after `databricks auth login` |
| `DATABRICKS_CLI` | Optional path to `databricks` binary |
| `DATABRICKS_AI_GATEWAY_MODELS` | Optional model catalog override |
| `DATABRICKS_MODEL_PROVIDER_SERVICE` | Optional UC provider-service name |

## Types

| Type | Role |
| --- | --- |
| `RouterDecision` | Choice result (`chosen_model` = ModelProvider id) |
| `RouteOutcome` | Turn record + async scores |
| `ModelProvider` | `list_models` + `complete` (Stub / Cursor / Databricks) |
| `ModelInfo` | Provider id + $/MTok (+ optional `tier_hint`) |
| Score rubrics | `quality`, `instruction_follow`, optional `task_fit` |

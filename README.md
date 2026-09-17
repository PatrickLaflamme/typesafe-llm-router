# typesafe-llm-router

Very low-latency **Rust library + CLI**: System One **Choice** routes a session →
**ModelProvider** executes → return decision + `model_output`. **Score is async**
(never on the hot path).

**R&D only.** Docs: [decision-rules](docs/decision-rules.md) ·
[score-feedback-loop](docs/score-feedback-loop.md) · [model-provider](docs/model-provider.md).

## Design lock (2026-09-17)

Choice allowlist + token prices come from `ModelProvider.list_models()`.
`RouterDecision.chosen_model` **is** the id passed to `ModelProvider.complete`
(no gpt-4o-mini → composer remap).

> Naming: `ModelSource` / informal “ModelService” → **`ModelProvider`**.

## Morning demo command

```bash
cargo run -- demo --session examples/a_e/b_short_classify.json --model-provider stub
```

Terminal shows three beats:

1. **INPUT** — session / prompt from the fixture
2. **PROCESS** — `list_models` → Choice (stub) → `ModelProvider.complete` (stub) → score pending
3. **OUTPUT** — `selected_model: composer-2.5 […]` + **`model_output: billing`**

Also: `cargo run -- route --session examples/a_e/b_short_classify.json --stub --execute --verbose`

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
| `TYPESAFE_BASE_URL` | Optional |
| `CURSOR_API_KEY` | Live Cursor ModelProvider (`--model-provider cursor`) |
| `DATABRICKS_HOST` | Workspace URL for AI Gateway |
| `DATABRICKS_TOKEN` | Token for `--model-provider databricks` |

## Types

| Type | Role |
| --- | --- |
| `RouterDecision` | Choice result (`chosen_model` = ModelProvider id) |
| `RouteOutcome` | Turn record + async scores |
| `ModelProvider` | `list_models` + `complete` (Stub / Cursor / Databricks) |
| `ModelInfo` | Provider id + $/MTok (+ optional `tier_hint`) |
| Score rubrics | `quality`, `instruction_follow`, optional `task_fit` |

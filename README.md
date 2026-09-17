# typesafe-llm-router

Very low-latency **Rust library + CLI**: System One **Choice** routes a session → **ModelSource** executes → return decision + `model_output`. **Score is async** (never on the hot path).

**R&D only.** Docs: [decision-rules](docs/decision-rules.md) · [score-feedback-loop](docs/score-feedback-loop.md) · [model-source](docs/model-source.md).

## Morning demo command

```bash
cargo run -- demo --session examples/a_e/b_short_classify.json --stub
```

Terminal shows three beats:

1. **INPUT** — session / prompt from the fixture  
2. **PROCESS** — enrich → Choice route (stub) → `ModelSource.complete` (stub) → score pending/enqueued  
3. **OUTPUT** — `selected_model` + **`model_output`** (e.g. `billing` for short-classify)

Also: `cargo run -- route --session examples/a_e/b_short_classify.json --stub --execute --verbose`

## Quick smoke (A–E)

```bash
cargo run -- route --session examples/a_e/a_routine_code_fix.json --stub
for f in examples/a_e/*.json; do cargo run --quiet -- route --session "$f" --stub; done
```

## Env (no secrets in repo)

| Variable | Purpose |
| --- | --- |
| `TYPESAFE_API_KEY` | Live Choice / Score |
| `TYPESAFE_BASE_URL` | Optional |
| `CURSOR_API_KEY` | Live Cursor ModelSource helper |

## Types

| Type | Role |
| --- | --- |
| `RouterDecision` | Choice result (`chosen_model`, alternatives, cache_hypothesis, rough_cost_note, primary_reason) |
| `RouteOutcome` | Turn record + async scores (`scores_status`: pending\|ok\|failed) |
| `ModelSource` | `complete` after Choice (Stub / Cursor) |
| Score rubrics | `quality`, `instruction_follow`, optional `task_fit` |

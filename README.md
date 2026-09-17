# typesafe-llm-router

Very low-latency **Rust library + CLI**: System One **Choice** routes a session →
**ModelSource** executes → return decision + `model_output`. **Score is async**
(never on the hot path).

**R&D only.** Docs: [decision-rules](docs/decision-rules.md) ·
[score-feedback-loop](docs/score-feedback-loop.md) · [model-source](docs/model-source.md).

## Design lock (2026-09-17)

Choice allowlist + token prices come from `ModelSource.list_models()`.
`RouterDecision.chosen_model` **is** the id passed to `ModelSource.complete`
(no gpt-4o-mini → composer remap).

## Morning demo command

```bash
cargo run -- demo --session examples/a_e/b_short_classify.json --model-source stub
```

Terminal shows three beats:

1. **INPUT** — session / prompt from the fixture
2. **PROCESS** — `list_models` → Choice (stub) → `ModelSource.complete` (stub) → score pending
3. **OUTPUT** — `selected_model: composer-2.5 […]` + **`model_output: billing`**

Also: `cargo run -- route --session examples/a_e/b_short_classify.json --stub --execute --verbose`

## Quick smoke (A–E)

```bash
cargo run -- route --session examples/a_e/a_routine_code_fix.json --stub --execute
for f in examples/a_e/*.json; do cargo run --quiet -- route --session "$f" --stub --execute; done
```

## Env (no secrets in repo)

| Variable | Purpose |
| --- | --- |
| `TYPESAFE_API_KEY` | Live Choice / Score |
| `TYPESAFE_BASE_URL` | Optional |
| `CURSOR_API_KEY` | Live Cursor ModelSource helper (`--model-source cursor`) |

## Types

| Type | Role |
| --- | --- |
| `RouterDecision` | Choice result (`chosen_model` = ModelSource id) |
| `RouteOutcome` | Turn record + async scores |
| `ModelSource` | `list_models` + `complete` (Stub / Cursor) |
| `ModelInfo` | Source id + $/MTok (+ optional `tier_hint`) |
| Score rubrics | `quality`, `instruction_follow`, optional `task_fit` |

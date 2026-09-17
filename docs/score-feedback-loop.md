# Score feedback loop (Phase A — collect only)

> **R&D / lab design.** Not production policy.  
> **Critical:** Score is **async** and must **never** affect hot-path latency.

## Pipeline

```
session → Choice route → model_output → return decision + output immediately
                              ↓
                    persist RouteOutcome (scores_status: pending)
                              ↓
                         enqueue Score job
                              ↓
              worker: System One Score → patch scores (ok|failed)
```

| Phase | What | Status in this crate |
| --- | --- | --- |
| **A** | Collect `RouteOutcome` (+ async Score) | Scaffolded |
| **B** | Offline aggregates by `(task_class, tier)` | Not wired |
| **C** | Online nudge of routing from scores | **Do not auto-wire** |

## Hot path vs Score

1. **Hot path:** `Router::route` (Choice) → caller produces `model_output` → `complete_turn_hot_path` persists pending `RouteOutcome`, enqueues `ScoreJob`, returns. **No Score await.**
2. **After output is durable:** worker / `drain_score_queue` / `run_score_job` calls System One Score and patches the outcome.
3. Score failures set `scores_status: failed` off the request thread; retry in the worker only.

## Types

| Name | Role |
| --- | --- |
| `RouterDecision` | Choice result: `chosen_model`, `alternatives_considered`, `cache_hypothesis`, `rough_cost_note`, `primary_reason`, … |
| `RouteOutcome` | Full turn record: signals + decision + `model_output` + async scores meta |
| `ScoreJob` | Queue payload for the worker |
| `OutcomeScores` | Rubric results: `quality`, `instruction_follow`, optional `task_fit` |

### Score rubrics (v0)

| Id | Levels (low → high) |
| --- | --- |
| `quality` | Wrong/unusable → Partially correct → Correct usable → Correct + robust |
| `instruction_follow` | Ignores constraints → Mostly follows → Follows cleanly |
| `task_fit` | Optional later (type stubbed; not required for A–E) |

Score state sent to TypeSafe includes: `incoming_prompt`, `chosen_model`, `model_output`, `task_class`, decision fields.

### `RouteOutcome` score fields

- `scores_status`: `pending` \| `ok` \| `failed`
- `scores`: filled when `ok`
- `scored_at`: null until done
- `client_mode`: `stub` \| `live`
- `typesafe_model`: e.g. `jev-latest`

## CLI

```bash
# Hot path: route only (RouterDecision JSON)
cargo run -- route --session examples/a_e/b_short_classify.json --stub

# Hot path + model_output → pending RouteOutcome + enqueue (does NOT wait on Score)
echo "billing" > /tmp/out.txt
cargo run -- route --session examples/a_e/b_short_classify.json --stub \
  --model-output /tmp/out.txt \
  --outcomes-dir .router-data/outcomes \
  --score-queue-dir .router-data/score-queue

# Worker: drain Score jobs and patch outcomes
cargo run -- drain-score-queue --stub \
  --outcomes-dir .router-data/outcomes \
  --score-queue-dir .router-data/score-queue

# LAB ONLY — inline Score (experiments). Not product path.
cargo run -- route --session examples/a_e/b_short_classify.json --stub \
  --model-output /tmp/out.txt --score-inline
```

Env: `TYPESAFE_API_KEY`, optional `TYPESAFE_BASE_URL` (live worker / live route).

## Library sketch

```rust
use typesafe_llm_router::{
    complete_turn_hot_path, drain_score_queue, ClientMode, InMemoryScoreQueue, Router,
    StubTypesafeClient,
};

// hot path — no Score await
let hot = complete_turn_hot_path(&router, &request, model_output, None, ClientMode::Stub, &queue, &store)?;

// later, worker
drain_score_queue(&client, &queue, &store, 32)?;
```

## Related

- [Decision rules](decision-rules.md) — paper routing policy / A–E examples
- Catalog: `config/models.example.toml`

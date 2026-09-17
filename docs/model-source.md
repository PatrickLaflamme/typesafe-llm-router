# ModelSource — pluggable execution after Choice

> After System One **Choice** picks a model id, call [`ModelSource::complete`](../src/model_source/mod.rs) to produce `model_output`. Then return decision + output; **async Score** runs later (`docs/score-feedback-loop.md`).

## Pipeline

```
session → Choice route → ModelSource.complete → return decision + model_output
                                              → persist RouteOutcome (scores pending)
                                              → enqueue Score (async; never on hot path)
```

## Trait (Typesafe Router lab)

```text
trait ModelSource {
  name() -> string
  list_models() -> Result<Vec<ModelInfo>>
  complete(req: CompleteRequest) -> Result<CompleteResponse>
}

CompleteRequest { model_id, prompt, messages?, cwd?, runtime: local|cloud, cloud_repos?, model_params?, timeout_ms? }
CompleteResponse { model_output, source, model_id, run_id?, usage?, raw_meta? }
ModelInfo { id, label?, tier?: T-small|T-mid|T-frontier }
```

Rust: sync `fn` for v0. Cursor may be slow — that latency is **model-execution**, not Score.

## Implementations

| Source | Module | Notes |
| --- | --- | --- |
| **StubModelSource** | `src/model_source/stub.rs` | Offline fixture text (e.g. short-classify → `billing`). No network. |
| **CursorAgentSdkSource** | `src/model_source/cursor_agent.rs` | Spawns `scripts/cursor_agent_complete.mjs` (`@cursor/sdk`). Needs `CURSOR_API_KEY`. |

Tier → Cursor id map (placeholders): `config/model-map.toml`.

### Adding OpenAI / others later

1. New file under `src/model_source/` implementing `ModelSource`.
2. Re-export from `mod.rs` / `lib.rs`.
3. Select via CLI flag (future). Do not bake provider secrets into the repo.

## Env

| Variable | Purpose |
| --- | --- |
| `TYPESAFE_API_KEY` | Live System One Choice / Score |
| `TYPESAFE_BASE_URL` | Optional API host override |
| `CURSOR_API_KEY` | Live Cursor Agent helper (never commit) |
| `CURSOR_AGENT_HELPER` | Override path to Node helper |

## Morning demo

```bash
cargo run -- demo --session examples/a_e/b_short_classify.json --stub
```

Shows INPUT → process logs → OUTPUT (`selected_model` + `model_output`).

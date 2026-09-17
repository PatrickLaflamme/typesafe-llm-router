# ModelSource (Typesafe Router lab design)

> Pluggable execution **after** System One Choice.  
> Pipeline: `Choice → ModelSource.complete → return decision+output → async Score`.  
> Score never blocks the hot path (`docs/score-feedback-loop.md`).

## Trait

```text
trait ModelSource {
  name() -> string
  list_models() -> Result<Vec<ModelInfo>>
  complete(req: CompleteRequest) -> Result<CompleteResponse>
}

CompleteRequest {
  model_id,
  prompt,
  cwd?,
  runtime: local | cloud,
  cloud_repos?,
  model_params?,
  timeout_ms?
}

CompleteResponse {
  model_output,
  source,
  model_id,
  run_id?,
  usage?,
  raw_meta?
}

ModelInfo {
  id,
  label?,
  tier?: T-small | T-mid | T-frontier
}
```

Rust location: [`src/model_source/mod.rs`](../src/model_source/mod.rs). Sync `fn` for v0.  
Optional extension on `CompleteRequest`: `messages` (full transcript) for chat-style sources — not required by the lab contract.

## Implementations

| Impl | Path | Behavior |
| --- | --- | --- |
| **StubModelSource** | `src/model_source/stub.rs` | Fixture text, **no network** (e.g. short-classify → `billing`) |
| **CursorAgentSdkSource** | `src/model_source/cursor_agent.rs` | Node sidecar `scripts/cursor_agent_complete.mjs` → `@cursor/sdk`: `Agent.create({ apiKey, model:{id}, local:{cwd} })` + send/wait. Auth: **`CURSOR_API_KEY` only**. |

Tier → Cursor model ids (placeholders until `Cursor.models.list`): [`config/model-map.toml`](../config/model-map.toml).

Future OpenAI / direct providers: new `ModelSource` impl under `src/model_source/`; do not rewrite the router.

## Env (no secrets in repo)

| Variable | Purpose |
| --- | --- |
| `CURSOR_API_KEY` | Cursor Agent SDK / sidecar |
| `CURSOR_AGENT_HELPER` | Override helper script path |
| `CURSOR_MODEL_MAP` | Override `config/model-map.toml` path |
| `TYPESAFE_API_KEY` | System One Choice / Score (separate) |

## Morning demo

```bash
cargo run -- demo --session examples/a_e/b_short_classify.json --stub
```

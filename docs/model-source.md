# ModelSource (lab design — updated 2026-09-17)

> Pluggable execution **after** System One Choice.
> Pipeline: `list_models → Choice → ModelSource.complete → return decision+output → async Score`.
> Score never blocks the hot path (`docs/score-feedback-loop.md`).

## Patrick lock (2026-09-17)

1. **Choice allowlist** MUST come from `ModelSource.list_models()` — source model ids only
   (e.g. `composer-2.5`). Do **not** maintain a separate `gpt-4o-mini`-style catalog that
   then remaps to Cursor ids.
2. **Token prices** live on the ModelSource / `ModelInfo` (baked in from provider docs).
   Choice uses those prices for cost / cache notes. No separate cost TOML required for v0.
3. **`RouterDecision.chosen_model` IS the ModelSource id** passed to `complete` — no
   post-Choice remap.
4. Score stays async / off the hot path.
5. No secrets in repo.

## Pipeline

```text
ModelSource.list_models()  →  allowlist (+ $/M tokens, cache rates)
        → Choice route (Typesafe) among those ids only
        → ModelSource.complete(chosen_model_id, …)  → model_output
        → return decision + output (hot path)
        → async Score → RouteOutcome
```

## Trait

```text
trait ModelSource {
  name() -> string
  list_models() -> Result<Vec<ModelInfo>>
  complete(req: CompleteRequest) -> Result<CompleteResponse>
}

ModelInfo {
  id,                         // e.g. "composer-2.5"
  label?,
  price_input_per_mtok,       // USD / 1M tokens
  price_output_per_mtok,
  price_cache_read_per_mtok?,
  price_cache_write_per_mtok?,
  tier_hint?                  // optional T-small|T-mid|T-frontier; not a parallel catalog
}

CompleteRequest { model_id, prompt, cwd?, runtime, … }
CompleteResponse { model_output, source, model_id, run_id?, usage?, raw_meta? }
```

Rust: [`src/model_source/mod.rs`](../src/model_source/mod.rs). Sync `fn` for v0.
`CompleteRequest.prompt` is the **full session** (`[system]` + `[user]` + …) so classify
demos keep the label instruction.

## Implementations

| Impl | Path | Behavior |
| --- | --- | --- |
| **StubModelSource** | `src/model_source/stub.rs` | Fixture text, **no network**; Cursor-style ids + **obvious fixture** $/MTok |
| **CursorAgentSdkSource** | `src/model_source/cursor_agent.rs` | Node sidecar → `@cursor/sdk` **`Agent.prompt`**; auth: **`CURSOR_API_KEY` only** |

### Cursor pricing snapshot (USD / 1M tokens)

Source: <https://cursor.com/docs/models-and-pricing> (fetched **2026-09-17**). Baked into
`CursorAgentSdkSource::list_models` / `ModelInfo`.

| id | input | cache read | output |
| --- | ---: | ---: | ---: |
| composer-2.5 | 0.50 | 0.20 | 2.50 |
| composer-2.5-fast | 3.00 | 0.50 | 15.00 |
| grok-4.6 | 2.00 | 0.50 | 6.00 |
| grok-4.6-fast | 4.00 | 1.00 | 12.00 |
| grok-4.5 | 2.00 | 0.50 | 6.00 |
| grok-4.5-fast | 4.00 | 1.00 | 18.00 |

**Caching / cost notes:** strong prefix reuse → favor low **cache read** (composer-2.5 @
$0.20/M) over fast tiers. Short-classify / low ambiguity → cheapest capable non-fast id
unless latency SLA forces fast.

Legacy `config/model-map.toml` tier→Cursor remap is **not** the source of truth (optional
`tier_hint` only). Optional `config/models.example.toml` is for route-only / paper demos
without a ModelSource.

## Env (no secrets in repo)

| Variable | Purpose |
| --- | --- |
| `CURSOR_API_KEY` | Cursor Agent SDK / sidecar |
| `CURSOR_AGENT_HELPER` | Override helper script path |
| `TYPESAFE_API_KEY` | System One Choice / Score (separate) |

## CLI

```bash
# Morning demo (stub ModelSource; allowlist from list_models)
cargo run -- demo --session examples/a_e/b_short_classify.json --model-source stub

# Execute with stub (default when --execute)
cargo run -- route --session examples/a_e/b_short_classify.json --stub --execute --verbose

# Live Cursor ModelSource (requires CURSOR_API_KEY + npm i @cursor/sdk)
cargo run -- route --session examples/a_e/b_short_classify.json --model-source cursor --execute
```

A–E fixtures may omit `allowlist`; when a ModelSource is selected the CLI fills it from
`list_models()`.

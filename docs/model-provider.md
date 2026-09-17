# ModelProvider (lab design — updated 2026-09-17)

> Pluggable execution **after** System One Choice.
> Pipeline: `list_models → Choice → ModelProvider.complete → return decision+output → async Score`.
> Score never blocks the hot path (`docs/score-feedback-loop.md`).
>
> **Rename note:** previously `ModelSource` (and Databricks docs’ “model service”).
> Prefer **`ModelProvider`** everywhere.

## Patrick lock (2026-09-17)

1. **Choice allowlist** MUST come from `ModelProvider.list_models()` — provider model ids only
   (e.g. `composer-2.5`, `system.ai.claude-sonnet-4-5`). Do **not** maintain a separate
   `gpt-4o-mini`-style catalog that then remaps to provider ids.
2. **Token prices** live on the ModelProvider / `ModelInfo` (baked in from provider docs).
   Choice uses those prices for cost / cache notes. No separate cost TOML required for v0.
3. **`RouterDecision.chosen_model` IS the ModelProvider id** passed to `complete` — no
   post-Choice remap.
4. Score stays async / off the hot path.
5. No secrets in repo.

## Pipeline

```text
ModelProvider.list_models()  →  allowlist (+ $/M tokens, cache rates)
        → Choice route (Typesafe) among those ids only
        → ModelProvider.complete(chosen_model_id, …)  → model_output
        → return decision + output (hot path)
        → async Score → RouteOutcome
```

## Trait

```text
trait ModelProvider {
  name() -> string
  list_models() -> Result<Vec<ModelInfo>>
  complete(req: CompleteRequest) -> Result<CompleteResponse>
}

ModelInfo {
  id,                         // e.g. "composer-2.5" or "system.ai.claude-sonnet-4-5"
  label?,
  price_input_per_mtok,       // USD / 1M tokens
  price_output_per_mtok,
  price_cache_read_per_mtok?,
  price_cache_write_per_mtok?,
  tier_hint?                  // optional T-small|T-mid|T-frontier; not a parallel catalog
}

CompleteRequest { model_id, prompt, messages?, cwd?, runtime, … }
CompleteResponse { model_output, source, model_id, run_id?, usage?, raw_meta? }
```

Rust: [`src/model_provider/mod.rs`](../src/model_provider/mod.rs). Sync `fn` for v0.
`CompleteRequest.prompt` is the **full session** (`[system]` + `[user]` + …) so classify
demos keep the label instruction. Chat-style providers (Databricks) prefer `messages`.

## Implementations

| Impl | Path | Behavior |
| --- | --- | --- |
| **StubModelProvider** | `src/model_provider/stub.rs` | Fixture text, **no network**; Cursor-style ids + **obvious fixture** $/MTok |
| **CursorAgentSdkSource** | `src/model_provider/cursor_agent.rs` | Node sidecar → `@cursor/sdk` **`Agent.prompt`**; auth: **`CURSOR_API_KEY` only** |
| **DatabricksAiGatewayProvider** | `src/model_provider/databricks_ai_gateway.rs` | Unity AI Gateway OpenAI-compatible chat completions; auth: **`DATABRICKS_HOST` + `DATABRICKS_TOKEN`** |

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

### Databricks Unity AI Gateway

| Path | When | Endpoint |
| --- | --- | --- |
| **Model service** (default) | Databricks-hosted / unified FQNs | `{HOST}/ai-gateway/mlflow/v1/chat/completions` |
| **Model provider service** | External provider via UC service | `{HOST}/ai-gateway/openai/v1/chat/completions` + header `Databricks-Model-Provider-Service` |

Default catalog ids (illustrative $/MTok for Choice notes):

| id | tier_hint |
| --- | --- |
| `system.ai.databricks-gpt-oss-120b` | T-small |
| `system.ai.claude-sonnet-4-5` | T-mid |
| `system.ai.claude-opus-4-6` | T-frontier |

Override with `DATABRICKS_AI_GATEWAY_MODELS` (JSON `ModelInfo[]` or comma-separated FQNs).

Docs: [query model services](https://docs.databricks.com/aws/en/ai-gateway/query-model-services),
[query model provider services](https://docs.databricks.com/aws/en/ai-gateway/query-model-provider-services).

Legacy `config/model-map.toml` tier→Cursor remap is **not** the source of truth (optional
`tier_hint` only). Optional `config/models.example.toml` is for route-only / paper demos
without a ModelProvider.

## Env (no secrets in repo)

| Variable | Purpose |
| --- | --- |
| `CURSOR_API_KEY` | Cursor Agent SDK / sidecar |
| `CURSOR_AGENT_HELPER` | Override helper script path |
| `DATABRICKS_HOST` | Workspace URL (`https://adb-….azuredatabricks.net`) |
| `DATABRICKS_TOKEN` | PAT / OAuth token for AI Gateway |
| `DATABRICKS_AI_GATEWAY_MODELS` | Optional model catalog override |
| `DATABRICKS_MODEL_PROVIDER_SERVICE` | Optional UC provider-service name (switches to OpenAI path) |
| `TYPESAFE_API_KEY` | System One Choice / Score (separate) |

## CLI

```bash
# Morning demo (stub ModelProvider; allowlist from list_models)
cargo run -- demo --session examples/a_e/b_short_classify.json --model-provider stub

# Execute with stub (default when --execute)
cargo run -- route --session examples/a_e/b_short_classify.json --stub --execute --verbose

# Live Cursor ModelProvider (requires CURSOR_API_KEY + npm i @cursor/sdk)
cargo run -- route --session examples/a_e/b_short_classify.json --model-provider cursor --execute

# Live Databricks AI Gateway (requires DATABRICKS_HOST + DATABRICKS_TOKEN)
cargo run -- route --session examples/a_e/b_short_classify.json --model-provider databricks --execute
```

`--model-source` remains a visible alias of `--model-provider`.

A–E fixtures may omit `allowlist`; when a ModelProvider is selected the CLI fills it from
`list_models()`.

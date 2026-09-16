# typesafe-llm-router

Very low-latency **Rust library + CLI** that decides which LLM a session should use next, using [TypeSafe System One](https://docs.typesafe.ai/) (Jev) as a *decision* API — not a text LLM.

**R&D only** — not production. Sister seat “Typesafe Router” owns runtime fixtures; this crate is the LLM Router Dev scaffold.

Local work (allowlist filtering, cost/cache enrichment, prompt packing) is cheap. The only material latency is the network round-trip to TypeSafe.

## Docs

- [Decision rules](docs/decision-rules.md) — paper-routing guide (task signals, scoring dimensions, required decision fields, worked examples A–E). **Policy / R&D**; concrete model ids and rates live in `config/models.example.toml`.

## I/O

### Input — `RouterRequest`

| Field | Meaning |
| --- | --- |
| `session` | Full conversation / transcript so far (`role` + `content`) |
| `current_model` | Model already chosen for this session, if any |
| `allowlist` | Caller-defined set of model ids that may be selected |
| Optional signals | `task_class`, length/complexity, `tools_required`, `latency_mode`, `prefix_reuse` (+ `prefix_tokens_est`) — passed through to System One state (no invented classifiers) |

See [`examples/a_e/`](examples/a_e/) (decision-rules Examples A–E) and [`examples/sample_session.json`](examples/sample_session.json).

### Enrichment — cost / cache catalog

Before calling TypeSafe, each allowlisted id is joined with a per-model profile from:

- built-in `ModelCatalog::demo()`, or
- a TOML file (`config/models.example.toml`)

Profiles include `id`, `tier` (`T-small` \| `T-mid` \| `T-frontier`), `tool_capable`, input/output cost (or band), and `cache_eligible` / provider cache hints.

### Output — `RouterDecision`

First-class JSON fields aligned with [decision-rules §5](docs/decision-rules.md):

```json
{
  "chosen_model": "gpt-4o-mini",
  "chosen_tier": "T-small",
  "primary_reason": "continue_current",
  "alternatives_considered": [
    { "model_or_tier": "claude-sonnet-4", "why_rejected": "higher cost; quality margin small" }
  ],
  "cache_hypothesis": {
    "strength": "strong",
    "rationale": "stable system prefix; continuing preserves warm cache"
  },
  "rough_cost_note": "~1.5K in / 0.4K out @ T-small band; expect prefix cache after warm-up (placeholder)",
  "confidence": 0.65,
  "open_risk": null,
  "model": "gpt-4o-mini",
  "why": { "primary": "continue_current", "summary": "..." }
}
```

`model` / `why` remain for backward compatibility; prefer the first-class fields above.

## How it talks to TypeSafe

Verified against the public HTTP docs ([API reference](https://docs.typesafe.ai/api)):

```http
POST https://api.typesafe.ai/v1/systemone
Authorization: Bearer $TYPESAFE_API_KEY
Content-Type: application/json
```

The packer sends session + optional signals + enriched candidates as `state`, plus Choice questions `route_to` and `primary_reason`. Default System One model: `jev-latest`.

## Quick start

```bash
# Offline stub (default for lab smoke — no API key)
cargo run -- route --session examples/a_e/a_routine_code_fix.json --stub

# All paper fixtures
for f in examples/a_e/*.json; do
  echo "=== $f ==="
  cargo run --quiet -- route --session "$f" --stub
done

# Live TypeSafe call
export TYPESAFE_API_KEY=...   # from https://console.typesafe.ai — never commit
cargo run -- route --session examples/a_e/b_short_classify.json --catalog config/models.example.toml
```

### Environment variables

| Variable | Required | Description |
| --- | --- | --- |
| `TYPESAFE_API_KEY` | for live calls | Bearer token for `api.typesafe.ai` |
| `TYPESAFE_BASE_URL` | no | Override host (default `https://api.typesafe.ai`) |

Catalog path: `--catalog config/models.example.toml` (optional; demo catalog otherwise). See [`.env.example`](.env.example). **Do not commit secrets.**

## Layout

```
src/                 library + CLI
config/models.example.toml
docs/decision-rules.md
examples/a_e/        Examples A–E session fixtures
examples/sample_session.json
```

## What to try next (Typesafe Router)

1. Stub smoke on A–E: `cargo run -- route --session examples/a_e/<fixture>.json --stub` and confirm JSON includes `chosen_model`, `alternatives_considered`, `cache_hypothesis`, `rough_cost_note`, `primary_reason`.
2. Live: set `TYPESAFE_API_KEY`, drop `--stub`, compare against paper decisions in `docs/decision-rules.md`.
3. Replace placeholder rates/tiers in the catalog when real pricing cards land.

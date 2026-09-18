# Demos (copy-paste)

Offline stub works with no keys. Cursor and Databricks live paths need local auth —
never commit secrets. Deeper design: [model-provider](model-provider.md),
[score-feedback-loop](score-feedback-loop.md).

## 1. Stub morning recording (offline)

No network. Score stays pending on the hot path (drain later if you want).

```bash
cargo run -- demo --session examples/a_e/b_short_classify.json --model-provider stub
```

Equivalent route form:

```bash
cargo run -- route --session examples/a_e/b_short_classify.json --model-provider stub --execute --verbose
```

## 2. Cursor live (`CURSOR_API_KEY` required)

Install the Node sidecar dependency once (pinned in root `package.json`):

```bash
npm i
# or: npm i @cursor/sdk
export CURSOR_API_KEY=…    # never commit; see .env.example
cargo run -- route --session examples/a_e/b_short_classify.json --model-provider cursor --execute
```

Without `CURSOR_API_KEY`, the Cursor provider fails fast with a missing-key error.
Sidecar: `scripts/cursor_agent_complete.mjs` (ESM `import fs` — do not switch to `require`).

## 3. Databricks (offline / mock note)

**Live** Databricks needs CLI login (not covered as an offline demo):

```bash
databricks auth login --host https://<workspace-url>
# optional: export DATABRICKS_CONFIG_PROFILE=lab
cargo run -- route --session examples/a_e/b_short_classify.json --model-provider databricks --execute
```

**Offline / CI:** use stub (`--model-provider stub`) instead. Integration coverage for the
Databricks provider uses an in-process mock transport (no live workspace) — see
`tests/integration_model_provider.rs` and the Databricks section of
[model-provider.md](model-provider.md).

## Async Score (optional drain)

After any demo/route with execute, outcomes enqueue under `.router-data/score-queue/`:

```bash
cargo run -- drain-score-queue
```

`--model-source` is a documented alias of `--model-provider` only.

# Live Cursor ModelSource demo

Terminal demo of the typesafe LLM router with **live** Cursor ModelSource
(`Agent.prompt` via `scripts/cursor_agent_complete.mjs` / `@cursor/sdk`).

Score stays **async** (pending/enqueued; not on the hot path).

## Auth status (this cloud-agent run)

**`CURSOR_API_KEY` was not set** in the environment for this run.

Per the recording brief: do **not** invent secrets and do **not** fake a live
execution. No `.mp4` is checked in here until a real Cursor Agent SDK call
succeeds.

Verified on `main` (post PR #4):

```text
cargo run -- demo --session examples/a_e/b_short_classify.json --model-source cursor
→ ModelSource.list_models (cursor-agent) → Choice allowlist + $/MTok
→ ModelSource.complete (cursor-agent)
error: model source error: missing or empty CURSOR_API_KEY
```

To unblock a live recording: add `CURSOR_API_KEY` to the Cloud Agent
environment secrets (or export it locally), then re-run the command below.
Never commit the key; never put it in logs/video.

## Exact command (when key is present)

```bash
# one-time sidecar dep
npm i @cursor/sdk

cargo run -- demo --session examples/a_e/b_short_classify.json --model-source cursor
```

Equivalent verbose route form:

```bash
cargo run -- route --session examples/a_e/b_short_classify.json \
  --model-source cursor --execute --verbose
```

Helper (same CLI; fails fast if `CURSOR_API_KEY` is missing):

```bash
./recordings/run_live_cursor_demo.sh
```

## Expected captured fields (live run)

| Field | Expected |
| --- | --- |
| `selected_model` | a Cursor source id from `ModelSource.list_models` (e.g. `composer-2.5`) |
| `model_output` | short classify label (e.g. `billing`) |
| process path | `ModelSource = cursor-agent` → Node sidecar → `@cursor/sdk` `Agent.prompt` |
| `scores_status` | `Pending` (async; not awaited on hot path) |

Beats to show in the video:

1. **INPUT** — session fixture (`examples/a_e/b_short_classify.json`)
2. **PROCESS** — `list_models` (cursor-agent) → Choice → `ModelSource.complete` (cursor-agent / Agent.prompt) → score pending
3. **OUTPUT** — `selected_model` + `model_output`

## Artifacts

| File | Notes |
| --- | --- |
| *(none yet)* | Live `.mp4` omitted — `CURSOR_API_KEY` missing in this environment |
| [`run_live_cursor_demo.sh`](run_live_cursor_demo.sh) | Repro helper for a future live capture |

## Notes

- No secrets in repo, logs, or video.
- Stub morning demo (offline) remains:  
  `cargo run -- demo --session examples/a_e/b_short_classify.json --model-source stub`
- Draft PR only — do not merge without review.

# Morning demo recordings

Offline, deterministic demo of the typesafe LLM router with **stub** ModelSource
(Patrick lock: Choice allowlist = `ModelSource.list_models()`, no catalog remap).

## Exact command

```bash
cargo run -- demo --session examples/a_e/b_short_classify.json --model-source stub
```

Helper (same CLI, uses the already-built binary for clean timing):

```bash
./recordings/run_morning_demo.sh
```

## Captured result (this recording)

| Field | Value |
| --- | --- |
| `selected_model` | **`composer-2.5`** (ModelSource id — not `gpt-4o-mini`) |
| `model_output` | **`billing`** |
| Choice allowlist | from **`ModelSource.list_models`** (Patrick lock) |

Beats shown in the video:

1. **INPUT** — session fixture (`examples/a_e/b_short_classify.json`)
2. **PROCESS** — `list_models` → Choice → `ModelSource.complete` (stub) → score pending/async
3. **OUTPUT** — `selected_model: composer-2.5` + `model_output: billing`

## Artifacts

| File | Notes |
| --- | --- |
| [`morning_demo_stub.mp4`](morning_demo_stub.mp4) | Preferred — terminal screen capture (~43s) |
| [`morning_demo_stub.cast`](morning_demo_stub.cast) | asciinema v2 cast (replay: `asciinema play …`) |
| [`morning_demo_stub.gif`](morning_demo_stub.gif) | Rendered from the cast via `agg` |

Also uploaded for operator download alongside the cloud-agent run:
`/opt/cursor/artifacts/morning_demo_stub_modelsource.mp4`

## Notes

- No secrets / API keys in the recording (stub only).
- Optional live clip with `--model-source cursor` needs `CURSOR_API_KEY`; not recorded here.
- Branch only — do not merge without CoS.

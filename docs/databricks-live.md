# Databricks live smoke (lab)

Short checklist for running the Unity AI Gateway **ModelProvider** against a real
workspace. Full copy-paste steps: [demos.md §3](demos.md#3-databricks-live-cli-checklist).

**Security:** never commit the workspace host, tokens, or `.databrickscfg`. Docs and
scripts use placeholders only (`https://<workspace-url>` or
`https://adb-xxxxxxxxxxxx.azuredatabricks.net`). Live login is lab-side — not CI.

## Quick path

1. Install the [Databricks CLI](https://docs.databricks.com/aws/en/dev-tools/cli/install).
2. Log in (browser U2M; do this on your lab machine, not in CI):

   ```bash
   databricks auth login --host https://<workspace-url>
   # optional named profile:
   # databricks auth login --host https://<workspace-url> --profile lab
   # export DATABRICKS_CONFIG_PROFILE=lab
   ```

3. Sanity (never paste token output into git / chat / PR):

   ```bash
   databricks auth env          # expect nonempty DATABRICKS_HOST
   databricks auth token        # proves token refresh works — do not commit output
   ```

4. Smoke (fixture uses a Databricks catalog `current_model`):

   ```bash
   bash scripts/smoke_databricks.sh
   # or:
   cargo run -- demo \
     --session examples/a_e/b_short_classify_databricks.json \
     --model-provider databricks
   ```

Expected OUTPUT beats: **`selected_model`** + **`model_output`**. Score stays
**pending / async** (not on the hot path).

## Offline / CI

Keep CI on stub only (`--model-provider stub`). Mock Databricks CLI/gateway scripts
are **not** on `main`; for offline demos use stub. See [demos.md](demos.md) and
[model-provider.md](model-provider.md).

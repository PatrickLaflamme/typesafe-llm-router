#!/usr/bin/env node
/**
 * Cursor Agent sidecar for CursorAgentSdkSource (Rust).
 *
 * JSON stdin → { api_key, chosen_model, prompt, cwd?, runtime? }
 * JSON stdout → { model_output, run_id?, input_tokens?, output_tokens?, raw_meta?, error? }
 *
 * Uses Agent.prompt (one-shot), not a broken Agent.create shape.
 * `prompt` should be the full session (system + user) so classify demos
 * keep the label instruction.
 *
 * Requires: npm i @cursor/sdk  (and CURSOR_API_KEY in the environment / payload)
 * Docs: https://cursor.com/docs/api/sdk/typescript
 *
 * Never log api_key.
 */
const fs = require("fs");

async function main() {
  const raw = fs.readFileSync(0, "utf8");
  let req;
  try {
    req = JSON.parse(raw);
  } catch (e) {
    console.log(JSON.stringify({ model_output: "", error: `invalid JSON: ${e}` }));
    process.exit(1);
  }

  const apiKey = req.api_key || process.env.CURSOR_API_KEY;
  if (!apiKey) {
    console.log(
      JSON.stringify({
        model_output: "",
        error: "missing CURSOR_API_KEY (set env or pass api_key in JSON)",
      })
    );
    process.exit(1);
  }

  let Agent;
  try {
    ({ Agent } = await import("@cursor/sdk"));
  } catch (e) {
    console.log(
      JSON.stringify({
        model_output: "",
        error: `@cursor/sdk not installed: ${e.message}. Run: npm i @cursor/sdk`,
      })
    );
    process.exit(1);
  }

  try {
    const opts = {
      apiKey,
      model: { id: req.chosen_model },
    };
    if (req.runtime === "cloud") {
      opts.cloud = true;
    } else {
      opts.local = { cwd: req.cwd || process.cwd() };
    }

    // One-shot: create + send + wait + dispose.
    const result = await Agent.prompt(req.prompt, opts);
    const model_output =
      (result && (result.result || result.text || result.message || result.output)) ||
      JSON.stringify(result ?? {});

    console.log(
      JSON.stringify({
        model_output: String(model_output),
        run_id: result?.id || result?.runId || null,
        raw_meta: {
          note: "Agent.prompt(message, { apiKey, model:{id}, local:{cwd} })",
          helper: "cursor_agent_complete.mjs",
          status: result?.status ?? null,
          durationMs: result?.durationMs ?? null,
        },
      })
    );
  } catch (e) {
    console.log(
      JSON.stringify({
        model_output: "",
        error: String(e && e.message ? e.message : e),
      })
    );
    process.exit(1);
  }
}

main();

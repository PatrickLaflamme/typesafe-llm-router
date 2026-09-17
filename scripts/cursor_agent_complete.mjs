#!/usr/bin/env node
/**
 * Cursor Agent sidecar for CursorAgentSdkSource (Rust).
 *
 * JSON stdin → { api_key, chosen_model, prompt, cwd?, runtime? }
 * JSON stdout → { model_output, run_id?, input_tokens?, output_tokens?, raw_meta?, error? }
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

    let agent = await Agent.create(opts);
    // Design: Agent.create({ apiKey, model:{id}, local:{cwd} }) + send/wait
    const run = await agent.send({ message: req.prompt });
    if (run && typeof run.wait === "function") {
      await run.wait();
    }
    // Best-effort extraction — SDK shapes evolve; keep raw_meta for debugging.
    const model_output =
      (run && (run.result || run.text || run.message || run.output)) ||
      JSON.stringify(run ?? {});

    console.log(
      JSON.stringify({
        model_output: String(model_output),
        run_id: run?.id || run?.runId || null,
        raw_meta: {
          note: "Agent.create({ apiKey, model:{id}, local:{cwd} }) + send/wait",
          helper: "cursor_agent_complete.mjs",
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

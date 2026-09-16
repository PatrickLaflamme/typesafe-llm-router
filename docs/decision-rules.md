# Decision rules — Typesafe Router (experimental)

> **Status:** R&D / paper-routing only. Not production. Not wired to live Typesafe.ai credentials or pricing.
>
> **Seat:** Typesafe Router (runtime experiments) — inspect prompt → choose LLM → weigh caching + cost.
> **Sister seat:** LLM Router Dev owns code, architecture, and evals. This doc is the human/agent playbook they can later encode.

Use this document to **paper-route** prompts: given an incoming prompt (and optional metadata), walk the procedure below and record a routing decision with the required output fields.

---

## 1. Goal

Given a prompt (plus optional context about the caller), select which model tier to delegate to such that:

1. **Capability fit** is adequate for the task (do not under-route hard work or over-route trivial work without reason).
2. **Caching** is considered explicitly — prefer routes where a stable system/prefix is likely to hit a prompt cache.
3. **Cost** is minimized when quality margin between tiers is small or when the task is cheap to verify.
4. **Latency** is respected when the interaction is interactive / user-blocking.

The router always produces an **explainable** decision: chosen model, alternatives, cache hypothesis, rough cost note.

---

## 2. Placeholder model tiers

Do **not** treat the names below as live Typesafe product SKUs or prices. They are paper labels for experiments.

| Tier ID | Role | Typical use | Example cost band (placeholder) |
|---------|------|-------------|----------------------------------|
| `small` | Cheap / fast | Short classify, extract, rewrite, simple Q&A | **Low** — e.g. ~0.1–1× baseline unit |
| `mid` | Balanced | Most code edits, structured generation, moderate reasoning | **Medium** — e.g. ~3–8× baseline unit |
| `frontier` | Highest capability | Hard reasoning, ambiguous specs, novel creative, multi-step tool plans | **High** — e.g. ~15–40× baseline unit |

**Baseline unit** = fictional relative cost of `small` for a fixed token budget. Replace with measured $/1K in+out when credentials and pricing exist.

Optional future axes (not required for paper routing yet): provider-specific cache strength, tool-calling reliability, context-window headroom.

---

## 3. Input signals to inspect

Before scoring, extract (or estimate) these signals from the prompt and caller metadata.

| Signal | What to look for | Notes |
|--------|------------------|-------|
| **Task class** | `code` · `short_classify` · `long_reason` · `creative` · `tool_use` · `mixed` / `unknown` | Primary capability driver. Prefer one primary class; note secondary if mixed. |
| **Length / complexity** | Token estimate (short &lt; ~500, medium ~500–4k, long &gt; ~4k); number of constraints; need for multi-hop reasoning | Longer ≠ always frontier; long *simple* extract can stay `small`/`mid`. |
| **Need for tools** | Explicit tool/API/browser/shell requirements; multi-step agent loops | Prefer tiers known (in evals) to be reliable at tool calling once measured. |
| **Latency sensitivity** | Interactive UI vs batch/offline; SLA hints | Interactive → may prefer `small`/`mid` even if frontier is slightly better. |
| **Shared system / prefix reuse** | Stable system prompt, shared few-shots, org-wide preamble identical across many requests | Strong caching signal — prefer providers/tiers with good prompt-cache hit rates when prefixes are shared. |
| **Verifiability** | Output easy to check (unit tests, schema, rubrics) vs subjective | High verifiability → safer to under-route and retry/escalate. |
| **Failure cost** | Wrong answer cheap vs expensive (user-facing, irreversible) | High failure cost → bias toward higher capability / more alternatives noted. |

If a signal is unknown, record `unknown` and default conservatively toward `mid` unless the prompt is clearly trivial or clearly hard.

---

## 4. Scoring dimensions

Score each candidate tier on four dimensions. Use a simple 1–5 scale for paper routing (1 = poor fit, 5 = excellent). Tie-break with the ordered procedure in §5.

### 4.1 Capability fit

- Does this tier historically (or by hypothesis) handle this **task class** and **complexity**?
- Under-routing risk: incomplete code, shallow reasoning, missed tool steps.
- Over-routing risk: paying frontier rates for a label/classify that `small` would nail.

### 4.2 Caching hypothesis

State an explicit hypothesis, e.g.:

- **High hit expected** — large identical prefix across requests; same system prompt; few-shot block reused.
- **Partial / uncertain** — some shared preamble, user body varies a lot.
- **Low / miss expected** — fully unique prompts; no stable system; one-off jobs.

Routing implication: when **high hit expected**, prefer a tier/provider path that benefits from prompt caching even if per-token list price is slightly higher — amortized cost may win. When **low**, optimize raw token cost and capability only.

### 4.3 Cost

Rough paper estimate:

```
rough_cost ≈ (expected_input_tokens × in_band + expected_output_tokens × out_band)
             × cache_miss_fraction   # 1.0 if no cache; ~0.1–0.3 on strong hits (placeholder)
```

Use **cost bands** from §2, not dollar claims. Prefer cheaper tier when capability scores are within ~1 point and failure cost is low.

### 4.4 Latency

- Interactive / chat: prefer lower-latency tiers unless capability gap is large.
- Batch / eval / overnight: latency weight ≈ 0; optimize cost + quality.
- Tool-heavy loops: count **round trips**; a slightly slower but more reliable tool-caller may finish sooner wall-clock.

---

## 5. Decision procedure

Follow these steps in order. Record notes as you go.

1. **Classify the task**  
   Assign primary `task_class` and note length/complexity, tools, latency sensitivity, prefix reuse (§3).

2. **Enumerate candidates**  
   Always consider at least `{small, mid, frontier}` unless a hard constraint removes one (e.g. “must use tools” and `small` is hypothesized unreliable — still *consider* it and reject with reason).

3. **Score capability**  
   Score each candidate 1–5. Drop any candidate with capability &lt; 3 **unless** verifiability is high and an escalate-on-fail path exists.

4. **Apply caching hypothesis**  
   Write the cache hypothesis. If **high hit expected**, boost candidates on strong-cache paths (paper: note “cache-favor”) and re-estimate rough cost with a lower miss fraction.

5. **Apply cost**  
   Among remaining candidates with capability within 1 point of the best, prefer the lower cost band (after cache adjustment).

6. **Apply latency (if interactive)**  
   If latency-sensitive, demote frontier when `mid` capability ≥ 4, or demote `mid` when `small` capability ≥ 4 and failure cost is low.

7. **Choose and document**  
   Emit the required output fields (§6). If uncertain between two tiers, pick the **cheaper** when failure cost is low / verifiable; pick the **stronger** when failure cost is high or task is ambiguous.

8. **Escalate rule (paper)**  
   If the chosen tier’s output would fail a cheap check (schema, tests, rubric), re-route once to the next tier up — log as `escalation`. Do not invent automatic retries in production until evals exist.

---

## 6. Required output fields

Every routing decision **must** include:

| Field | Description |
|-------|-------------|
| `chosen_model` | Tier ID (e.g. `mid`) or future concrete model id once catalog exists |
| `alternatives_considered` | List of other tiers/models scored, each with brief reject/accept reason |
| `cache_hypothesis` | `high_hit` · `partial` · `low_miss` · `unknown` + one-sentence rationale |
| `rough_cost_note` | Placeholder band / relative estimate — **no live price claims** |
| *(recommended)* `task_class` | Primary class from §3 |
| *(recommended)* `latency_mode` | `interactive` · `batch` · `unknown` |
| *(recommended)* `rationale` | 2–4 sentences tying scores → choice |

### Example output shape (placeholder)

```yaml
chosen_model: mid
task_class: code
latency_mode: interactive
cache_hypothesis: high_hit — shared repo system prompt + coding guidelines prefix reused across requests
rough_cost_note: medium band; assume ~20% cache miss on prefix → effective cost closer to low-medium
alternatives_considered:
  - small: rejected — multi-file refactor likely needs stronger code reasoning
  - frontier: considered — better on ambiguous refactors but cost band high; margin not justified for scoped edit
rationale: >
  Scoped TypeScript refactor with tests present (high verifiability). Stable system prefix favors
  cache-aware mid tier. Interactive but not ultra-latency-critical.
```

---

## 7. Worked examples (paper scenarios)

Costs below are **labeled placeholders**, not Typesafe or vendor quotes.

### Example A — Code edit (scoped)

**Prompt sketch:** “In `auth.ts`, extract JWT validation into a helper; keep existing tests green.”

| Signal | Value |
|--------|-------|
| Task class | `code` |
| Length / complexity | Medium; localized |
| Tools | None required |
| Latency | Interactive |
| Prefix reuse | High (shared coding system prompt) |

**Decision:**

```yaml
chosen_model: mid
alternatives_considered:
  - small: weak on non-trivial refactors / edge cases in auth
  - frontier: overkill for single-file extract with tests
cache_hypothesis: high_hit — org coding system prompt stable across sessions
rough_cost_note: medium band; cache hit on system prefix → effective closer to low-medium
```

---

### Example B — Short classify

**Prompt sketch:** “Label this support ticket: billing | tech | account. Reply with one label only.”  
Body: ~80 tokens of ticket text. Same classifier system prompt on every call.

| Signal | Value |
|--------|-------|
| Task class | `short_classify` |
| Length / complexity | Short; low |
| Tools | None |
| Latency | Batch or near-real-time OK |
| Prefix reuse | High |

**Decision:**

```yaml
chosen_model: small
alternatives_considered:
  - mid: unnecessary quality margin for closed label set
  - frontier: rejected — cost band high for trivial classify
cache_hypothesis: high_hit — identical classifier system + label schema every request
rough_cost_note: low band; strong cache → amortized cost near floor of low band
```

---

### Example C — Long reasoning

**Prompt sketch:** “Compare three distributed consensus approaches for our multi-region queue; recommend one given constraints X/Y/Z (detailed annex, ~6k tokens).”

| Signal | Value |
|--------|-------|
| Task class | `long_reason` |
| Length / complexity | Long; high ambiguity |
| Tools | None |
| Latency | Batch |
| Prefix reuse | Low (one-off design review) |
| Failure cost | High (architecture choice) |

**Decision:**

```yaml
chosen_model: frontier
alternatives_considered:
  - mid: possible draft, but easy to miss constraint interactions
  - small: rejected — insufficient depth for multi-constraint trade study
cache_hypothesis: low_miss — unique annex; little reusable prefix
rough_cost_note: high band; full miss assumed; accept cost for failure-cost reasons
```

---

### Example D — Creative copy

**Prompt sketch:** “Write three witty hero taglines for a camping gear brand; tone playful, not corporate.”

| Signal | Value |
|--------|-------|
| Task class | `creative` |
| Length / complexity | Short output; subjective quality |
| Tools | None |
| Latency | Interactive |
| Prefix reuse | Partial (brand voice sheet sometimes shared) |
| Verifiability | Low (subjective) |

**Decision:**

```yaml
chosen_model: mid
alternatives_considered:
  - small: often flat/generic on brand voice
  - frontier: better prose occasional win; cost band high for three short lines — try mid first
cache_hypothesis: partial — brand voice sheet reused when present; user product blurb varies
rough_cost_note: medium band; if voice sheet cached, effective low-medium
```

*Paper note:* If mid outputs fail a human taste check twice, escalate once to `frontier` and log.

---

### Example E — Tool use / agent loop

**Prompt sketch:** “Find open SEV tickets in Linear, summarize blockers, draft a Slack update — use tools; don’t invent issue IDs.”

| Signal | Value |
|--------|-------|
| Task class | `tool_use` |
| Length / complexity | Medium; multi-step |
| Tools | Required (search + message draft) |
| Latency | Interactive |
| Prefix reuse | High (agent system + tool schemas) |
| Failure cost | Medium-high (wrong IDs bad) |

**Decision:**

```yaml
chosen_model: mid   # or frontier if evals show mid tool-calling unreliable
alternatives_considered:
  - small: rejected — multi-step tool planning + schema adherence weak (hypothesis)
  - frontier: keep as escalation if mid drops tool args or hallucinates IDs
cache_hypothesis: high_hit — agent system prompt + tool JSON schemas stable
rough_cost_note: medium band per call; multiple round trips → watch cumulative cost; cache helps on system/tools each turn
```

---

## 8. Open questions / experiment backlog

Track these for LLM Router Dev evals; do not block paper routing on answers.

1. **Catalog binding** — Map `small` / `mid` / `frontier` to concrete Typesafe (or upstream) model IDs once credentials exist.
2. **Measured pricing** — Replace placeholder cost bands with real $/1K in+out and cache discount factors; never commit secrets or live key material to the repo.
3. **Cache hit telemetry** — What prefix length / stability predicts hits? Per-provider differences?
4. **Task classifier** — Rules vs small model vs embeddings for `task_class`; error rates that cause mis-routes.
5. **Tool-calling reliability matrix** — Per-tier success rate on tool schema adherence and multi-step loops.
6. **Escalate-on-fail policy** — Cheap verifiers (JSON schema, tests, regex) that justify starting cheap and upgrading once.
7. **Latency SLOs** — When does interactive UX force `small` even if `mid` is only slightly better?
8. **Mixed tasks** — Decomposition (classify then reason) vs single frontier call — cost/quality tradeoffs.
9. **Corpus** — First labeled prompt set for offline paper-routing agreement between humans/agents.
10. **Calibration loop** — Compare paper decisions vs post-hoc quality scores; adjust capability thresholds in §5.

---

## 9. Out of scope (this doc)

- Production deployment, auth, or endpoint configuration
- Real API keys, `.env` files, or credential placeholders that look real
- Claiming live Typesafe pricing or SLA guarantees
- Implementing the router binary (sister seat: LLM Router Dev)

When in doubt: **document the hypothesis, choose a tier, record the four required fields.**

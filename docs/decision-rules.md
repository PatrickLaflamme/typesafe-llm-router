# Typesafe Router — Decision Rules (Experimental)

> **Status:** R&D paper-routing guide only. Not production policy.  
> **Audience:** Typesafe Router (runtime seat) and LLM Router Dev (code / evals).  
> **Do not** treat placeholder cost bands as live pricing. No credentials or endpoints that require auth appear here.

This document is the first in-repo bootstrap of routing judgment: given a prompt, choose a model tier while making **caching** and **cost** reasoning explicit. Sister seat “LLM Router Dev” owns implementation and eval harnesses; this file stays documentation-first until those land.

---

## 1. Goal

Map **prompt → model choice** with an auditable trail:

1. Inspect the prompt (and any stable system / tool preamble).
2. Score candidate tiers on capability fit, caching hypothesis, cost, and latency.
3. Emit a routing decision with required output fields (see §5).

Prefer the cheapest tier that is expected to meet quality needs when the quality margin is small. Prefer tiers (or providers) with stronger prompt-cache hit potential when prefixes are shared and stable.

---

## 2. Input signals to inspect

Before scoring, extract these signals from the request (prompt + optional metadata):

| Signal | What to look for | Why it matters |
| --- | --- | --- |
| **Task class** | Code, short classify, long reasoning, creative / generative, tool-use / agentic | Anchors capability fit |
| **Length / complexity** | Token estimate, multi-step structure, nested constraints, ambiguity | Cheap models fail on dense or long-horizon work |
| **Need for tools** | Function/tool schemas, browse/code-exec, multi-turn tool loops | May require tool-capable mid or frontier tiers |
| **Latency sensitivity** | Interactive chat vs batch / offline | Only elevate latency weight when the ask is interactive |
| **Shared system / prefix reuse** | Stable system prompt, shared RAG preamble, repeated few-shot block | Drives caching hypothesis and may outweigh small $/token gaps |

Optional metadata (when present): expected output length, hard latency SLA, “must not invent facts,” prior route outcomes for the same prefix hash.

---

## 3. Scoring dimensions

Score each candidate tier (see placeholder tiers below) roughly on a 1–5 scale, or use relative ranks. Dimensions are not equal weight; apply the ordered procedure in §4.

### 3.1 Capability fit

Does this tier reliably handle the task class and complexity?

- **High** when the task matches the tier’s known strengths and failure modes are unlikely.
- **Low** when the task needs deep reasoning, careful tool use, or high instruction adherence and the tier is known to skip steps or hallucinate structure.

### 3.2 Caching hypothesis

Will this route likely benefit from prompt caching (provider-side prefix cache or equivalent)?

- **Strong hit** — large stable prefix reused across many requests; same model/provider family as prior hits.
- **Weak / none** — unique user-only prompts, unstable prefixes, or first-seen system text.
- Prefer routes where a **strong hit** is plausible even if base $/token is slightly higher—**if** expected reused tokens × hit rate outweigh the delta. State the hypothesis explicitly; do not invent measured hit rates.

### 3.3 Cost

Rough cost using **example / placeholder bands only** (not live Typesafe or vendor quotes):

| Placeholder tier | Label | Example input band | Example output band |
| --- | --- | --- | --- |
| `T-small` | Small / cheap | `$0.05–0.20` / 1M in | `$0.20–0.80` / 1M out |
| `T-mid` | Mid | `$0.50–2.00` / 1M in | `$1.50–8.00` / 1M out |
| `T-frontier` | Frontier | `$3.00–15.00` / 1M in | `$12.00–60.00` / 1M out |

Cost note = band × rough expected tokens (in + out), adjusted qualitatively for expected cache hits (e.g. “prefix ~2K tokens may be cached after warm-up”).

### 3.4 Latency

Weight this dimension **only** when the request is interactive or has an explicit latency budget. Otherwise note “batch / no SLA” and do not reject a slower-but-cheaper-or-better-cache route.

---

## 4. Decision procedure

Follow these steps in order. Skip only with an explicit reason in the decision record.

1. **Classify the task**  
   Assign primary task class (code | short-classify | long-reason | creative | tool-use). Note secondary tags if mixed (e.g. code + tool-use).

2. **Estimate size and difficulty**  
   Short / medium / long; simple / medium / hard. Flag if output must be structured (JSON, schema, diff).

3. **Detect tools and latency**  
   Tools required? Interactive? Record yes/no and any SLA.

4. **Detect cacheable prefix**  
   Identify stable system / shared preamble. Estimate reusable prefix tokens (order-of-magnitude). Hypothesis: strong | weak | none.

5. **Build candidate set**  
   Always consider at least two tiers when capability allows (e.g. `T-small` vs `T-mid`). Include `T-frontier` only when lower tiers are likely to fail or when quality risk is high.

6. **Score candidates**  
   Capability fit → caching hypothesis → cost → latency (latency last, and only if interactive).

7. **Choose and justify**  
   Default: lowest cost among candidates with acceptable capability fit. Upgrade for: hard reasoning, fragile tool loops, high-stakes correctness, or when a mid/frontier route has a clearly stronger cache story for a large shared prefix.

8. **Emit required output fields** (§5). Do not route silently.

### Default tier heuristics (paper only)

| Situation | Default lean |
| --- | --- |
| Short classify / extract, low ambiguity | `T-small` |
| Routine code edit / explain, moderate length | `T-mid` (try `T-small` if tiny and local) |
| Long multi-step reasoning, proofs, planning | `T-frontier` or strong `T-mid` |
| Creative writing with style control | `T-mid`; frontier if brand-critical |
| Tool-use / agent loops | `T-mid` minimum; frontier if many tools or brittle schemas |
| Large shared system prompt, high QPS | Prefer tier with best cache story among capable options |

---

## 5. Required output fields

Every routing decision (paper or future runtime) must include:

| Field | Description |
| --- | --- |
| **chosen_model** | Placeholder tier id + optional human label (e.g. `T-mid` — “mid”). Never a real API key. |
| **alternatives_considered** | List of other tiers scored, with one-line why rejected. |
| **cache_hypothesis** | `strong` \| `weak` \| `none`, plus short rationale (prefix stability, reuse). |
| **rough_cost_note** | Placeholder band language only (e.g. “~1.5K in / 0.4K out @ T-small band; weak cache”). |

Optional but useful: task_class, latency_mode (`interactive` \| `batch`), confidence (`high` \| `medium` \| `low`), open_risk (“may under-reason on edge cases”).

### Example decision record shape

```text
chosen_model: T-mid (mid)
alternatives_considered:
  - T-small: rejected — tool schema adherence risk
  - T-frontier: rejected — quality margin small vs mid; cost 5–10× higher (placeholder bands)
cache_hypothesis: strong — shared 1.8K-token system + tool preamble reused across session
rough_cost_note: ~2.5K in / 0.8K out @ T-mid band; expect prefix cache after warm-up (placeholder)
```

---

## 6. Worked examples (paper scenarios)

All models and costs below are **placeholders**. No live pricing or authenticated endpoints.

### Example A — Routine code fix

**Prompt sketch:** “In this 80-line TypeScript function, fix the off-by-one in pagination; keep public API.”

| Signal | Value |
| --- | --- |
| Task class | code |
| Length / complexity | short–medium / medium |
| Tools | no |
| Latency | interactive |
| Prefix reuse | weak (one-off snippet) |

**Decision**

- **chosen_model:** `T-mid` (mid)
- **alternatives_considered:** `T-small` — may miss edge cases in pagination; `T-frontier` — overkill for localized fix
- **cache_hypothesis:** none — unique user code, no shared system block called out
- **rough_cost_note:** ~1.2K in / 0.5K out @ T-mid band (placeholder)

### Example B — Short classify

**Prompt sketch:** “Label this support ticket: billing | bug | feature | other. Reply with one label only.” + 2-sentence ticket body. Shared classifier system prompt (~400 tokens) used on every ticket.

| Signal | Value |
| --- | --- |
| Task class | short-classify |
| Length / complexity | short / simple |
| Tools | no |
| Latency | batch ok |
| Prefix reuse | strong (stable system) |

**Decision**

- **chosen_model:** `T-small` (small / cheap)
- **alternatives_considered:** `T-mid` — unnecessary for single-label task; `T-frontier` — rejected on cost
- **cache_hypothesis:** strong — identical system prefix across high volume
- **rough_cost_note:** ~0.5K in / 5 out @ T-small band; system ~400 tokens likely cached after warm-up (placeholder)

### Example C — Long reasoning

**Prompt sketch:** “Compare three distributed consensus designs for our multi-region queue; recommend one with failure-mode analysis.” No tools; offline memo.

| Signal | Value |
| --- | --- |
| Task class | long-reason |
| Length / complexity | long / hard |
| Tools | no |
| Latency | batch |
| Prefix reuse | none |

**Decision**

- **chosen_model:** `T-frontier` (frontier)
- **alternatives_considered:** `T-mid` — may under-develop failure modes; `T-small` — rejected — insufficient depth
- **cache_hypothesis:** none — one-shot analysis prompt
- **rough_cost_note:** ~2K in / 2.5K out @ T-frontier band (placeholder); batch so latency ignored

### Example D — Creative brief

**Prompt sketch:** “Write three taglines for a hiking-gear brand; dry humor; no clichés about ‘adventure’.” Interactive brainstorm.

| Signal | Value |
| --- | --- |
| Task class | creative |
| Length / complexity | short / medium (style control) |
| Tools | no |
| Latency | interactive |
| Prefix reuse | weak |

**Decision**

- **chosen_model:** `T-mid` (mid)
- **alternatives_considered:** `T-small` — higher cliché / flat tone risk; `T-frontier` — optional upgrade if brand-critical (not stated)
- **cache_hypothesis:** none
- **rough_cost_note:** ~0.3K in / 0.25K out @ T-mid band (placeholder)

### Example E — Tool-use agent step

**Prompt sketch:** System defines 6 tools (search docs, open file, run tests, …). User: “Find why CI fails on `auth.test.ts` and propose a patch.” Multi-step tool loop expected.

| Signal | Value |
| --- | --- |
| Task class | tool-use (+ code) |
| Length / complexity | medium–long / hard |
| Tools | yes |
| Latency | interactive |
| Prefix reuse | strong (large tool/system preamble) |

**Decision**

- **chosen_model:** `T-frontier` (frontier) — brittle multi-tool loop + debugging
- **alternatives_considered:** `T-mid` — viable if tool count/schemas stay simple and prior mid success exists; `T-small` — rejected — tool adherence risk
- **cache_hypothesis:** strong — shared tool/system preamble across agent turns
- **rough_cost_note:** ~3K in / 1K out first turn @ T-frontier band; subsequent turns may cache system+tools (placeholder). Costly but cache may amortize preamble.

---

## 7. Open questions / experiment backlog

For LLM Router Dev evals and later Typesafe Router runtime wiring:

1. **Measured cache hit rates** by tier/provider for shared system prompts of size 0.5K / 2K / 8K tokens.
2. **Quality cliffs** — where does `T-small` → `T-mid` → `T-frontier` actually move pass rate on code, classify, reason, creative, tool-use corpora?
3. **When does a stronger cache story beat a cheaper base rate?** Need a simple break-even formula once real bands exist (still placeholder until credentials + pricing cards land).
4. **Latency vs cost** tradeoffs for interactive tool loops (stop/continue thresholds).
5. **Mixed task prompts** — primary class conflicts (e.g. creative + tools); need tie-break rules.
6. **Prefix hashing** — what counts as “same” system prompt for cache hypothesis (whitespace, tool order, version pins)?
7. **Confidence calibration** — can the router abstain / escalate when signals are incomplete?
8. **Eval harness** — gold routes for the five example classes; paper decisions above are hypotheses, not ground truth.
9. **Credentials & live pricing** — blocked on Typesafe.ai access via Chief of Staff; replace placeholder bands only after secure card handoff. Never commit secrets.

---

## 8. Change control

- Edits to this doc are experimental policy changes; keep them reviewable in PRs.
- Implementation may diverge; when code lands, either update this doc or link an authoritative machine-readable table owned by LLM Router Dev.
- No production claims until evals + credentials + explicit promotion out of R&D.

//! Pack enriched session context into a TypeSafe System One request.
//!
//! Docs: <https://docs.typesafe.ai/api> — `POST /v1/systemone` with Choice.
//! Policy signals: `docs/decision-rules.md`.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::catalog::EnrichedCandidate;
use crate::types::RouterRequest;
use crate::typesafe::api::{ChoiceQuestion, Question, SystemOneRequest};

/// Default System One model alias per TypeSafe docs (`jev-latest`).
pub const DEFAULT_SYSTEM_ONE_MODEL: &str = "jev-latest";

/// Question id for the primary routing Choice.
pub const ROUTE_QUESTION_ID: &str = "route_to";

/// Question id for the structured tradeoff reason Choice.
pub const WHY_QUESTION_ID: &str = "primary_reason";

/// Build the System One payload TypeSafe evaluates.
///
/// State carries the full session, optional decision-rules input signals, and
/// per-model cost/cache/tier rows so the Choice can weigh continue-on-current
/// vs switch. Questions are narrow: pick a model id, then pick the primary axis.
pub fn pack_system_one_request(
    request: &RouterRequest,
    candidates: &[EnrichedCandidate],
    system_one_model: &str,
) -> SystemOneRequest {
    let state = build_state(request, candidates);
    let route_criteria = build_route_criteria(candidates);
    let why_criteria = why_reason_criteria(request.current_model.is_some());

    let mut questions = BTreeMap::new();
    questions.insert(
        ROUTE_QUESTION_ID.to_string(),
        Question::Choice(ChoiceQuestion {
            instructions: ROUTE_INSTRUCTIONS.to_string(),
            criteria: route_criteria,
        }),
    );
    questions.insert(
        WHY_QUESTION_ID.to_string(),
        Question::Choice(ChoiceQuestion {
            instructions: WHY_INSTRUCTIONS.to_string(),
            criteria: why_criteria,
        }),
    );

    SystemOneRequest {
        state,
        model: system_one_model.to_string(),
        questions,
    }
}

const ROUTE_INSTRUCTIONS: &str = "\
Which LLM should handle the next turn of this session? \
Follow decision-rules: prefer the cheapest tier that meets capability needs; \
prefer stronger prompt-cache stories when prefixes are shared and stable; \
weight latency only when latency_mode is interactive. \
Prefer continuing on the current model when cache locality meaningfully reduces \
cost without sacrificing needed quality. \
Only pick from the candidate model ids listed in criteria.";

const WHY_INSTRUCTIONS: &str = "\
What is the primary reason for the model choice in route_to? \
Pick exactly one axis.";

fn build_state(request: &RouterRequest, candidates: &[EnrichedCandidate]) -> Value {
    let session: Vec<Value> = request
        .session
        .iter()
        .map(|m| {
            json!({
                "role": m.role,
                "content": m.content,
            })
        })
        .collect();

    let candidate_rows: Vec<Value> = candidates
        .iter()
        .map(|c| {
            json!({
                "model_id": c.model_id,
                "tier": c.profile.tier.map(|t| t.as_str()),
                "tool_capable": c.profile.tool_capable,
                "cache_eligible": c.profile.cache_eligible,
                "is_current": c.is_current,
                "continuing_preserves_prompt_cache": c.is_current && c.profile.cache_eligible,
                "input_usd_per_mtok": c.profile.input_usd_per_mtok,
                "output_usd_per_mtok": c.profile.output_usd_per_mtok,
                "cost_band_in": c.profile.cost_band_in,
                "cost_band_out": c.profile.cost_band_out,
                "cache_read_usd_per_mtok": c.profile.cache_read_usd_per_mtok,
                "cache_write_usd_per_mtok": c.profile.cache_write_usd_per_mtok,
                "notes": c.profile.notes,
            })
        })
        .collect();

    json!({
        "task": "llm_session_routing",
        "policy_ref": "docs/decision-rules.md",
        "current_model": request.current_model,
        "session": session,
        "signals": {
            "task_class": request.task_class,
            "length": request.length,
            "complexity": request.complexity,
            "tools_required": request.tools_required,
            "latency_mode": request.latency_mode,
            "prefix_reuse": request.prefix_reuse,
            "prefix_tokens_est": request.prefix_tokens_est,
            "tokens_in_est": request.tokens_in_est,
            "tokens_out_est": request.tokens_out_est,
        },
        "candidates": candidate_rows,
        "routing_guidance": {
            "goal": "Pick the next model for this session.",
            "consider": [
                "capability fit for task_class / complexity",
                "caching hypothesis from prefix_reuse",
                "absolute $/MTok and cost bands",
                "whether continuing on current_model preserves warm prompt cache",
                "latency only when latency_mode is interactive"
            ]
        }
    })
}

fn build_route_criteria(candidates: &[EnrichedCandidate]) -> BTreeMap<String, Option<String>> {
    candidates
        .iter()
        .map(|c| {
            let desc = format_candidate_criterion(c);
            (c.model_id.clone(), Some(desc))
        })
        .collect()
}

fn format_candidate_criterion(c: &EnrichedCandidate) -> String {
    let mut parts = Vec::new();
    if let Some(tier) = c.profile.tier {
        parts.push(format!("{} ({})", tier.as_str(), tier.label()));
    }
    if c.is_current {
        parts.push("CURRENT model — continuing likely keeps prompt-cache hits".to_string());
    }
    parts.push(format!("tool_capable={}", c.profile.tool_capable));
    parts.push(format!("cache_eligible={}", c.profile.cache_eligible));
    parts.push(format!(
        "input=${:.4}/MTok output=${:.4}/MTok",
        c.profile.input_usd_per_mtok, c.profile.output_usd_per_mtok
    ));
    if let (Some(i), Some(o)) = (&c.profile.cost_band_in, &c.profile.cost_band_out) {
        parts.push(format!("bands in={i} out={o}"));
    }
    if let Some(notes) = &c.profile.notes {
        parts.push(notes.clone());
    }
    parts.join("; ")
}

fn why_reason_criteria(has_current: bool) -> BTreeMap<String, Option<String>> {
    let mut criteria = BTreeMap::new();
    if has_current {
        criteria.insert(
            "continue_current".into(),
            Some("Stay on the current model mainly to preserve cache / avoid switch cost".into()),
        );
    }
    criteria.insert(
        "cost".into(),
        Some("Primarily minimize dollar spend for adequate quality".into()),
    );
    criteria.insert(
        "cache".into(),
        Some("Primarily optimize prompt-cache economics (read vs write / locality)".into()),
    );
    criteria.insert(
        "quality".into(),
        Some("Primarily improve expected answer quality / capability".into()),
    );
    criteria
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ModelCatalog;
    use crate::types::{MessageRole, PrefixReuse, SessionMessage, TaskClass};

    #[test]
    fn pack_includes_route_why_and_signals() {
        let catalog = ModelCatalog::demo();
        let request = RouterRequest {
            session: vec![SessionMessage {
                role: MessageRole::User,
                content: "Summarize the thread so far.".into(),
            }],
            current_model: Some("composer-2.5".into()),
            allowlist: vec!["composer-2.5".into(), "grok-4.5".into()],
            task_class: Some(TaskClass::Code),
            length: None,
            complexity: None,
            tools_required: Some(false),
            latency_mode: None,
            prefix_reuse: Some(PrefixReuse::Weak),
            prefix_tokens_est: Some(200),
            tokens_in_est: None,
            tokens_out_est: None,
        };
        let candidates = catalog
            .enrich_allowlist(&request.allowlist, request.current_model.as_deref())
            .unwrap();
        let packed = pack_system_one_request(&request, &candidates, DEFAULT_SYSTEM_ONE_MODEL);
        assert_eq!(packed.model, "jev-latest");
        assert!(packed.questions.contains_key(ROUTE_QUESTION_ID));
        assert!(packed.questions.contains_key(WHY_QUESTION_ID));
        let value = serde_json::to_value(&packed).unwrap();
        assert_eq!(value["state"]["signals"]["task_class"], "code");
        assert_eq!(value["state"]["signals"]["prefix_reuse"], "weak");
        assert_eq!(value["questions"]["route_to"]["type"], "choice");
    }
}

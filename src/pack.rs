//! Pack enriched session context into a TypeSafe System One request.
//!
//! Docs: <https://docs.typesafe.ai/api> — `POST /v1/systemone` with Choice.

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
/// State carries the full session plus per-model cost/cache implications so
/// the Choice can weigh continue-on-current vs switch. Questions are narrow:
/// pick a model id, then pick the primary reason axis.
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
Weigh expected quality for the upcoming turn against dollar cost and \
prompt-cache implications. Prefer continuing on the current model when \
cache locality meaningfully reduces cost without sacrificing needed quality. \
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
                "is_current": c.is_current,
                "continuing_preserves_prompt_cache": c.is_current,
                "input_usd_per_mtok": c.profile.input_usd_per_mtok,
                "output_usd_per_mtok": c.profile.output_usd_per_mtok,
                "cache_read_usd_per_mtok": c.profile.cache_read_usd_per_mtok,
                "cache_write_usd_per_mtok": c.profile.cache_write_usd_per_mtok,
                "quality_tier": c.profile.quality_tier,
                "notes": c.profile.notes,
            })
        })
        .collect();

    json!({
        "task": "llm_session_routing",
        "current_model": request.current_model,
        "session": session,
        "candidates": candidate_rows,
        "routing_guidance": {
            "goal": "Pick the next model for this session.",
            "consider": [
                "quality needed for the latest user turn",
                "absolute $/MTok input and output",
                "cache read vs write economics if switching mid-session",
                "whether continuing on current_model preserves warm prompt cache"
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
    if c.is_current {
        parts.push("CURRENT model — continuing likely keeps prompt-cache hits".to_string());
    }
    if let Some(tier) = &c.profile.quality_tier {
        parts.push(format!("quality_tier={tier}"));
    }
    parts.push(format!(
        "input=${:.4}/MTok output=${:.4}/MTok",
        c.profile.input_usd_per_mtok, c.profile.output_usd_per_mtok
    ));
    if let Some(r) = c.profile.cache_read_usd_per_mtok {
        parts.push(format!("cache_read=${r:.4}/MTok"));
    }
    if let Some(w) = c.profile.cache_write_usd_per_mtok {
        parts.push(format!("cache_write=${w:.4}/MTok"));
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
    use crate::types::{MessageRole, SessionMessage};

    #[test]
    fn pack_includes_route_and_why_choices() {
        let catalog = ModelCatalog::demo();
        let request = RouterRequest {
            session: vec![SessionMessage {
                role: MessageRole::User,
                content: "Summarize the thread so far.".into(),
            }],
            current_model: Some("gpt-4o-mini".into()),
            allowlist: vec!["gpt-4o-mini".into(), "claude-sonnet-4".into()],
        };
        let candidates = catalog
            .enrich_allowlist(&request.allowlist, request.current_model.as_deref())
            .unwrap();
        let packed = pack_system_one_request(&request, &candidates, DEFAULT_SYSTEM_ONE_MODEL);
        assert_eq!(packed.model, "jev-latest");
        assert!(packed.questions.contains_key(ROUTE_QUESTION_ID));
        assert!(packed.questions.contains_key(WHY_QUESTION_ID));
        match packed.questions.get(ROUTE_QUESTION_ID).unwrap() {
            Question::Choice(c) => {
                assert!(c.criteria.contains_key("gpt-4o-mini"));
                assert!(c.criteria.contains_key("claude-sonnet-4"));
            }
            other => panic!("expected Choice, got {other:?}"),
        }
    }

    #[test]
    fn packed_json_matches_typesafe_wire_shape() {
        let catalog = ModelCatalog::demo();
        let request = RouterRequest {
            session: vec![SessionMessage {
                role: MessageRole::User,
                content: "hi".into(),
            }],
            current_model: None,
            allowlist: vec!["gpt-4o-mini".into()],
        };
        let candidates = catalog
            .enrich_allowlist(&request.allowlist, None)
            .unwrap();
        let packed = pack_system_one_request(&request, &candidates, DEFAULT_SYSTEM_ONE_MODEL);
        let value = serde_json::to_value(&packed).unwrap();
        assert_eq!(value["model"], "jev-latest");
        assert!(value["state"].is_object());
        assert_eq!(value["questions"]["route_to"]["type"], "choice");
        assert!(value["questions"]["route_to"]["criteria"]["gpt-4o-mini"].is_string());
        assert_eq!(value["questions"]["primary_reason"]["type"], "choice");
    }
}

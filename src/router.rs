//! Orchestrates enrich → pack → TypeSafe → map to [`RouterDecision`].

use crate::catalog::{EnrichedCandidate, ModelCatalog, ModelTier};
use crate::error::RouterError;
use crate::pack::{self, DEFAULT_SYSTEM_ONE_MODEL, ROUTE_QUESTION_ID, WHY_QUESTION_ID};
use crate::types::{
    AlternativeConsidered, CacheHypothesis, DecisionReason, PrefixReuse, RouterDecision,
    RouterRequest, TaskClass, WhyTradeoff,
};
use crate::typesafe::api::Answer;
use crate::typesafe::TypesafeClient;

pub struct Router<'a, C: TypesafeClient> {
    pub catalog: &'a ModelCatalog,
    pub client: &'a C,
    pub system_one_model: String,
}

impl<'a, C: TypesafeClient> Router<'a, C> {
    pub fn new(catalog: &'a ModelCatalog, client: &'a C) -> Self {
        Self {
            catalog,
            client,
            system_one_model: DEFAULT_SYSTEM_ONE_MODEL.to_string(),
        }
    }

    pub fn with_system_one_model(mut self, model: impl Into<String>) -> Self {
        self.system_one_model = model.into();
        self
    }

    /// Decide which allowlisted model should handle the next turn.
    pub fn route(&self, request: &RouterRequest) -> Result<RouterDecision, RouterError> {
        let candidates = self
            .catalog
            .enrich_allowlist(&request.allowlist, request.current_model.as_deref())?;

        let packed = pack::pack_system_one_request(request, &candidates, &self.system_one_model);

        let response = self.client.system_one(&packed)?;

        let route = response
            .answers
            .get(ROUTE_QUESTION_ID)
            .ok_or_else(|| RouterError::InvalidDecision("missing route_to answer".into()))?;

        let (chosen_model, confidence, probabilities) = match route {
            Answer::Choice(c) => {
                if !request.allowlist.iter().any(|m| m == &c.choice) {
                    return Err(RouterError::InvalidDecision(format!(
                        "TypeSafe chose `{}` which is not in the allowlist",
                        c.choice
                    )));
                }
                let probs = c
                    .probabilities
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                    .collect();
                (c.choice.clone(), Some(c.confidence), Some(probs))
            }
            other => {
                return Err(RouterError::InvalidDecision(format!(
                    "route_to expected Choice answer, got {other:?}"
                )))
            }
        };

        let primary_reason = response
            .answers
            .get(WHY_QUESTION_ID)
            .and_then(|a| match a {
                Answer::Choice(c) => Some(map_reason(&c.choice)),
                _ => None,
            })
            .unwrap_or(DecisionReason::Unspecified);

        let chosen_profile = candidates
            .iter()
            .find(|c| c.model_id == chosen_model)
            .map(|c| &c.profile);

        let chosen_tier = chosen_profile.and_then(|p| p.tier.map(|t| t.as_str().to_string()));

        let alternatives_considered =
            build_alternatives(&candidates, &chosen_model, request.tools_required);

        let cache_hypothesis =
            build_cache_hypothesis(request, chosen_profile.map(|p| p.cache_eligible));

        let rough_cost_note = build_cost_note(request, chosen_profile, &cache_hypothesis);

        let open_risk = build_open_risk(request, chosen_profile.and_then(|p| p.tier));

        let summary = build_summary(
            &chosen_model,
            chosen_tier.as_deref(),
            request.current_model.as_deref(),
            primary_reason,
            confidence,
        );

        Ok(RouterDecision {
            chosen_model: chosen_model.clone(),
            chosen_tier,
            primary_reason,
            alternatives_considered,
            cache_hypothesis,
            rough_cost_note,
            confidence,
            open_risk,
            model: chosen_model,
            why: WhyTradeoff {
                primary: primary_reason,
                summary,
                confidence,
                probabilities,
            },
        })
    }
}

fn map_reason(raw: &str) -> DecisionReason {
    match raw {
        "continue_current" => DecisionReason::ContinueCurrent,
        "cost" => DecisionReason::Cost,
        "cache" => DecisionReason::Cache,
        "quality" => DecisionReason::Quality,
        _ => DecisionReason::Unspecified,
    }
}

fn build_alternatives(
    candidates: &[EnrichedCandidate],
    chosen: &str,
    tools_required: Option<bool>,
) -> Vec<AlternativeConsidered> {
    candidates
        .iter()
        .filter(|c| c.model_id != chosen)
        .map(|c| {
            let tier = c.profile.tier.map(|t| t.as_str()).unwrap_or("unknown-tier");
            let why_rejected = reject_reason(c, chosen, tools_required);
            AlternativeConsidered {
                model_or_tier: format!("{} ({})", c.model_id, tier),
                why_rejected,
            }
        })
        .collect()
}

fn reject_reason(c: &EnrichedCandidate, chosen: &str, tools_required: Option<bool>) -> String {
    if tools_required == Some(true) && !c.profile.tool_capable {
        return "rejected — not tool_capable".into();
    }
    match c.profile.tier {
        Some(ModelTier::Small) => {
            "rejected — may under-serve complexity / adherence vs chosen".into()
        }
        Some(ModelTier::Frontier) => {
            format!("rejected — quality margin small vs `{chosen}`; higher cost band (placeholder)")
        }
        Some(ModelTier::Mid) => {
            format!("rejected — `{chosen}` preferred on cost/capability balance (placeholder)")
        }
        None => format!("rejected — not selected vs `{chosen}`"),
    }
}

fn build_cache_hypothesis(
    request: &RouterRequest,
    chosen_cache_eligible: Option<bool>,
) -> CacheHypothesis {
    let strength = request.prefix_reuse.unwrap_or(PrefixReuse::None);
    let mut parts = Vec::new();
    match strength {
        PrefixReuse::Strong => parts.push("shared/stable system or tool preamble expected".into()),
        PrefixReuse::Weak => parts.push("partial or unstable prefix reuse".into()),
        PrefixReuse::None => {
            parts.push("unique / one-shot prompt; no shared prefix called out".into())
        }
    }
    if let Some(n) = request.prefix_tokens_est {
        parts.push(format!("prefix_tokens_est≈{n}"));
    }
    if let Some(cur) = &request.current_model {
        parts.push(format!("current_model=`{cur}`"));
    }
    match chosen_cache_eligible {
        Some(true) => parts.push("chosen model is cache_eligible".into()),
        Some(false) => parts.push("chosen model not cache_eligible".into()),
        None => {}
    }
    CacheHypothesis {
        strength,
        rationale: parts.join("; "),
    }
}

fn build_cost_note(
    request: &RouterRequest,
    profile: Option<&crate::catalog::ModelCostProfile>,
    cache: &CacheHypothesis,
) -> String {
    let tin = request.tokens_in_est.unwrap_or(1500);
    let tout = request.tokens_out_est.unwrap_or(400);
    let tier = profile
        .and_then(|p| p.tier)
        .map(|t| t.as_str())
        .unwrap_or("unknown-tier");
    let band = profile
        .map(|p| p.band_note())
        .unwrap_or_else(|| "placeholder band".into());
    let cache_rate = profile
        .and_then(|p| p.cache_read_usd_per_mtok)
        .map(|r| format!("; cache_read=${r:.2}/MTok"))
        .unwrap_or_default();
    let cache_bit = match cache.strength {
        PrefixReuse::Strong => "expect prefix cache after warm-up",
        PrefixReuse::Weak => "weak cache",
        PrefixReuse::None => "no cache assumed",
    };
    format!(
        "~{} in / {} out @ {tier} ({band}{cache_rate}); {cache_bit}",
        fmt_token_est(tin),
        fmt_token_est(tout)
    )
}

fn fmt_token_est(n: u32) -> String {
    if n < 100 {
        format!("{n}")
    } else {
        format!("{:.1}K", n as f64 / 1000.0)
    }
}

fn build_open_risk(request: &RouterRequest, tier: Option<ModelTier>) -> Option<String> {
    match (request.task_class, tier) {
        (Some(TaskClass::LongReason), Some(ModelTier::Mid) | Some(ModelTier::Small)) => {
            Some("may under-develop failure modes / deep reasoning".into())
        }
        (Some(TaskClass::ToolUse), Some(ModelTier::Small)) => {
            Some("tool schema adherence risk on small tier".into())
        }
        (Some(TaskClass::Code), Some(ModelTier::Small)) => {
            Some("may miss edge cases on non-trivial code".into())
        }
        _ => None,
    }
}

fn build_summary(
    chosen: &str,
    tier: Option<&str>,
    current: Option<&str>,
    primary: DecisionReason,
    confidence: Option<f64>,
) -> String {
    let conf = confidence
        .map(|c| format!(" (confidence={c:.2})"))
        .unwrap_or_default();
    let tier_bit = tier.map(|t| format!(" [{t}]")).unwrap_or_default();
    let switched = match current {
        Some(cur) if cur == chosen => format!("continue on `{chosen}`{tier_bit}"),
        Some(cur) => format!("switch `{cur}` → `{chosen}`{tier_bit}"),
        None => format!("select `{chosen}`{tier_bit}"),
    };
    let axis = match primary {
        DecisionReason::ContinueCurrent => "preserve cache / continuity",
        DecisionReason::Cost => "cost",
        DecisionReason::Cache => "cache economics",
        DecisionReason::Quality => "quality",
        DecisionReason::Unspecified => "unspecified tradeoff",
    };
    format!("{switched}; primary axis: {axis}{conf}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ComplexityHint, LatencyMode, LengthHint, MessageRole, SessionMessage, TaskClass,
    };
    use crate::StubTypesafeClient;

    fn sample_request() -> RouterRequest {
        RouterRequest {
            session: vec![
                SessionMessage {
                    role: MessageRole::System,
                    content: "You are a helpful assistant.".into(),
                },
                SessionMessage {
                    role: MessageRole::User,
                    content: "What is the capital of France?".into(),
                },
            ],
            current_model: Some("composer-2.5".into()),
            allowlist: vec![
                "composer-2.5".into(),
                "grok-4.6".into(),
                "composer-2.5-fast".into(),
            ],
            task_class: Some(TaskClass::ShortClassify),
            length: Some(LengthHint::Short),
            complexity: Some(ComplexityHint::Simple),
            tools_required: Some(false),
            latency_mode: Some(LatencyMode::Batch),
            prefix_reuse: Some(PrefixReuse::Strong),
            prefix_tokens_est: Some(400),
            tokens_in_est: Some(500),
            tokens_out_est: Some(5),
        }
    }

    #[test]
    fn stub_router_emits_required_decision_fields() {
        let catalog = ModelCatalog::demo();
        let client = StubTypesafeClient::new();
        let router = Router::new(&catalog, &client);
        let decision = router.route(&sample_request()).unwrap();
        assert!(!decision.chosen_model.is_empty());
        assert_eq!(decision.model, decision.chosen_model);
        assert!(decision.chosen_tier.is_some());
        assert!(!decision.alternatives_considered.is_empty());
        assert_eq!(decision.cache_hypothesis.strength, PrefixReuse::Strong);
        assert!(decision.rough_cost_note.contains("cache_read"));
        assert!(
            decision.primary_reason != DecisionReason::Unspecified
                || decision.why.primary == decision.primary_reason
        );
    }

    #[test]
    fn stub_router_can_force_switch() {
        let catalog = ModelCatalog::demo();
        let client = StubTypesafeClient::with_force("composer-2.5-fast", "cost");
        let router = Router::new(&catalog, &client);
        let decision = router.route(&sample_request()).unwrap();
        assert_eq!(decision.chosen_model, "composer-2.5-fast");
        assert_eq!(decision.primary_reason, DecisionReason::Cost);
    }
}

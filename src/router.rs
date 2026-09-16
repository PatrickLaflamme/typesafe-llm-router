//! Orchestrates enrich → pack → TypeSafe → map to [`RouterDecision`].

use crate::catalog::ModelCatalog;
use crate::error::RouterError;
use crate::pack::{self, DEFAULT_SYSTEM_ONE_MODEL, ROUTE_QUESTION_ID, WHY_QUESTION_ID};
use crate::types::{DecisionReason, RouterDecision, RouterRequest, WhyTradeoff};
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

        let packed =
            pack::pack_system_one_request(request, &candidates, &self.system_one_model);

        let response = self.client.system_one(&packed)?;

        let route = response
            .answers
            .get(ROUTE_QUESTION_ID)
            .ok_or_else(|| RouterError::InvalidDecision("missing route_to answer".into()))?;

        let (model, confidence, probabilities) = match route {
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

        let primary = response
            .answers
            .get(WHY_QUESTION_ID)
            .and_then(|a| match a {
                Answer::Choice(c) => Some(map_reason(&c.choice)),
                _ => None,
            })
            .unwrap_or(DecisionReason::Unspecified);

        let summary = build_summary(
            &model,
            request.current_model.as_deref(),
            primary,
            confidence,
        );

        Ok(RouterDecision {
            model,
            why: WhyTradeoff {
                primary,
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

fn build_summary(
    chosen: &str,
    current: Option<&str>,
    primary: DecisionReason,
    confidence: Option<f64>,
) -> String {
    let conf = confidence
        .map(|c| format!(" (confidence={c:.2})"))
        .unwrap_or_default();
    let switched = match current {
        Some(cur) if cur == chosen => format!("continue on `{chosen}`"),
        Some(cur) => format!("switch `{cur}` → `{chosen}`"),
        None => format!("select `{chosen}`"),
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
    use crate::types::{MessageRole, SessionMessage};
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
            current_model: Some("gpt-4o-mini".into()),
            allowlist: vec![
                "gpt-4o-mini".into(),
                "gpt-4o".into(),
                "claude-haiku-3.5".into(),
            ],
        }
    }

    #[test]
    fn stub_router_continues_current() {
        let catalog = ModelCatalog::demo();
        let client = StubTypesafeClient::new();
        let router = Router::new(&catalog, &client);
        let decision = router.route(&sample_request()).unwrap();
        assert_eq!(decision.model, "gpt-4o-mini");
        assert_eq!(decision.why.primary, DecisionReason::ContinueCurrent);
    }

    #[test]
    fn stub_router_can_force_switch() {
        let catalog = ModelCatalog::demo();
        let client = StubTypesafeClient::with_force("claude-haiku-3.5", "cost");
        let router = Router::new(&catalog, &client);
        let decision = router.route(&sample_request()).unwrap();
        assert_eq!(decision.model, "claude-haiku-3.5");
        assert_eq!(decision.why.primary, DecisionReason::Cost);
    }
}

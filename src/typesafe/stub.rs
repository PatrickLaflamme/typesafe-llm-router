//! Offline stub for compile + tests without network or API keys.
//!
//! When not forced, applies lightweight decision-rules heuristics from
//! `task_class` / `prefix_reuse` so Examples A–E produce inspectable output.

use std::collections::BTreeMap;

use serde_json::Value;

use super::api::{Answer, ChoiceAnswer, Question, SystemOneRequest, SystemOneResponse, Usage};
use super::TypesafeClient;
use crate::error::TypesafeError;
use crate::pack::{ROUTE_QUESTION_ID, WHY_QUESTION_ID};

/// Deterministic stub: heuristics from packed state signals, else first option.
#[derive(Debug, Default, Clone)]
pub struct StubTypesafeClient {
    /// Optional forced model id (must appear in the route Choice criteria).
    pub force_model: Option<String>,
    /// Optional forced why reason id.
    pub force_reason: Option<String>,
}

impl StubTypesafeClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_force(model: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            force_model: Some(model.into()),
            force_reason: Some(reason.into()),
        }
    }
}

impl TypesafeClient for StubTypesafeClient {
    fn system_one(&self, request: &SystemOneRequest) -> Result<SystemOneResponse, TypesafeError> {
        let route_q = request
            .questions
            .get(ROUTE_QUESTION_ID)
            .ok_or_else(|| TypesafeError::UnexpectedAnswer("missing route_to question".into()))?;

        let options = match route_q {
            Question::Choice(c) => c.criteria.keys().cloned().collect::<Vec<_>>(),
            _ => {
                return Err(TypesafeError::UnexpectedAnswer(
                    "route_to must be a Choice".into(),
                ))
            }
        };
        if options.is_empty() {
            return Err(TypesafeError::UnexpectedAnswer(
                "route_to Choice has no options".into(),
            ));
        }

        let current = request
            .state
            .get("current_model")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let chosen = if let Some(forced) = &self.force_model {
            if !options.iter().any(|o| o == forced) {
                return Err(TypesafeError::UnexpectedAnswer(format!(
                    "force_model `{forced}` not in criteria"
                )));
            }
            forced.clone()
        } else {
            heuristic_choice(&request.state, &options, current.as_deref())
        };

        let remainder = if options.len() == 1 {
            0.0
        } else {
            0.3 / (options.len() as f64 - 1.0)
        };
        let mut probabilities = BTreeMap::new();
        for opt in &options {
            probabilities.insert(
                opt.clone(),
                if opt == &chosen { 0.7 } else { remainder },
            );
        }

        let reason = self.force_reason.clone().unwrap_or_else(|| {
            if current.as_deref() == Some(chosen.as_str()) {
                "continue_current".into()
            } else if request
                .state
                .pointer("/signals/prefix_reuse")
                .and_then(|v| v.as_str())
                == Some("strong")
            {
                "cache".into()
            } else if matches!(
                request
                    .state
                    .pointer("/signals/task_class")
                    .and_then(|v| v.as_str()),
                Some("long-reason" | "tool-use" | "code" | "creative")
            ) {
                "quality".into()
            } else {
                "cost".into()
            }
        });

        let why_options = match request.questions.get(WHY_QUESTION_ID) {
            Some(Question::Choice(c)) => c.criteria.keys().cloned().collect::<Vec<_>>(),
            _ => vec![reason.clone()],
        };
        let reason = if why_options.iter().any(|o| o == &reason) {
            reason
        } else {
            why_options
                .first()
                .cloned()
                .unwrap_or_else(|| "cost".into())
        };

        let mut why_probs = BTreeMap::new();
        let why_rem = if why_options.len() <= 1 {
            0.0
        } else {
            0.2 / (why_options.len() as f64 - 1.0)
        };
        for opt in &why_options {
            why_probs.insert(
                opt.clone(),
                if opt == &reason { 0.8 } else { why_rem },
            );
        }

        let mut answers = BTreeMap::new();
        answers.insert(
            ROUTE_QUESTION_ID.to_string(),
            Answer::Choice(ChoiceAnswer {
                choice: chosen,
                probabilities,
                confidence: 0.65,
            }),
        );
        answers.insert(
            WHY_QUESTION_ID.to_string(),
            Answer::Choice(ChoiceAnswer {
                choice: reason,
                probabilities: why_probs,
                confidence: 0.7,
            }),
        );

        Ok(SystemOneResponse {
            model: request.model.clone(),
            answers,
            usage: Some(Usage {
                input_tokens: 128,
                output_tokens: 16,
            }),
        })
    }
}

/// Prefer model ids whose catalog tier (embedded in candidate rows) matches
/// decision-rules defaults for the given task_class.
fn heuristic_choice(state: &Value, options: &[String], current: Option<&str>) -> String {
    let task = state
        .pointer("/signals/task_class")
        .and_then(|v| v.as_str());
    let prefix = state
        .pointer("/signals/prefix_reuse")
        .and_then(|v| v.as_str());
    let tools = state
        .pointer("/signals/tools_required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let preferred_tier = match task {
        Some("short-classify") => Some("T-small"),
        Some("code") | Some("creative") => Some("T-mid"),
        Some("long-reason") | Some("tool-use") => Some("T-frontier"),
        _ => None,
    };

    // Strong prefix + current still allowlisted → continue (cache locality).
    if prefix == Some("strong") {
        if let Some(cur) = current {
            if options.iter().any(|o| o == cur) && !matches!(task, Some("long-reason" | "tool-use"))
            {
                return cur.to_string();
            }
        }
    }

    if let Some(tier) = preferred_tier {
        if let Some(id) = first_matching_tier(state, options, tier, tools) {
            return id;
        }
    }

    if let Some(cur) = current {
        if options.iter().any(|o| o == cur) {
            return cur.to_string();
        }
    }
    options[0].clone()
}

fn first_matching_tier(
    state: &Value,
    options: &[String],
    tier: &str,
    require_tools: bool,
) -> Option<String> {
    let candidates = state.get("candidates")?.as_array()?;
    for opt in options {
        for row in candidates {
            if row.get("model_id").and_then(|v| v.as_str()) != Some(opt.as_str()) {
                continue;
            }
            if row.get("tier").and_then(|v| v.as_str()) != Some(tier) {
                continue;
            }
            if require_tools && row.get("tool_capable").and_then(|v| v.as_bool()) != Some(true) {
                continue;
            }
            return Some(opt.clone());
        }
    }
    None
}

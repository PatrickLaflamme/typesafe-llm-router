//! Offline stub for compile + tests without network or API keys.
//!
//! Handles Choice routing and Score rubrics (async worker / lab).

use std::collections::BTreeMap;

use serde_json::Value;

use super::api::{
    Answer, ChoiceAnswer, Question, ScoreAnswer, SystemOneRequest, SystemOneResponse, Usage,
};
use super::TypesafeClient;
use crate::error::TypesafeError;
use crate::pack::{ROUTE_QUESTION_ID, WHY_QUESTION_ID};
use crate::score::{INSTRUCTION_FOLLOW_QUESTION_ID, QUALITY_QUESTION_ID, TASK_FIT_QUESTION_ID};

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
        let mut answers = BTreeMap::new();

        if request.questions.contains_key(ROUTE_QUESTION_ID) {
            answers.extend(self.answer_route(request)?);
        }

        for (id, q) in &request.questions {
            if let Question::Score(score_q) = q {
                answers.insert(id.clone(), stub_score_answer(id, &score_q.criteria));
            }
        }

        if answers.is_empty() {
            return Err(TypesafeError::UnexpectedAnswer(
                "stub: no Choice or Score questions to answer".into(),
            ));
        }

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

impl StubTypesafeClient {
    fn answer_route(
        &self,
        request: &SystemOneRequest,
    ) -> Result<BTreeMap<String, Answer>, TypesafeError> {
        let route_q = request.questions.get(ROUTE_QUESTION_ID).unwrap();
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
            probabilities.insert(opt.clone(), if opt == &chosen { 0.7 } else { remainder });
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
            why_probs.insert(opt.clone(), if opt == &reason { 0.8 } else { why_rem });
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
        Ok(answers)
    }
}

fn stub_score_answer(id: &str, criteria: &[String]) -> Answer {
    let n_levels = criteria.len().max(1);
    // Bias toward upper-mid levels for demo inspectability.
    let peak = match id {
        QUALITY_QUESTION_ID => (n_levels.saturating_sub(1)).min(2),
        INSTRUCTION_FOLLOW_QUESTION_ID => n_levels.saturating_sub(1),
        TASK_FIT_QUESTION_ID => (n_levels.saturating_sub(1)).min(1),
        _ => n_levels / 2,
    };
    let mut probabilities = BTreeMap::new();
    let mut legend = BTreeMap::new();
    let rem = if n_levels <= 1 {
        0.0
    } else {
        0.25 / (n_levels as f64 - 1.0)
    };
    for i in 0..n_levels {
        let key = i.to_string();
        let label = criteria
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("level_{i}"));
        legend.insert(key.clone(), label);
        probabilities.insert(key, if i == peak { 0.75 } else { rem });
    }
    Answer::Score(ScoreAnswer {
        score: peak as f64,
        legend,
        probabilities,
        confidence: 0.72,
    })
}

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

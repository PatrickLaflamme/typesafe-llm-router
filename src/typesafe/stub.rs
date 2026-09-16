//! Offline stub for compile + tests without network or API keys.

use std::collections::BTreeMap;

use super::api::{Answer, ChoiceAnswer, Question, SystemOneRequest, SystemOneResponse, Usage};
use super::TypesafeClient;
use crate::error::TypesafeError;
use crate::pack::{ROUTE_QUESTION_ID, WHY_QUESTION_ID};

/// Deterministic stub: prefers `current_model` when present, else first Choice option.
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
        } else if let Some(cur) = &current {
            if options.iter().any(|o| o == cur) {
                cur.clone()
            } else {
                options[0].clone()
            }
        } else {
            options[0].clone()
        };

        // 0.7 on chosen; equal split of the remaining 0.3.
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

        let reason = self
            .force_reason
            .clone()
            .unwrap_or_else(|| {
                if current.as_deref() == Some(chosen.as_str()) {
                    "continue_current".into()
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

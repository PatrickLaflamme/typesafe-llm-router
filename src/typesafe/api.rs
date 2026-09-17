//! Wire types for TypeSafe `POST /v1/systemone`.
//!
//! Shape taken from the official HTTP API reference:
//! <https://docs.typesafe.ai/api>
//!
//! TODO(Patrick): if TypeSafe adds fields (e.g. request ids, tracing), extend
//! these structs with `#[serde(default)]` rather than inventing undocumented ones.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Request body for `POST /v1/systemone`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneRequest {
    /// Content to evaluate (string, object, or array). We send a structured object.
    pub state: Value,
    /// System One model alias. Docs default: `jev-latest`.
    pub model: String,
    /// Named typed questions; answers return under the same keys.
    pub questions: BTreeMap<String, Question>,
}

/// One of the three TypeSafe question primitives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Choice(ChoiceQuestion),
    Score(ScoreQuestion),
    Noul(NoulQuestion),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceQuestion {
    pub instructions: String,
    /// Option id → rubric description (`null` when no extra detail).
    pub criteria: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreQuestion {
    pub instructions: String,
    pub criteria: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulQuestion {
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<NoulCriteria>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulCriteria {
    #[serde(rename = "true", default, skip_serializing_if = "Option::is_none")]
    pub true_desc: Option<String>,
    #[serde(rename = "false", default, skip_serializing_if = "Option::is_none")]
    pub false_desc: Option<String>,
}

/// Response body from `POST /v1/systemone`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneResponse {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Choice(ChoiceAnswer),
    Score(ScoreAnswer),
    Noul(NoulAnswer),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceAnswer {
    pub choice: String,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreAnswer {
    pub score: f64,
    pub legend: BTreeMap<String, String>,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulAnswer {
    pub noul: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

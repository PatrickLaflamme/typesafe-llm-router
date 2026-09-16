//! Public I/O types for the router.

use serde::{Deserialize, Serialize};

/// One turn in the conversation / transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// Inputs the caller provides for a routing decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterRequest {
    /// Full session so far (conversation / transcript).
    pub session: Vec<SessionMessage>,
    /// Model already chosen for this session, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_model: Option<String>,
    /// Caller-defined allowlist of model ids that may be selected.
    pub allowlist: Vec<String>,
}

/// Structured explanation of the cost / cache / quality tradeoff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WhyTradeoff {
    /// Primary axis that drove the decision.
    pub primary: DecisionReason,
    /// Short human-readable summary (safe to log / show in UIs).
    pub summary: String,
    /// TypeSafe Choice confidence for the model pick, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// Full probability mass over allowlisted models, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probabilities: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionReason {
    /// Stay on the current model (usually to preserve prompt-cache hits).
    ContinueCurrent,
    /// Switch primarily to reduce spend.
    Cost,
    /// Switch primarily because cache economics favor another model.
    Cache,
    /// Switch primarily for expected quality / capability.
    Quality,
    /// Fallback when the decision client did not return a reason Choice.
    Unspecified,
}

/// Router output: which model to use next, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouterDecision {
    /// Selected model id (always a member of the request allowlist).
    pub model: String,
    pub why: WhyTradeoff,
}

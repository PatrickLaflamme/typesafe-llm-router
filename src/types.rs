//! Public I/O types for the router.
//!
//! Decision fields align with [`docs/decision-rules.md`](../../docs/decision-rules.md) §5.

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

/// Primary task class from decision-rules §2 / §4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskClass {
    Code,
    ShortClassify,
    LongReason,
    Creative,
    ToolUse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LengthHint {
    Short,
    Medium,
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComplexityHint {
    Simple,
    Medium,
    Hard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LatencyMode {
    Interactive,
    Batch,
}

/// Prompt-prefix reuse / cache hypothesis input signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrefixReuse {
    Strong,
    Weak,
    None,
}

impl PrefixReuse {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::Weak => "weak",
            Self::None => "none",
        }
    }
}

/// Inputs the caller provides for a routing decision.
///
/// Optional signal fields are pass-through only — this crate does not invent
/// classifiers. Callers (or Typesafe Router fixtures) supply them when known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterRequest {
    /// Full session so far (conversation / transcript).
    pub session: Vec<SessionMessage>,
    /// Model already chosen for this session, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_model: Option<String>,
    /// Model ids Choice may select. When a ModelProvider is selected and this is
    /// empty, the CLI fills it from `ModelProvider.list_models()`.
    #[serde(default)]
    pub allowlist: Vec<String>,

    /// Optional: task class (code | short-classify | long-reason | creative | tool-use).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_class: Option<TaskClass>,
    /// Optional: length hint (short | medium | long).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<LengthHint>,
    /// Optional: complexity hint (simple | medium | hard).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<ComplexityHint>,
    /// Optional: whether tools / function calling are required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_required: Option<bool>,
    /// Optional: interactive vs batch latency mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_mode: Option<LatencyMode>,
    /// Optional: shared-prefix reuse strength (feeds cache_hypothesis).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_reuse: Option<PrefixReuse>,
    /// Optional: order-of-magnitude reusable prefix tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_tokens_est: Option<u32>,
    /// Optional: rough expected input tokens (for cost note; placeholder OK).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_in_est: Option<u32>,
    /// Optional: rough expected output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_out_est: Option<u32>,
}

/// Rejected alternative with a one-line reason (decision-rules §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlternativeConsidered {
    pub model_or_tier: String,
    pub why_rejected: String,
}

/// Cache hypothesis emitted on every decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheHypothesis {
    pub strength: PrefixReuse,
    pub rationale: String,
}

/// Structured explanation of the cost / cache / quality tradeoff (compat nest).
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

impl DecisionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ContinueCurrent => "continue_current",
            Self::Cost => "cost",
            Self::Cache => "cache",
            Self::Quality => "quality",
            Self::Unspecified => "unspecified",
        }
    }
}

/// Router output: first-class decision-rules fields + backward-compat nest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouterDecision {
    /// Selected model id (always a member of the request allowlist).
    pub chosen_model: String,
    /// Catalog tier label when known (`T-small` | `T-mid` | `T-frontier`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen_tier: Option<String>,
    /// Primary tradeoff axis from System One Choice `primary_reason`.
    pub primary_reason: DecisionReason,
    /// Other candidates scored, with one-line rejection reasons.
    pub alternatives_considered: Vec<AlternativeConsidered>,
    /// Caching hypothesis (strong | weak | none + rationale).
    pub cache_hypothesis: CacheHypothesis,
    /// Placeholder band × token estimate note (not live pricing).
    pub rough_cost_note: String,
    /// TypeSafe Choice confidence for the model pick, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// Optional open risk note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_risk: Option<String>,

    /// Backward-compatible alias of `chosen_model`.
    pub model: String,
    /// Backward-compatible nested why blob.
    pub why: WhyTradeoff,
}

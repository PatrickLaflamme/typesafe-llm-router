//! Pluggable model execution after Choice routing.
//!
//! Hot path: Choice route → [`ModelSource::complete`] → return decision + output.
//! Score stays async *after* output is durable (see [`crate::score_queue`]).
//!
//! Trait shape aligned with Typesafe Router lab design (`docs/model-source.md`).

mod cursor_agent;
mod stub;

pub use cursor_agent::{CursorAgentSdkSource, CURSOR_API_KEY_ENV, CURSOR_HELPER_ENV};
pub use stub::StubModelSource;

use serde::{Deserialize, Serialize};

use crate::catalog::ModelTier;
use crate::error::ModelSourceError;

/// Stable backend name (`stub`, `cursor-agent`, …).
pub type SourceName = &'static str;

/// Catalog / source model card for [`ModelSource::list_models`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<ModelTier>,
}

/// Local vs cloud Cursor (or provider) runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelRuntime {
    #[default]
    Local,
    Cloud,
}

/// Request to execute the model chosen by the router.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompleteRequest {
    pub model_id: String,
    /// Flattened prompt (and/or last user turn). Sources may also use `messages`.
    pub prompt: String,
    /// Optional full transcript for chat-style sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<crate::types::SessionMessage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub runtime: ModelRuntime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_repos: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_params: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// Result of a model completion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompleteResponse {
    pub model_output: String,
    /// Backend that produced the output (`stub`, `cursor-agent`, …).
    pub source: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
}

/// Execute a chosen model. Sync for v0 (crate is sync).
///
/// Cursor execution may be slow — that latency is on the **model-execution**
/// path. Score remains async after output is durable.
pub trait ModelSource {
    fn name(&self) -> SourceName;

    fn list_models(&self) -> Result<Vec<ModelInfo>, ModelSourceError>;

    fn complete(&self, req: &CompleteRequest) -> Result<CompleteResponse, ModelSourceError>;
}

/// Build a [`CompleteRequest`] from a routed session.
pub fn complete_request_from_session(
    model_id: impl Into<String>,
    session: &[crate::types::SessionMessage],
    cwd: Option<String>,
) -> CompleteRequest {
    let prompt = session
        .iter()
        .rev()
        .find(|m| matches!(m.role, crate::types::MessageRole::User))
        .or_else(|| session.last())
        .map(|m| m.content.clone())
        .unwrap_or_default();
    CompleteRequest {
        model_id: model_id.into(),
        prompt,
        messages: Some(session.to_vec()),
        cwd,
        runtime: ModelRuntime::Local,
        cloud_repos: None,
        model_params: None,
        timeout_ms: None,
    }
}

// Backward-compatible aliases used in early scaffold docs / call sites.
pub type ModelSourceRequest = CompleteRequest;
pub type ModelSourceResult = CompleteResponse;

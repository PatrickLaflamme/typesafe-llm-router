//! Pluggable model execution after Choice routing.
//!
//! Hot path: Choice route → [`ModelSource::complete`] → return decision + output.
//! Score stays async *after* output is durable (see [`crate::score_queue`]).
//!
//! **Patrick lock (2026-09-17):** Choice allowlist MUST come from
//! [`ModelSource::list_models`] (source model ids only). Token prices live on
//! [`ModelInfo`]. `RouterDecision.chosen_model` **is** the id passed to
//! [`ModelSource::complete`] — no post-Choice remap.
//!
//! Trait shape: `docs/model-source.md`.

mod cursor_agent;
mod stub;

pub use cursor_agent::{CursorAgentSdkSource, CURSOR_API_KEY_ENV, CURSOR_HELPER_ENV};
pub use stub::StubModelSource;

use serde::{Deserialize, Serialize};

use crate::catalog::{ModelCostProfile, ModelTier};
use crate::error::ModelSourceError;
use crate::types::{MessageRole, SessionMessage};

/// Stable backend name (`stub`, `cursor-agent`, …).
pub type SourceName = &'static str;

/// Source model card for [`ModelSource::list_models`].
///
/// Choice allowlist := these `id`s. Prices are USD per 1M tokens (lab bake-in
/// from provider docs, or obvious fixture rates on the stub).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Source-native model id (e.g. `composer-2.5`).
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// USD per 1M input tokens.
    pub price_input_per_mtok: f64,
    /// USD per 1M output tokens.
    pub price_output_per_mtok: f64,
    /// USD per 1M cache-read tokens, when the provider publishes a rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_cache_read_per_mtok: Option<f64>,
    /// USD per 1M cache-write tokens, when published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_cache_write_per_mtok: Option<f64>,
    /// Optional capability hint only — not a parallel catalog allowlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier_hint: Option<ModelTier>,
}

impl ModelInfo {
    /// Map source pricing into the catalog profile used by Choice packing.
    pub fn to_cost_profile(&self) -> ModelCostProfile {
        let cache_eligible = self.price_cache_read_per_mtok.is_some()
            || self.price_cache_write_per_mtok.is_some();
        ModelCostProfile {
            id: Some(self.id.clone()),
            tier: self.tier_hint,
            tool_capable: true,
            input_usd_per_mtok: self.price_input_per_mtok,
            output_usd_per_mtok: self.price_output_per_mtok,
            cost_band_in: None,
            cost_band_out: None,
            cache_eligible,
            cache_read_usd_per_mtok: self.price_cache_read_per_mtok,
            cache_write_usd_per_mtok: self.price_cache_write_per_mtok,
            notes: self.label.clone(),
        }
    }
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
    /// Full session flattened for the source (system + user + …).
    pub prompt: String,
    /// Optional full transcript for chat-style sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<SessionMessage>>,
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

    /// Source-native model cards (ids + prices). Choice allowlist := these ids.
    fn list_models(&self) -> Result<Vec<ModelInfo>, ModelSourceError>;

    fn complete(&self, req: &CompleteRequest) -> Result<CompleteResponse, ModelSourceError>;
}

/// Flatten a full session into a single prompt for ModelSource execution.
///
/// Includes system + user (+ assistant/tool) so classify demos keep the label
/// instruction and return a short reply.
pub fn format_session_prompt(session: &[SessionMessage]) -> String {
    if session.is_empty() {
        return String::new();
    }
    let mut parts = Vec::with_capacity(session.len());
    for m in session {
        let role = match m.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        parts.push(format!("[{role}]\n{}", m.content));
    }
    parts.join("\n\n")
}

/// Build a [`CompleteRequest`] from a routed session.
///
/// `model_id` must be the ModelSource id that Choice selected (no remap).
pub fn complete_request_from_session(
    model_id: impl Into<String>,
    session: &[SessionMessage],
    cwd: Option<String>,
) -> CompleteRequest {
    CompleteRequest {
        model_id: model_id.into(),
        prompt: format_session_prompt(session),
        messages: Some(session.to_vec()),
        cwd,
        runtime: ModelRuntime::Local,
        cloud_repos: None,
        model_params: None,
        timeout_ms: None,
    }
}

/// Allowlist ids (+ optional current) from a ModelSource, optionally intersected
/// with an explicit fixture allowlist.
pub fn allowlist_from_source(
    models: &[ModelInfo],
    explicit: &[String],
    current_model: Option<&str>,
) -> Result<(Vec<String>, crate::catalog::ModelCatalog), ModelSourceError> {
    if models.is_empty() {
        return Err(ModelSourceError::Other(
            "ModelSource.list_models() returned no models".into(),
        ));
    }

    let catalog = crate::catalog::ModelCatalog::from_model_infos(models);

    let allowlist = if explicit.is_empty() {
        models.iter().map(|m| m.id.clone()).collect()
    } else {
        for id in explicit {
            if !models.iter().any(|m| m.id == *id) {
                return Err(ModelSourceError::Other(format!(
                    "allowlist id `{id}` is not in ModelSource.list_models()"
                )));
            }
        }
        explicit.to_vec()
    };

    if let Some(cur) = current_model {
        if !allowlist.iter().any(|m| m == cur) {
            return Err(ModelSourceError::Other(format!(
                "current_model `{cur}` is not in the ModelSource allowlist"
            )));
        }
    }

    Ok((allowlist, catalog))
}

// Backward-compatible aliases used in early scaffold docs / call sites.
pub type ModelSourceRequest = CompleteRequest;
pub type ModelSourceResult = CompleteResponse;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_session_includes_system_and_user() {
        let session = vec![
            SessionMessage {
                role: MessageRole::System,
                content: "Reply with one label only.".into(),
            },
            SessionMessage {
                role: MessageRole::User,
                content: "I was charged twice.".into(),
            },
        ];
        let prompt = format_session_prompt(&session);
        assert!(prompt.contains("[system]"));
        assert!(prompt.contains("Reply with one label only."));
        assert!(prompt.contains("[user]"));
        assert!(prompt.contains("charged twice"));
    }

    #[test]
    fn allowlist_from_source_rejects_foreign_ids() {
        let models = vec![ModelInfo {
            id: "composer-2.5".into(),
            label: None,
            price_input_per_mtok: 0.5,
            price_output_per_mtok: 2.5,
            price_cache_read_per_mtok: Some(0.2),
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Small),
        }];
        let err = allowlist_from_source(&models, &["gpt-4o-mini".into()], None).unwrap_err();
        assert!(err.to_string().contains("gpt-4o-mini"));
    }
}

//! Offline model execution for A–E fixtures and morning demo.
//!
//! Stub ids match Cursor ModelProvider ids so Choice allowlist == execute id
//! (Patrick lock 2026-09-17). Prices are obvious fixture rates, not live quotes.

use super::{CompleteRequest, CompleteResponse, ModelInfo, ModelProvider, UsageMeta};
use crate::catalog::ModelTier;
use crate::error::ModelProviderError;
use crate::types::TaskClass;

/// Deterministic stub: returns visible fixture text (no network).
#[derive(Debug, Default, Clone)]
pub struct StubModelProvider {
    /// Optional task_class hint for fixture-shaped outputs (demo).
    pub task_class: Option<TaskClass>,
}

impl StubModelProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_task_class(task_class: Option<TaskClass>) -> Self {
        Self { task_class }
    }
}

impl ModelProvider for StubModelProvider {
    fn name(&self) -> &'static str {
        "stub"
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ModelProviderError> {
        // Obvious fixture USD/MTok — not Cursor live rates (see CursorAgentSdkSource).
        Ok(vec![
            ModelInfo {
                id: "composer-2.5".into(),
                label: Some("stub fixture · cheap / cache-friendly".into()),
                price_input_per_mtok: 1.0,
                price_output_per_mtok: 2.0,
                price_cache_read_per_mtok: Some(0.1),
                price_cache_write_per_mtok: Some(0.5),
                tier_hint: Some(ModelTier::Small),
            },
            ModelInfo {
                id: "composer-2.5-fast".into(),
                label: Some("stub fixture · fast small".into()),
                price_input_per_mtok: 5.0,
                price_output_per_mtok: 10.0,
                price_cache_read_per_mtok: Some(1.0),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Small),
            },
            ModelInfo {
                id: "grok-4.6".into(),
                label: Some("stub fixture · mid".into()),
                price_input_per_mtok: 3.0,
                price_output_per_mtok: 8.0,
                price_cache_read_per_mtok: Some(0.5),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Mid),
            },
            ModelInfo {
                id: "grok-4.6-fast".into(),
                label: Some("stub fixture · fast mid".into()),
                price_input_per_mtok: 6.0,
                price_output_per_mtok: 16.0,
                price_cache_read_per_mtok: Some(1.5),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Mid),
            },
            ModelInfo {
                id: "grok-4.5".into(),
                label: Some("stub fixture · frontier-ish".into()),
                price_input_per_mtok: 4.0,
                price_output_per_mtok: 12.0,
                price_cache_read_per_mtok: Some(0.5),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Frontier),
            },
            ModelInfo {
                id: "grok-4.5-fast".into(),
                label: Some("stub fixture · fast frontier".into()),
                price_input_per_mtok: 8.0,
                price_output_per_mtok: 24.0,
                price_cache_read_per_mtok: Some(2.0),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Frontier),
            },
        ])
    }

    fn complete(&self, req: &CompleteRequest) -> Result<CompleteResponse, ModelProviderError> {
        let output = fixture_output(self.task_class, &req.prompt, &req.model_id);
        Ok(CompleteResponse {
            model_output: output,
            source: self.name().to_string(),
            model_id: req.model_id.clone(),
            run_id: Some(format!("stub-{}", req.model_id)),
            usage: Some(UsageMeta {
                input_tokens: Some((req.prompt.len() / 4) as u32),
                output_tokens: Some(8),
            }),
            raw_meta: Some(serde_json::json!({
                "stub": true,
                "task_class": self.task_class,
            })),
        })
    }
}

/// Visible, recording-friendly stub text for paper fixtures.
fn fixture_output(task_class: Option<TaskClass>, prompt: &str, model_id: &str) -> String {
    match task_class {
        Some(TaskClass::ShortClassify) => "billing".into(),
        Some(TaskClass::Code) => {
            "Fixed off-by-one: use `items.slice(start, start + pageSize)` (keep public API)."
                .into()
        }
        Some(TaskClass::LongReason) => {
            "Recommend Raft-style consensus for the multi-region queue; document split-brain and leader failover (stub)."
                .into()
        }
        Some(TaskClass::Creative) => {
            "1) Pack light. Walk loud.\n2) Mud is a feature.\n3) Trails > trends.".into()
        }
        Some(TaskClass::ToolUse) => {
            "CI fails in auth.test.ts on expired fixture token; patch: refresh mock JWT in beforeEach (stub)."
                .into()
        }
        None => {
            // Heuristic from prompt for demos without task_class.
            let lower = prompt.to_lowercase();
            if lower.contains("billing") || lower.contains("label this support") {
                "billing".into()
            } else if lower.contains("off-by-one") || lower.contains("pagination") {
                "Fixed off-by-one: use `items.slice(start, start + pageSize)`.".into()
            } else {
                format!("[stub:{model_id}] ok")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_classify_returns_billing() {
        let src = StubModelProvider::with_task_class(Some(TaskClass::ShortClassify));
        let out = src
            .complete(&CompleteRequest {
                model_id: "composer-2.5".into(),
                prompt: "[system]\nlabel\n\n[user]\nticket".into(),
                messages: None,
                cwd: None,
                runtime: Default::default(),
                cloud_repos: None,
                model_params: None,
                timeout_ms: None,
            })
            .unwrap();
        assert_eq!(out.model_output, "billing");
        assert_eq!(out.source, "stub");
        assert_eq!(out.model_id, "composer-2.5");
    }

    #[test]
    fn list_models_uses_source_ids_with_fixture_prices() {
        let models = StubModelProvider::new().list_models().unwrap();
        assert!(models.iter().any(|m| m.id == "composer-2.5"));
        assert!(models.iter().any(|m| m.id == "grok-4.6"));
        let cheap = models.iter().find(|m| m.id == "composer-2.5").unwrap();
        assert_eq!(cheap.price_input_per_mtok, 1.0);
        assert!(cheap.price_cache_read_per_mtok.is_some());
    }
}

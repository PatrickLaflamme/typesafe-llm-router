//! Offline model execution for A–E fixtures and morning demo.

use super::{CompleteRequest, CompleteResponse, ModelInfo, ModelSource, UsageMeta};
use crate::catalog::ModelTier;
use crate::error::ModelSourceError;
use crate::types::TaskClass;

/// Deterministic stub: returns visible fixture text (no network).
#[derive(Debug, Default, Clone)]
pub struct StubModelSource {
    /// Optional task_class hint for fixture-shaped outputs (demo).
    pub task_class: Option<TaskClass>,
}

impl StubModelSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_task_class(task_class: Option<TaskClass>) -> Self {
        Self { task_class }
    }
}

impl ModelSource for StubModelSource {
    fn name(&self) -> &'static str {
        "stub"
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ModelSourceError> {
        Ok(vec![
            ModelInfo {
                id: "gpt-4o-mini".into(),
                label: Some("stub small".into()),
                tier: Some(ModelTier::Small),
            },
            ModelInfo {
                id: "gpt-4o".into(),
                label: Some("stub mid".into()),
                tier: Some(ModelTier::Mid),
            },
            ModelInfo {
                id: "claude-sonnet-4".into(),
                label: Some("stub frontier".into()),
                tier: Some(ModelTier::Frontier),
            },
        ])
    }

    fn complete(&self, req: &CompleteRequest) -> Result<CompleteResponse, ModelSourceError> {
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
        let src = StubModelSource::with_task_class(Some(TaskClass::ShortClassify));
        let out = src
            .complete(&CompleteRequest {
                model_id: "gpt-4o-mini".into(),
                prompt: "label ticket".into(),
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
    }
}

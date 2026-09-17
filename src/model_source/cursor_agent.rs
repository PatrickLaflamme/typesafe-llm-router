//! Cursor Agent model source via Node sidecar (`@cursor/sdk`).
//!
//! Uses `Agent.prompt(message, { apiKey, model:{id}, local:{cwd} })` (one-shot).
//! Auth: `CURSOR_API_KEY` env only (never commit secrets).
//!
//! **Pricing snapshot** (USD / 1M tokens) baked into [`ModelInfo`]:
//! source <https://cursor.com/docs/models-and-pricing> as of **2026-09-17**.
//! Refresh when Cursor docs change. No separate cost TOML required for Choice.

use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::{
    CompleteRequest, CompleteResponse, ModelInfo, ModelRuntime, ModelSource, UsageMeta,
};
use crate::catalog::ModelTier;
use crate::error::ModelSourceError;

pub const CURSOR_API_KEY_ENV: &str = "CURSOR_API_KEY";
pub const CURSOR_HELPER_ENV: &str = "CURSOR_AGENT_HELPER";
pub const DEFAULT_HELPER_REL: &str = "scripts/cursor_agent_complete.mjs";

/// Cursor Agent SDK source (Node helper → `@cursor/sdk` `Agent.prompt`).
#[derive(Debug, Clone)]
pub struct CursorAgentSdkSource {
    pub helper_path: PathBuf,
    pub api_key: Option<String>,
    pub node_bin: String,
}

impl CursorAgentSdkSource {
    pub fn from_env() -> Self {
        let helper_path = env::var(CURSOR_HELPER_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_HELPER_REL));
        let api_key = env::var(CURSOR_API_KEY_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty());
        Self {
            helper_path,
            api_key,
            node_bin: env::var("NODE_BIN").unwrap_or_else(|_| "node".into()),
        }
    }

    pub fn new(helper_path: impl Into<PathBuf>, api_key: Option<String>) -> Self {
        Self {
            helper_path: helper_path.into(),
            api_key,
            node_bin: "node".into(),
        }
    }
}

/// Cursor Models pool + prices from docs (2026-09-17 snapshot).
///
/// Source: https://cursor.com/docs/models-and-pricing
fn cursor_model_catalog() -> Vec<ModelInfo> {
    // Prices: input / cache_read / output — USD per 1M tokens.
    vec![
        ModelInfo {
            id: "composer-2.5".into(),
            label: Some("Composer 2.5 — prefer when prefix reuse / batch latency OK".into()),
            price_input_per_mtok: 0.50,
            price_output_per_mtok: 2.50,
            price_cache_read_per_mtok: Some(0.20),
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Small),
        },
        ModelInfo {
            id: "composer-2.5-fast".into(),
            label: Some("Composer 2.5 Fast — ~6× standard input".into()),
            price_input_per_mtok: 3.00,
            price_output_per_mtok: 15.00,
            price_cache_read_per_mtok: Some(0.50),
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Small),
        },
        ModelInfo {
            id: "grok-4.6".into(),
            label: Some("Grok 4.6 — Cursor Models pool".into()),
            price_input_per_mtok: 2.00,
            price_output_per_mtok: 6.00,
            price_cache_read_per_mtok: Some(0.50),
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Mid),
        },
        ModelInfo {
            id: "grok-4.6-fast".into(),
            label: Some("Grok 4.6 Fast".into()),
            price_input_per_mtok: 4.00,
            price_output_per_mtok: 12.00,
            price_cache_read_per_mtok: Some(1.00),
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Mid),
        },
        ModelInfo {
            id: "grok-4.5".into(),
            label: Some("Grok 4.5 — Cursor Models pool".into()),
            price_input_per_mtok: 2.00,
            price_output_per_mtok: 6.00,
            price_cache_read_per_mtok: Some(0.50),
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Frontier),
        },
        ModelInfo {
            id: "grok-4.5-fast".into(),
            label: Some("Grok 4.5 Fast".into()),
            price_input_per_mtok: 4.00,
            price_output_per_mtok: 18.00,
            price_cache_read_per_mtok: Some(1.00),
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Frontier),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_models_bakes_cursor_pricing_snapshot() {
        let src = CursorAgentSdkSource::new("scripts/cursor_agent_complete.mjs", None);
        let models = src.list_models().unwrap();
        assert_eq!(models.len(), 6);

        let c = models.iter().find(|m| m.id == "composer-2.5").unwrap();
        assert_eq!(c.price_input_per_mtok, 0.50);
        assert_eq!(c.price_cache_read_per_mtok, Some(0.20));
        assert_eq!(c.price_output_per_mtok, 2.50);

        let cf = models.iter().find(|m| m.id == "composer-2.5-fast").unwrap();
        assert_eq!(cf.price_input_per_mtok, 3.00);
        assert_eq!(cf.price_cache_read_per_mtok, Some(0.50));
        assert_eq!(cf.price_output_per_mtok, 15.00);

        let g = models.iter().find(|m| m.id == "grok-4.6").unwrap();
        assert_eq!(g.price_input_per_mtok, 2.00);
        assert_eq!(g.price_cache_read_per_mtok, Some(0.50));
        assert_eq!(g.price_output_per_mtok, 6.00);

        let gf = models.iter().find(|m| m.id == "grok-4.6-fast").unwrap();
        assert_eq!(gf.price_input_per_mtok, 4.00);
        assert_eq!(gf.price_output_per_mtok, 12.00);

        let g45 = models.iter().find(|m| m.id == "grok-4.5").unwrap();
        assert_eq!(g45.price_input_per_mtok, 2.00);
        assert_eq!(g45.price_output_per_mtok, 6.00);

        let g45f = models.iter().find(|m| m.id == "grok-4.5-fast").unwrap();
        assert_eq!(g45f.price_input_per_mtok, 4.00);
        assert_eq!(g45f.price_cache_read_per_mtok, Some(1.00));
        assert_eq!(g45f.price_output_per_mtok, 18.00);
    }
}

#[derive(Debug, Serialize)]
struct HelperRequest<'a> {
    api_key: &'a str,
    chosen_model: &'a str,
    prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<&'a str>,
    runtime: &'a str,
}

#[derive(Debug, Deserialize)]
struct HelperResponse {
    model_output: String,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
    #[serde(default)]
    raw_meta: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
}

impl ModelSource for CursorAgentSdkSource {
    fn name(&self) -> &'static str {
        "cursor-agent"
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ModelSourceError> {
        Ok(cursor_model_catalog())
    }

    fn complete(&self, req: &CompleteRequest) -> Result<CompleteResponse, ModelSourceError> {
        let api_key = self
            .api_key
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or(ModelSourceError::MissingCursorApiKey)?;

        if !self.helper_path.exists() {
            return Err(ModelSourceError::CursorHelper(format!(
                "helper not found at {} — set {CURSOR_HELPER_ENV} or run from repo root \
                 (docs/model-source.md).",
                self.helper_path.display()
            )));
        }

        let runtime = match req.runtime {
            ModelRuntime::Local => "local",
            ModelRuntime::Cloud => "cloud",
        };
        let payload = HelperRequest {
            api_key,
            chosen_model: &req.model_id,
            prompt: &req.prompt,
            cwd: req.cwd.as_deref(),
            runtime,
        };

        let mut child = Command::new(&self.node_bin)
            .arg(&self.helper_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                ModelSourceError::CursorHelper(format!(
                    "failed to spawn `{} {}`: {e}",
                    self.node_bin,
                    self.helper_path.display()
                ))
            })?;

        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| ModelSourceError::CursorHelper("helper stdin unavailable".into()))?;
            serde_json::to_writer(&mut *stdin, &payload)?;
            stdin.flush()?;
        }

        let output = child.wait_with_output()?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            return Err(ModelSourceError::CursorHelper(format!(
                "exit {}: {stderr}",
                output.status
            )));
        }

        let parsed: HelperResponse = serde_json::from_slice(&output.stdout).map_err(|e| {
            ModelSourceError::CursorHelper(format!(
                "invalid helper JSON ({e}): {}",
                String::from_utf8_lossy(&output.stdout)
            ))
        })?;

        if let Some(err) = parsed.error {
            return Err(ModelSourceError::CursorHelper(err));
        }

        Ok(CompleteResponse {
            model_output: parsed.model_output,
            source: self.name().to_string(),
            model_id: req.model_id.clone(),
            run_id: parsed.run_id,
            usage: Some(UsageMeta {
                input_tokens: parsed.input_tokens,
                output_tokens: parsed.output_tokens,
            }),
            raw_meta: parsed.raw_meta.or_else(|| {
                Some(serde_json::json!({
                    "helper": self.helper_path.display().to_string(),
                    "runtime": runtime,
                    "sdk": "Agent.prompt(message, { apiKey, model:{id}, local:{cwd} })"
                }))
            }),
        })
    }
}

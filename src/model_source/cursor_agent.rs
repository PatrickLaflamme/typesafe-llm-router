//! Cursor Agent model source via Node sidecar (`@cursor/sdk`).
//!
//! Typesafe Router design: `Agent.create({ apiKey, model:{id}, local:{cwd} })` + send/wait.
//! Auth: `CURSOR_API_KEY` env only (never commit secrets).
//!
//! No first-party Rust SDK — sidecar or [SDK Bridge](https://cursor.com/docs/sdk/bridge).
//! Tier → Cursor id map: `config/model-map.toml`.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
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
pub const DEFAULT_MODEL_MAP_REL: &str = "config/model-map.toml";

/// Cursor Agent SDK source (Node helper → `@cursor/sdk`).
///
/// TODO: optional Connect client against `cursor-sdk-bridge` once binaries are pinned.
#[derive(Debug, Clone)]
pub struct CursorAgentSdkSource {
    pub helper_path: PathBuf,
    pub model_map_path: PathBuf,
    pub api_key: Option<String>,
    pub node_bin: String,
}

impl CursorAgentSdkSource {
    pub fn from_env() -> Self {
        let helper_path = env::var(CURSOR_HELPER_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_HELPER_REL));
        let model_map_path = env::var("CURSOR_MODEL_MAP")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_MODEL_MAP_REL));
        let api_key = env::var(CURSOR_API_KEY_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty());
        Self {
            helper_path,
            model_map_path,
            api_key,
            node_bin: env::var("NODE_BIN").unwrap_or_else(|_| "node".into()),
        }
    }

    pub fn new(helper_path: impl Into<PathBuf>, api_key: Option<String>) -> Self {
        Self {
            helper_path: helper_path.into(),
            model_map_path: PathBuf::from(DEFAULT_MODEL_MAP_REL),
            api_key,
            node_bin: "node".into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ModelMapFile {
    tiers: ModelMapTiers,
}

#[derive(Debug, Deserialize)]
struct ModelMapTiers {
    #[serde(rename = "T-small")]
    t_small: Option<ModelMapEntry>,
    #[serde(rename = "T-mid")]
    t_mid: Option<ModelMapEntry>,
    #[serde(rename = "T-frontier")]
    t_frontier: Option<ModelMapEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelMapEntry {
    cursor_model_id: String,
    #[serde(default)]
    label: Option<String>,
}

fn load_model_map(path: &Path) -> Vec<ModelInfo> {
    let Ok(raw) = fs::read_to_string(path) else {
        return placeholder_models();
    };
    let Ok(file) = toml::from_str::<ModelMapFile>(&raw) else {
        return placeholder_models();
    };
    let mut out = Vec::new();
    if let Some(e) = file.tiers.t_small {
        out.push(ModelInfo {
            id: e.cursor_model_id,
            label: e.label.or_else(|| Some("T-small map".into())),
            tier: Some(ModelTier::Small),
        });
    }
    if let Some(e) = file.tiers.t_mid {
        out.push(ModelInfo {
            id: e.cursor_model_id,
            label: e.label.or_else(|| Some("T-mid map".into())),
            tier: Some(ModelTier::Mid),
        });
    }
    if let Some(e) = file.tiers.t_frontier {
        out.push(ModelInfo {
            id: e.cursor_model_id,
            label: e.label.or_else(|| Some("T-frontier map".into())),
            tier: Some(ModelTier::Frontier),
        });
    }
    if out.is_empty() {
        placeholder_models()
    } else {
        out
    }
}

fn placeholder_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "composer-2.5".into(),
            label: Some("placeholder until Cursor.models.list".into()),
            tier: Some(ModelTier::Mid),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn list_models_reads_model_map_toml() {
        let src = CursorAgentSdkSource {
            helper_path: PathBuf::from("scripts/cursor_agent_complete.mjs"),
            model_map_path: PathBuf::from("config/model-map.toml"),
            api_key: None,
            node_bin: "node".into(),
        };
        let models = src.list_models().unwrap();
        assert!(models.len() >= 3);
        assert!(models.iter().any(|m| m.tier == Some(ModelTier::Small)));
        assert!(models.iter().any(|m| m.tier == Some(ModelTier::Frontier)));
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
        Ok(load_model_map(&self.model_map_path))
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
                 (docs/model-source.md). Bridge TODO: Connect → cursor-sdk-bridge.",
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
                    "sdk": "Agent.create({ apiKey, model:{id}, local:{cwd} }) + send"
                }))
            }),
        })
    }
}

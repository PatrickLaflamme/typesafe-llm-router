//! Cursor Agent model source via Node sidecar (`@cursor/sdk`).
//!
//! No first-party Rust SDK — see https://cursor.com/docs/sdk/bridge and
//! `scripts/cursor_agent_complete.mjs`. Env: `CURSOR_API_KEY` only (no secrets in repo).

use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::{
    CompleteRequest, CompleteResponse, ModelInfo, ModelSource, ModelRuntime, UsageMeta,
};
use crate::error::ModelSourceError;

pub const CURSOR_API_KEY_ENV: &str = "CURSOR_API_KEY";
pub const CURSOR_HELPER_ENV: &str = "CURSOR_AGENT_HELPER";
pub const DEFAULT_HELPER_REL: &str = "scripts/cursor_agent_complete.mjs";

/// Alias matching Typesafe Router design note.
pub type CursorAgentSdkSource = CursorAgentModelSource;

/// Thin adapter: spawn Node helper that calls `Agent.create` + `send`.
///
/// TODO: optional Connect client against `cursor-sdk-bridge` once binaries are pinned.
#[derive(Debug, Clone)]
pub struct CursorAgentModelSource {
    pub helper_path: PathBuf,
    pub api_key: Option<String>,
    pub node_bin: String,
}

impl CursorAgentModelSource {
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

impl ModelSource for CursorAgentModelSource {
    fn name(&self) -> &'static str {
        "cursor-agent"
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ModelSourceError> {
        // Placeholders until Cursor.models.list via helper/bridge.
        Ok(load_model_map_placeholders())
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
                }))
            }),
        })
    }
}

fn load_model_map_placeholders() -> Vec<ModelInfo> {
    use crate::catalog::ModelTier;
    vec![
        ModelInfo {
            id: "composer-2.5".into(),
            label: Some("Cursor composer (map: T-mid)".into()),
            tier: Some(ModelTier::Mid),
        },
        ModelInfo {
            id: "gpt-5".into(),
            label: Some("placeholder until Cursor.models.list".into()),
            tier: Some(ModelTier::Frontier),
        },
    ]
}

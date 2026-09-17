//! Databricks Unity AI Gateway model provider.
//!
//! Calls the OpenAI-compatible chat completions API:
//! `POST {host}/ai-gateway/mlflow/v1/chat/completions`
//!
//! **Auth (preferred):** Databricks CLI after `databricks auth login`
//! - Host from `databricks auth env` (`DATABRICKS_HOST`)
//! - Access token + expiry from `databricks auth token`
//!   (`access_token`, `expiry`) — refreshed when close to expiry
//!
//! Optional external **model provider service** path:
//! `POST …/ai-gateway/openai/v1/chat/completions` with header
//! `Databricks-Model-Provider-Service: <catalog.schema.service>`.
//!
//! Docs:
//! - <https://docs.databricks.com/aws/en/ai-gateway/query-model-services>
//! - <https://docs.databricks.com/aws/en/dev-tools/cli/reference/auth-commands>

use std::env;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use super::{CompleteRequest, CompleteResponse, ModelInfo, ModelProvider, UsageMeta};
use crate::catalog::ModelTier;
use crate::error::ModelProviderError;
use crate::types::{MessageRole, SessionMessage};

/// Optional Databricks CLI profile (`-p` / `DATABRICKS_CONFIG_PROFILE`).
pub const DATABRICKS_CONFIG_PROFILE_ENV: &str = "DATABRICKS_CONFIG_PROFILE";
/// Override path to the `databricks` binary (default: `databricks` on `PATH`).
pub const DATABRICKS_CLI_ENV: &str = "DATABRICKS_CLI";
/// Optional JSON array of [`ModelInfo`], or comma-separated model service FQNs.
pub const DATABRICKS_MODELS_ENV: &str = "DATABRICKS_AI_GATEWAY_MODELS";
/// When set, use the OpenAI managed path + this Unity Catalog provider-service name.
pub const DATABRICKS_MODEL_PROVIDER_SERVICE_ENV: &str = "DATABRICKS_MODEL_PROVIDER_SERVICE";

/// @deprecated Host/token come from the Databricks CLI; kept for docs / older call sites.
pub const DATABRICKS_HOST_ENV: &str = "DATABRICKS_HOST";
/// @deprecated Prefer `databricks auth token` after `databricks auth login`.
pub const DATABRICKS_TOKEN_ENV: &str = "DATABRICKS_TOKEN";

const MLFLOW_CHAT_PATH: &str = "/ai-gateway/mlflow/v1/chat/completions";
const OPENAI_CHAT_PATH: &str = "/ai-gateway/openai/v1/chat/completions";
/// Refresh the CLI token this many seconds before `expiry`.
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(120);

/// Which Unity AI Gateway surface to call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DatabricksGatewayPath {
    /// Databricks-hosted / unified model services (`/ai-gateway/mlflow/v1`).
    #[default]
    ModelService,
    /// External model provider services (`/ai-gateway/openai/v1` + header).
    ModelProviderService,
}

/// Injectable HTTP transport so unit/integration tests need no live workspace.
pub trait DatabricksGatewayTransport: Send + Sync {
    fn post_chat_completions(
        &self,
        url: &str,
        headers: HeaderMap,
        body: &ChatCompletionsRequest,
    ) -> Result<(u16, String), ModelProviderError>;
}

/// Runs Databricks CLI subcommands (`auth env`, `auth token`, …).
pub trait DatabricksCliRunner: Send + Sync {
    fn run(&self, args: &[&str]) -> Result<String, ModelProviderError>;
}

/// Production transport using `reqwest` blocking client.
#[derive(Debug, Clone)]
pub struct ReqwestDatabricksTransport {
    http: Client,
}

impl ReqwestDatabricksTransport {
    pub fn new() -> Result<Self, ModelProviderError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| ModelProviderError::DatabricksHttp(e.to_string()))?;
        Ok(Self { http })
    }
}

impl Default for ReqwestDatabricksTransport {
    fn default() -> Self {
        Self::new().expect("reqwest client builder")
    }
}

impl DatabricksGatewayTransport for ReqwestDatabricksTransport {
    fn post_chat_completions(
        &self,
        url: &str,
        headers: HeaderMap,
        body: &ChatCompletionsRequest,
    ) -> Result<(u16, String), ModelProviderError> {
        let response = self
            .http
            .post(url)
            .headers(headers)
            .json(body)
            .send()
            .map_err(|e| ModelProviderError::DatabricksHttp(e.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|e| ModelProviderError::DatabricksHttp(e.to_string()))?;
        Ok((status, text))
    }
}

/// Shell out to the `databricks` binary on `PATH` (or `DATABRICKS_CLI`).
#[derive(Debug, Clone)]
pub struct ProcessDatabricksCli {
    pub bin: String,
}

impl ProcessDatabricksCli {
    pub fn from_env() -> Self {
        Self {
            bin: env::var(DATABRICKS_CLI_ENV).unwrap_or_else(|_| "databricks".into()),
        }
    }
}

impl Default for ProcessDatabricksCli {
    fn default() -> Self {
        Self::from_env()
    }
}

impl DatabricksCliRunner for ProcessDatabricksCli {
    fn run(&self, args: &[&str]) -> Result<String, ModelProviderError> {
        let output = Command::new(&self.bin).args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ModelProviderError::DatabricksCli(format!(
                    "`{}` not found on PATH — install the Databricks CLI and run \
                     `databricks auth login` (or set {DATABRICKS_CLI_ENV})",
                    self.bin
                ))
            } else {
                ModelProviderError::DatabricksCli(format!(
                    "failed to spawn `{} {}`: {e}",
                    self.bin,
                    args.join(" ")
                ))
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else {
                stdout.trim().to_string()
            };
            return Err(ModelProviderError::DatabricksCli(format!(
                "`{} {}` failed (exit {}): {detail}\n\
                 Hint: run `databricks auth login` then retry",
                self.bin,
                args.join(" "),
                output.status
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Credentials sourced from the Databricks CLI (`auth login` session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabricksCliCredentials {
    pub host: String,
    pub access_token: String,
    /// Token expiry from `databricks auth token` (`expiry` field), when present.
    pub expiry: Option<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthEnvOutput {
    env: AuthEnvVars,
}

#[derive(Debug, Deserialize)]
struct AuthEnvVars {
    #[serde(rename = "DATABRICKS_HOST")]
    host: Option<String>,
    #[serde(rename = "DATABRICKS_CONFIG_PROFILE")]
    profile: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthTokenOutput {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expiry: Option<String>,
}

/// Load host + token (+ expiry) via `databricks auth env` and `databricks auth token`.
///
/// Prerequisite: `databricks auth login` (U2M) for the chosen profile.
pub fn load_credentials_from_cli(
    cli: &dyn DatabricksCliRunner,
    profile: Option<&str>,
) -> Result<DatabricksCliCredentials, ModelProviderError> {
    let host = load_host_from_cli(cli, profile)?;
    let token = load_token_from_cli(cli, profile)?;
    Ok(DatabricksCliCredentials {
        host: normalize_host(host),
        access_token: token.access_token,
        expiry: token.expiry,
        profile: profile.map(str::to_string),
    })
}

fn profile_args(profile: Option<&str>) -> Vec<&str> {
    match profile {
        Some(p) if !p.is_empty() => vec!["--profile", p],
        _ => Vec::new(),
    }
}

fn load_host_from_cli(
    cli: &dyn DatabricksCliRunner,
    profile: Option<&str>,
) -> Result<String, ModelProviderError> {
    let mut args = vec!["auth", "env"];
    args.extend(profile_args(profile));
    let stdout = cli.run(&args)?;
    let parsed: AuthEnvOutput = serde_json::from_str(stdout.trim()).map_err(|e| {
        ModelProviderError::DatabricksCli(format!(
            "invalid `databricks auth env` JSON ({e}): {}",
            stdout.trim()
        ))
    })?;
    let host = parsed
        .env
        .host
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            ModelProviderError::DatabricksCli(
                "`databricks auth env` did not include DATABRICKS_HOST — run `databricks auth login`"
                    .into(),
            )
        })?;
    let _ = parsed.env.profile;
    Ok(host)
}

fn load_token_from_cli(
    cli: &dyn DatabricksCliRunner,
    profile: Option<&str>,
) -> Result<AuthTokenOutput, ModelProviderError> {
    let mut args = vec!["auth", "token"];
    args.extend(profile_args(profile));
    let stdout = cli.run(&args)?;
    let parsed: AuthTokenOutput = serde_json::from_str(stdout.trim()).map_err(|e| {
        ModelProviderError::DatabricksCli(format!(
            "invalid `databricks auth token` JSON ({e}): {}",
            stdout.trim()
        ))
    })?;
    if parsed.access_token.trim().is_empty() {
        return Err(ModelProviderError::DatabricksCli(
            "`databricks auth token` returned an empty access_token — run `databricks auth login`"
                .into(),
        ));
    }
    let _ = parsed.token_type;
    Ok(parsed)
}

fn token_needs_refresh(expiry: Option<&str>, now: SystemTime) -> bool {
    let Some(expiry) = expiry else {
        // No expiry from CLI — refresh each time so U2M stays valid.
        return true;
    };
    match parse_rfc3339_to_system_time(expiry) {
        Some(exp) => now + TOKEN_REFRESH_SKEW >= exp,
        None => true,
    }
}

/// Minimal RFC3339 / ISO-8601 parser for CLI `expiry` values (`…Z` or with offset).
fn parse_rfc3339_to_system_time(s: &str) -> Option<SystemTime> {
    let s = s.trim();
    let (datetime, offset_secs) =
        if let Some(rest) = s.strip_suffix('Z').or_else(|| s.strip_suffix('z')) {
            (rest, 0_i64)
        } else {
            // Split timezone offset at the last '+' or '-' after the date (`T` separator).
            let t_pos = s.find('T').or_else(|| s.find(' '))?;
            let after_t = &s[t_pos + 1..];
            let off_rel = after_t.rfind(['+', '-'])?;
            let abs = t_pos + 1 + off_rel;
            let (datetime, off) = s.split_at(abs);
            let sign = if off.starts_with('+') { 1_i64 } else { -1_i64 };
            let off = off.trim_start_matches(['+', '-']);
            let mut parts = off.split(':');
            let hh: i64 = parts.next()?.parse().ok()?;
            let mm: i64 = parts.next().unwrap_or("0").parse().ok()?;
            (datetime, sign * (hh * 3600 + mm * 60))
        };

    let datetime = datetime.split('.').next()?; // drop fractional seconds
    let (date, time) = datetime
        .split_once('T')
        .or_else(|| datetime.split_once(' '))?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;
    let mut t = time.split(':');
    let hour: u32 = t.next()?.parse().ok()?;
    let minute: u32 = t.next()?.parse().ok()?;
    let second: u32 = t.next().unwrap_or("0").parse().ok()?;

    let days = days_from_civil(year, month, day)?;
    let secs = days * 86400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second)
        - offset_secs;
    if secs < 0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_secs(secs as u64))
}

/// Civil date → days since Unix epoch (Howard Hinnant algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp as i64 + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// Databricks AI Gateway [`ModelProvider`].
#[derive(Clone)]
pub struct DatabricksAiGatewayProvider {
    pub host: String,
    pub path: DatabricksGatewayPath,
    /// Unity Catalog name for [`DatabricksGatewayPath::ModelProviderService`].
    pub model_provider_service: Option<String>,
    pub models: Vec<ModelInfo>,
    pub profile: Option<String>,
    credentials: Arc<Mutex<DatabricksCliCredentials>>,
    cli: Arc<dyn DatabricksCliRunner>,
    transport: Arc<dyn DatabricksGatewayTransport>,
}

impl std::fmt::Debug for DatabricksAiGatewayProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabricksAiGatewayProvider")
            .field("host", &self.host)
            .field("path", &self.path)
            .field("model_provider_service", &self.model_provider_service)
            .field("models", &self.models)
            .field("profile", &self.profile)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl DatabricksAiGatewayProvider {
    /// Build using the Databricks CLI (`databricks auth login` session).
    ///
    /// Reads optional `DATABRICKS_CONFIG_PROFILE`, model catalog, and provider-service
    /// overrides from the environment. Host and token always come from the CLI.
    pub fn from_cli() -> Result<Self, ModelProviderError> {
        let profile = env::var(DATABRICKS_CONFIG_PROFILE_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty());
        let cli = Arc::new(ProcessDatabricksCli::from_env());
        Self::from_cli_with(cli, profile, Arc::new(ReqwestDatabricksTransport::new()?))
    }

    /// Like [`Self::from_cli`] with injectable CLI + HTTP (tests).
    pub fn from_cli_with(
        cli: Arc<dyn DatabricksCliRunner>,
        profile: Option<String>,
        transport: Arc<dyn DatabricksGatewayTransport>,
    ) -> Result<Self, ModelProviderError> {
        let creds = load_credentials_from_cli(cli.as_ref(), profile.as_deref())?;
        let model_provider_service = env::var(DATABRICKS_MODEL_PROVIDER_SERVICE_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty());
        let path = if model_provider_service.is_some() {
            DatabricksGatewayPath::ModelProviderService
        } else {
            DatabricksGatewayPath::ModelService
        };
        let models = models_from_env().unwrap_or_else(default_databricks_catalog);
        Self::from_credentials(
            creds,
            models,
            path,
            model_provider_service,
            profile,
            cli,
            transport,
        )
    }

    /// @deprecated Prefer [`Self::from_cli`] — host/token are sourced from the Databricks CLI.
    pub fn from_env() -> Result<Self, ModelProviderError> {
        Self::from_cli()
    }

    /// Construct with already-known credentials (unit / integration tests).
    pub fn new(
        host: impl Into<String>,
        token: impl Into<String>,
        models: Vec<ModelInfo>,
        path: DatabricksGatewayPath,
        model_provider_service: Option<String>,
        transport: Arc<dyn DatabricksGatewayTransport>,
    ) -> Result<Self, ModelProviderError> {
        let host = normalize_host(host.into());
        let creds = DatabricksCliCredentials {
            host: host.clone(),
            access_token: token.into(),
            expiry: None,
            profile: None,
        };
        // Static-token tests: CLI runner is unused because expiry is None but we
        // short-circuit refresh when cli is a no-op stub — use StaticCli below.
        let cli: Arc<dyn DatabricksCliRunner> = Arc::new(StaticTokenCli {
            host: host.clone(),
            token: creds.access_token.clone(),
            expiry: None,
        });
        Self::from_credentials(
            creds,
            models,
            path,
            model_provider_service,
            None,
            cli,
            transport,
        )
    }

    fn from_credentials(
        creds: DatabricksCliCredentials,
        models: Vec<ModelInfo>,
        path: DatabricksGatewayPath,
        model_provider_service: Option<String>,
        profile: Option<String>,
        cli: Arc<dyn DatabricksCliRunner>,
        transport: Arc<dyn DatabricksGatewayTransport>,
    ) -> Result<Self, ModelProviderError> {
        if models.is_empty() {
            return Err(ModelProviderError::Other(
                "DatabricksAiGatewayProvider requires at least one model in list_models()".into(),
            ));
        }
        if path == DatabricksGatewayPath::ModelProviderService
            && model_provider_service
                .as_ref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(ModelProviderError::Other(
                "ModelProviderService path requires DATABRICKS_MODEL_PROVIDER_SERVICE".into(),
            ));
        }
        Ok(Self {
            host: creds.host.clone(),
            path,
            model_provider_service,
            models,
            profile,
            credentials: Arc::new(Mutex::new(creds)),
            cli,
            transport,
        })
    }

    /// Chat completions URL for the configured gateway path.
    pub fn chat_completions_url(&self) -> String {
        let suffix = match self.path {
            DatabricksGatewayPath::ModelService => MLFLOW_CHAT_PATH,
            DatabricksGatewayPath::ModelProviderService => OPENAI_CHAT_PATH,
        };
        format!("{}{suffix}", self.host)
    }

    /// Current access token, refreshing via `databricks auth token` when expiry is near.
    pub fn bearer_token(&self) -> Result<String, ModelProviderError> {
        let mut guard = self.credentials.lock().map_err(|_| {
            ModelProviderError::Other("Databricks credentials lock poisoned".into())
        })?;
        if token_needs_refresh(guard.expiry.as_deref(), SystemTime::now()) {
            let refreshed = load_token_from_cli(self.cli.as_ref(), self.profile.as_deref())?;
            guard.access_token = refreshed.access_token;
            guard.expiry = refreshed.expiry;
        }
        Ok(guard.access_token.clone())
    }

    /// Cached token expiry string from the last CLI `auth token` call, if any.
    pub fn token_expiry(&self) -> Result<Option<String>, ModelProviderError> {
        let guard = self.credentials.lock().map_err(|_| {
            ModelProviderError::Other("Databricks credentials lock poisoned".into())
        })?;
        Ok(guard.expiry.clone())
    }

    fn auth_headers(&self) -> Result<HeaderMap, ModelProviderError> {
        let token = self.bearer_token()?;
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| ModelProviderError::Other(format!("invalid token header: {e}")))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(svc) = &self.model_provider_service {
            let name = HeaderName::from_static("databricks-model-provider-service");
            headers.insert(
                name,
                HeaderValue::from_str(svc).map_err(|e| {
                    ModelProviderError::Other(format!(
                        "invalid Databricks-Model-Provider-Service header: {e}"
                    ))
                })?,
            );
        }
        Ok(headers)
    }
}

/// CLI stub that always returns the same host/token (used by [`DatabricksAiGatewayProvider::new`]).
struct StaticTokenCli {
    host: String,
    token: String,
    expiry: Option<String>,
}

impl DatabricksCliRunner for StaticTokenCli {
    fn run(&self, args: &[&str]) -> Result<String, ModelProviderError> {
        if args.get(0..2) == Some(&["auth", "env"]) {
            return Ok(serde_json::json!({
                "env": { "DATABRICKS_HOST": self.host }
            })
            .to_string());
        }
        if args.get(0..2) == Some(&["auth", "token"]) {
            return Ok(serde_json::json!({
                "access_token": self.token,
                "token_type": "Bearer",
                "expiry": self.expiry,
            })
            .to_string());
        }
        Err(ModelProviderError::DatabricksCli(format!(
            "StaticTokenCli unexpected args: {args:?}"
        )))
    }
}

impl ModelProvider for DatabricksAiGatewayProvider {
    fn name(&self) -> &'static str {
        "databricks-ai-gateway"
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ModelProviderError> {
        Ok(self.models.clone())
    }

    fn complete(&self, req: &CompleteRequest) -> Result<CompleteResponse, ModelProviderError> {
        let messages = chat_messages_from_request(req);
        let max_tokens = req
            .model_params
            .as_ref()
            .and_then(|v| v.get("max_tokens"))
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .or(Some(1024));
        let temperature = req
            .model_params
            .as_ref()
            .and_then(|v| v.get("temperature"))
            .and_then(|v| v.as_f64());

        let body = ChatCompletionsRequest {
            model: req.model_id.clone(),
            messages,
            max_tokens,
            temperature,
            stream: Some(false),
        };

        let url = self.chat_completions_url();
        let headers = self.auth_headers()?;
        let (status, text) = self.transport.post_chat_completions(&url, headers, &body)?;

        if !(200..300).contains(&status) {
            return Err(ModelProviderError::DatabricksApi { status, body: text });
        }

        let parsed: ChatCompletionsResponse = serde_json::from_str(&text).map_err(|e| {
            ModelProviderError::Other(format!(
                "invalid Databricks chat completions JSON ({e}): {text}"
            ))
        })?;

        let model_output = parsed
            .choices
            .first()
            .and_then(|c| c.message.as_ref())
            .map(|m| m.content.clone())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ModelProviderError::Other(
                    "Databricks response had no choices[0].message.content".into(),
                )
            })?;

        Ok(CompleteResponse {
            model_output,
            source: self.name().to_string(),
            model_id: req.model_id.clone(),
            run_id: parsed.id.clone(),
            usage: parsed.usage.map(|u| UsageMeta {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            }),
            raw_meta: Some(serde_json::json!({
                "gateway": "databricks-ai-gateway",
                "auth": "databricks-cli",
                "path": match self.path {
                    DatabricksGatewayPath::ModelService => "mlflow",
                    DatabricksGatewayPath::ModelProviderService => "openai",
                },
                "url": url,
                "response_id": parsed.id,
                "model_provider_service": self.model_provider_service,
                "profile": self.profile,
                "token_expiry": self.token_expiry().ok().flatten(),
            })),
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatCompletionsResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatChoice {
    #[serde(default)]
    message: Option<ChatMessage>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
}

fn normalize_host(host: String) -> String {
    let host = host.trim().trim_end_matches('/').to_string();
    if host.starts_with("http://") || host.starts_with("https://") {
        host
    } else {
        format!("https://{host}")
    }
}

/// Default lab catalog — Databricks foundation model service FQNs.
///
/// Prices are illustrative placeholders for Choice cost notes (refresh from
/// Databricks / foundation-model pricing when binding a real workspace).
pub fn default_databricks_catalog() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "system.ai.databricks-gpt-oss-120b".into(),
            label: Some("Databricks GPT OSS 120B (model service)".into()),
            price_input_per_mtok: 0.10,
            price_output_per_mtok: 0.40,
            price_cache_read_per_mtok: None,
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Small),
        },
        ModelInfo {
            id: "system.ai.claude-sonnet-4-5".into(),
            label: Some("Claude Sonnet 4.5 via AI Gateway".into()),
            price_input_per_mtok: 3.00,
            price_output_per_mtok: 15.00,
            price_cache_read_per_mtok: Some(0.30),
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Mid),
        },
        ModelInfo {
            id: "system.ai.claude-opus-4-6".into(),
            label: Some("Claude Opus 4.6 via AI Gateway".into()),
            price_input_per_mtok: 15.00,
            price_output_per_mtok: 75.00,
            price_cache_read_per_mtok: Some(1.50),
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Frontier),
        },
    ]
}

fn models_from_env() -> Option<Vec<ModelInfo>> {
    let raw = env::var(DATABRICKS_MODELS_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('[') {
        return serde_json::from_str(trimmed).ok();
    }
    let models = trimmed
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|id| ModelInfo {
            id: id.to_string(),
            label: Some(format!("DATABRICKS_AI_GATEWAY_MODELS · {id}")),
            price_input_per_mtok: 1.0,
            price_output_per_mtok: 3.0,
            price_cache_read_per_mtok: None,
            price_cache_write_per_mtok: None,
            tier_hint: None,
        })
        .collect::<Vec<_>>();
    if models.is_empty() {
        None
    } else {
        Some(models)
    }
}

/// Build OpenAI-style chat messages from a complete request.
pub fn chat_messages_from_request(req: &CompleteRequest) -> Vec<ChatMessage> {
    if let Some(messages) = &req.messages {
        if !messages.is_empty() {
            return messages.iter().map(session_to_chat).collect();
        }
    }
    vec![ChatMessage {
        role: "user".into(),
        content: req.prompt.clone(),
    }]
}

fn session_to_chat(m: &SessionMessage) -> ChatMessage {
    let role = match m.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "user",
    };
    ChatMessage {
        role: role.into(),
        content: m.content.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockTransport {
        last_url: Mutex<Option<String>>,
        last_body: Mutex<Option<ChatCompletionsRequest>>,
        last_headers: Mutex<Option<HeaderMap>>,
        response: Mutex<(u16, String)>,
    }

    impl MockTransport {
        fn ok_json(body: &str) -> Self {
            Self {
                last_url: Mutex::new(None),
                last_body: Mutex::new(None),
                last_headers: Mutex::new(None),
                response: Mutex::new((200, body.to_string())),
            }
        }
    }

    impl DatabricksGatewayTransport for MockTransport {
        fn post_chat_completions(
            &self,
            url: &str,
            headers: HeaderMap,
            body: &ChatCompletionsRequest,
        ) -> Result<(u16, String), ModelProviderError> {
            *self.last_url.lock().unwrap() = Some(url.to_string());
            *self.last_body.lock().unwrap() = Some(body.clone());
            *self.last_headers.lock().unwrap() = Some(headers);
            Ok(self.response.lock().unwrap().clone())
        }
    }

    struct ScriptedCli {
        host: String,
        tokens: Mutex<Vec<(String, Option<String>)>>,
        auth_env_calls: AtomicUsize,
        auth_token_calls: AtomicUsize,
    }

    impl ScriptedCli {
        fn new(host: &str, tokens: Vec<(String, Option<String>)>) -> Self {
            Self {
                host: host.into(),
                tokens: Mutex::new(tokens),
                auth_env_calls: AtomicUsize::new(0),
                auth_token_calls: AtomicUsize::new(0),
            }
        }
    }

    impl DatabricksCliRunner for ScriptedCli {
        fn run(&self, args: &[&str]) -> Result<String, ModelProviderError> {
            if args.get(0..2) == Some(&["auth", "env"]) {
                self.auth_env_calls.fetch_add(1, Ordering::SeqCst);
                return Ok(serde_json::json!({
                    "env": {
                        "DATABRICKS_HOST": self.host,
                        "DATABRICKS_CONFIG_PROFILE": "lab"
                    }
                })
                .to_string());
            }
            if args.get(0..2) == Some(&["auth", "token"]) {
                self.auth_token_calls.fetch_add(1, Ordering::SeqCst);
                let mut tokens = self.tokens.lock().unwrap();
                if tokens.is_empty() {
                    return Err(ModelProviderError::DatabricksCli(
                        "no scripted tokens".into(),
                    ));
                }
                let (token, expiry) = tokens.remove(0);
                return Ok(serde_json::json!({
                    "access_token": token,
                    "token_type": "Bearer",
                    "expiry": expiry,
                })
                .to_string());
            }
            Err(ModelProviderError::DatabricksCli(format!(
                "unexpected {args:?}"
            )))
        }
    }

    #[test]
    fn normalize_host_adds_https() {
        assert_eq!(
            normalize_host("adb-123.azuredatabricks.net".into()),
            "https://adb-123.azuredatabricks.net"
        );
        assert_eq!(
            normalize_host("https://adb-123.azuredatabricks.net/".into()),
            "https://adb-123.azuredatabricks.net"
        );
    }

    #[test]
    fn parse_expiry_zulu() {
        let t = parse_rfc3339_to_system_time("2020-01-01T00:00:00Z").unwrap();
        assert_eq!(
            t.duration_since(UNIX_EPOCH).unwrap().as_secs(),
            1_577_836_800
        );
    }

    #[test]
    fn token_needs_refresh_when_expiry_missing_or_near() {
        assert!(token_needs_refresh(None, SystemTime::now()));
        let past = "2020-01-01T00:00:00Z";
        assert!(token_needs_refresh(Some(past), SystemTime::now()));
        // Far-future expiry should not refresh.
        let future = "2099-01-01T00:00:00Z";
        assert!(!token_needs_refresh(Some(future), SystemTime::now()));
    }

    #[test]
    fn load_credentials_from_cli_reads_host_token_and_expiry() {
        let cli = ScriptedCli::new(
            "https://example.databricks.com",
            vec![("tok-1".into(), Some("2099-01-01T00:00:00Z".into()))],
        );
        let creds = load_credentials_from_cli(&cli, Some("lab")).unwrap();
        assert_eq!(creds.host, "https://example.databricks.com");
        assert_eq!(creds.access_token, "tok-1");
        assert_eq!(creds.expiry.as_deref(), Some("2099-01-01T00:00:00Z"));
        assert_eq!(cli.auth_env_calls.load(Ordering::SeqCst), 1);
        assert_eq!(cli.auth_token_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn from_cli_with_uses_cli_host_and_refreshes_near_expiry() {
        let cli = Arc::new(ScriptedCli::new(
            "https://example.databricks.com",
            vec![
                // Initial load — already stale so next bearer_token refreshes.
                ("tok-old".into(), Some("2020-01-01T00:00:00Z".into())),
                ("tok-new".into(), Some("2099-06-01T12:00:00Z".into())),
            ],
        ));
        let transport = Arc::new(MockTransport::ok_json(
            r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}]}"#,
        ));
        let provider = DatabricksAiGatewayProvider::from_cli_with(
            cli.clone(),
            Some("lab".into()),
            transport.clone(),
        )
        .unwrap();

        assert_eq!(provider.host, "https://example.databricks.com");
        assert_eq!(provider.profile.as_deref(), Some("lab"));
        // Initial load consumed tok-old; bearer_token should refresh to tok-new.
        assert_eq!(provider.bearer_token().unwrap(), "tok-new");
        assert_eq!(
            provider.token_expiry().unwrap().as_deref(),
            Some("2099-06-01T12:00:00Z")
        );
        assert_eq!(cli.auth_token_calls.load(Ordering::SeqCst), 2);

        provider
            .complete(&CompleteRequest {
                model_id: "system.ai.claude-sonnet-4-5".into(),
                prompt: "hi".into(),
                messages: None,
                cwd: None,
                runtime: Default::default(),
                cloud_repos: None,
                model_params: None,
                timeout_ms: None,
            })
            .unwrap();

        let headers = transport.last_headers.lock().unwrap().clone().unwrap();
        assert_eq!(
            headers.get(AUTHORIZATION).unwrap().to_str().unwrap(),
            "Bearer tok-new"
        );
    }

    #[test]
    fn chat_url_mlflow_vs_openai() {
        let transport = Arc::new(MockTransport::ok_json("{}"));
        let mlflow = DatabricksAiGatewayProvider::new(
            "https://example.databricks.com",
            "tok",
            default_databricks_catalog(),
            DatabricksGatewayPath::ModelService,
            None,
            transport.clone(),
        )
        .unwrap();
        assert!(mlflow.chat_completions_url().ends_with(MLFLOW_CHAT_PATH));

        let openai = DatabricksAiGatewayProvider::new(
            "https://example.databricks.com",
            "tok",
            default_databricks_catalog(),
            DatabricksGatewayPath::ModelProviderService,
            Some("main.default.openai_prod".into()),
            transport,
        )
        .unwrap();
        assert!(openai.chat_completions_url().ends_with(OPENAI_CHAT_PATH));
    }

    #[test]
    fn complete_posts_openai_compatible_body_and_parses_content() {
        let transport = Arc::new(MockTransport::ok_json(
            r#"{
              "id": "chatcmpl-test",
              "choices": [{"message": {"role": "assistant", "content": "billing"}}],
              "usage": {"prompt_tokens": 12, "completion_tokens": 1}
            }"#,
        ));
        let provider = DatabricksAiGatewayProvider::new(
            "https://example.databricks.com",
            "secret-token",
            default_databricks_catalog(),
            DatabricksGatewayPath::ModelService,
            None,
            transport.clone(),
        )
        .unwrap();

        let out = provider
            .complete(&CompleteRequest {
                model_id: "system.ai.claude-sonnet-4-5".into(),
                prompt: "[system]\nlabel\n\n[user]\nticket".into(),
                messages: Some(vec![
                    SessionMessage {
                        role: MessageRole::System,
                        content: "Reply with one label.".into(),
                    },
                    SessionMessage {
                        role: MessageRole::User,
                        content: "I was charged twice.".into(),
                    },
                ]),
                cwd: None,
                runtime: Default::default(),
                cloud_repos: None,
                model_params: Some(serde_json::json!({"max_tokens": 64, "temperature": 0.0})),
                timeout_ms: None,
            })
            .unwrap();

        assert_eq!(out.model_output, "billing");
        assert_eq!(out.source, "databricks-ai-gateway");
        assert_eq!(out.model_id, "system.ai.claude-sonnet-4-5");
        assert_eq!(out.run_id.as_deref(), Some("chatcmpl-test"));
        assert_eq!(out.usage.as_ref().unwrap().input_tokens, Some(12));
        assert_eq!(out.usage.as_ref().unwrap().output_tokens, Some(1));
        assert_eq!(out.raw_meta.as_ref().unwrap()["auth"], "databricks-cli");

        let body = transport.last_body.lock().unwrap().clone().unwrap();
        assert_eq!(body.model, "system.ai.claude-sonnet-4-5");
        assert_eq!(body.max_tokens, Some(64));
        assert_eq!(body.temperature, Some(0.0));
        assert_eq!(body.messages.len(), 2);
        assert_eq!(body.messages[0].role, "system");
        assert_eq!(body.messages[1].role, "user");

        let headers = transport.last_headers.lock().unwrap().clone().unwrap();
        assert_eq!(
            headers.get(AUTHORIZATION).unwrap().to_str().unwrap(),
            "Bearer secret-token"
        );
    }

    #[test]
    fn model_provider_service_sets_header() {
        let transport = Arc::new(MockTransport::ok_json(
            r#"{"choices":[{"message":{"role":"assistant","content":"hi"}}]}"#,
        ));
        let provider = DatabricksAiGatewayProvider::new(
            "https://example.databricks.com",
            "tok",
            vec![ModelInfo {
                id: "gpt-4o-mini".into(),
                label: None,
                price_input_per_mtok: 0.15,
                price_output_per_mtok: 0.60,
                price_cache_read_per_mtok: None,
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Small),
            }],
            DatabricksGatewayPath::ModelProviderService,
            Some("main.default.openai_prod".into()),
            transport.clone(),
        )
        .unwrap();

        provider
            .complete(&CompleteRequest {
                model_id: "gpt-4o-mini".into(),
                prompt: "Say hello".into(),
                messages: None,
                cwd: None,
                runtime: Default::default(),
                cloud_repos: None,
                model_params: None,
                timeout_ms: None,
            })
            .unwrap();

        let headers = transport.last_headers.lock().unwrap().clone().unwrap();
        assert_eq!(
            headers
                .get("databricks-model-provider-service")
                .unwrap()
                .to_str()
                .unwrap(),
            "main.default.openai_prod"
        );
        assert!(transport
            .last_url
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .ends_with(OPENAI_CHAT_PATH));
    }

    #[test]
    fn list_models_returns_default_catalog() {
        let transport = Arc::new(MockTransport::ok_json("{}"));
        let provider = DatabricksAiGatewayProvider::new(
            "https://example.databricks.com",
            "tok",
            default_databricks_catalog(),
            DatabricksGatewayPath::ModelService,
            None,
            transport,
        )
        .unwrap();
        let models = provider.list_models().unwrap();
        assert!(models.iter().any(|m| m.id == "system.ai.claude-sonnet-4-5"));
        assert_eq!(provider.name(), "databricks-ai-gateway");
    }

    #[test]
    fn api_error_surfaces_status_and_body() {
        let transport = Arc::new(MockTransport {
            last_url: Mutex::new(None),
            last_body: Mutex::new(None),
            last_headers: Mutex::new(None),
            response: Mutex::new((401, "unauthorized".into())),
        });
        let provider = DatabricksAiGatewayProvider::new(
            "https://example.databricks.com",
            "bad",
            default_databricks_catalog(),
            DatabricksGatewayPath::ModelService,
            None,
            transport,
        )
        .unwrap();
        let err = provider
            .complete(&CompleteRequest {
                model_id: "system.ai.claude-sonnet-4-5".into(),
                prompt: "hi".into(),
                messages: None,
                cwd: None,
                runtime: Default::default(),
                cloud_repos: None,
                model_params: None,
                timeout_ms: None,
            })
            .unwrap_err();
        match err {
            ModelProviderError::DatabricksApi { status, body } => {
                assert_eq!(status, 401);
                assert_eq!(body, "unauthorized");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

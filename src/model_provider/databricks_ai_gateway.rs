//! Databricks Unity AI Gateway model provider.
//!
//! Calls the OpenAI-compatible chat completions API:
//! `POST {DATABRICKS_HOST}/ai-gateway/mlflow/v1/chat/completions`
//!
//! Auth: `Authorization: Bearer $DATABRICKS_TOKEN` (workspace PAT / OAuth).
//! Model ids are fully-qualified model service names (e.g. `system.ai.claude-sonnet-4-5`).
//!
//! Optional external **model provider service** path:
//! `POST …/ai-gateway/openai/v1/chat/completions` with header
//! `Databricks-Model-Provider-Service: <catalog.schema.service>`.
//!
//! Docs:
//! - <https://docs.databricks.com/aws/en/ai-gateway/query-model-services>
//! - <https://docs.databricks.com/aws/en/ai-gateway/query-model-provider-services>

use std::env;
use std::sync::Arc;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use super::{CompleteRequest, CompleteResponse, ModelInfo, ModelProvider, UsageMeta};
use crate::catalog::ModelTier;
use crate::error::ModelProviderError;
use crate::types::{MessageRole, SessionMessage};

pub const DATABRICKS_HOST_ENV: &str = "DATABRICKS_HOST";
pub const DATABRICKS_TOKEN_ENV: &str = "DATABRICKS_TOKEN";
/// Optional JSON array of [`ModelInfo`], or comma-separated model service FQNs.
pub const DATABRICKS_MODELS_ENV: &str = "DATABRICKS_AI_GATEWAY_MODELS";
/// When set, use the OpenAI managed path + this Unity Catalog provider-service name.
pub const DATABRICKS_MODEL_PROVIDER_SERVICE_ENV: &str = "DATABRICKS_MODEL_PROVIDER_SERVICE";

const MLFLOW_CHAT_PATH: &str = "/ai-gateway/mlflow/v1/chat/completions";
const OPENAI_CHAT_PATH: &str = "/ai-gateway/openai/v1/chat/completions";

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

/// Databricks AI Gateway [`ModelProvider`].
#[derive(Clone)]
pub struct DatabricksAiGatewayProvider {
    pub host: String,
    pub token: String,
    pub path: DatabricksGatewayPath,
    /// Unity Catalog name for [`DatabricksGatewayPath::ModelProviderService`].
    pub model_provider_service: Option<String>,
    pub models: Vec<ModelInfo>,
    transport: Arc<dyn DatabricksGatewayTransport>,
}

impl std::fmt::Debug for DatabricksAiGatewayProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabricksAiGatewayProvider")
            .field("host", &self.host)
            .field("path", &self.path)
            .field("model_provider_service", &self.model_provider_service)
            .field("models", &self.models)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl DatabricksAiGatewayProvider {
    /// Build from env (`DATABRICKS_HOST`, `DATABRICKS_TOKEN`, optional models / provider service).
    pub fn from_env() -> Result<Self, ModelProviderError> {
        let host = env::var(DATABRICKS_HOST_ENV)
            .map_err(|_| ModelProviderError::MissingDatabricksCredentials)?;
        let token = env::var(DATABRICKS_TOKEN_ENV)
            .map_err(|_| ModelProviderError::MissingDatabricksCredentials)?;
        if host.trim().is_empty() || token.trim().is_empty() {
            return Err(ModelProviderError::MissingDatabricksCredentials);
        }
        let model_provider_service = env::var(DATABRICKS_MODEL_PROVIDER_SERVICE_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty());
        let path = if model_provider_service.is_some() {
            DatabricksGatewayPath::ModelProviderService
        } else {
            DatabricksGatewayPath::ModelService
        };
        let models = models_from_env().unwrap_or_else(default_databricks_catalog);
        Self::new(
            host,
            token,
            models,
            path,
            model_provider_service,
            Arc::new(ReqwestDatabricksTransport::new()?),
        )
    }

    pub fn new(
        host: impl Into<String>,
        token: impl Into<String>,
        models: Vec<ModelInfo>,
        path: DatabricksGatewayPath,
        model_provider_service: Option<String>,
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
            host: normalize_host(host.into()),
            token: token.into(),
            path,
            model_provider_service,
            models,
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

    fn auth_headers(&self) -> Result<HeaderMap, ModelProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.token))
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
                "path": match self.path {
                    DatabricksGatewayPath::ModelService => "mlflow",
                    DatabricksGatewayPath::ModelProviderService => "openai",
                },
                "url": url,
                "response_id": parsed.id,
                "model_provider_service": self.model_provider_service,
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
    // Fall back to a single user message with the flattened prompt.
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
        MessageRole::Tool => "user", // gateway chat API has no tool role in v0
    };
    ChatMessage {
        role: role.into(),
        content: m.content.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

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

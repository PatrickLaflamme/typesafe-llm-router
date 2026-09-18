//! Blocking HTTP client for TypeSafe System One.
//!
//! Endpoint (docs): `POST https://api.typesafe.ai/v1/systemone`
//! Auth: `Authorization: Bearer $TYPESAFE_API_KEY`

use std::env;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};

use super::api::{SystemOneRequest, SystemOneResponse};
use super::TypesafeClient;
use crate::error::TypesafeError;

/// Default API base URL from TypeSafe docs.
pub const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";

/// Environment variable holding the API key (no secrets in repo).
pub const API_KEY_ENV: &str = "TYPESAFE_API_KEY";

/// Optional override for the API host (proxies / staging).
pub const BASE_URL_ENV: &str = "TYPESAFE_BASE_URL";

pub struct HttpTypesafeClient {
    http: Client,
    base_url: String,
    api_key: String,
}

impl HttpTypesafeClient {
    /// Build from env: `TYPESAFE_API_KEY` (required), `TYPESAFE_BASE_URL` (optional).
    pub fn from_env() -> Result<Self, TypesafeError> {
        let api_key = env::var(API_KEY_ENV).map_err(|_| TypesafeError::MissingApiKey)?;
        if api_key.trim().is_empty() {
            return Err(TypesafeError::MissingApiKey);
        }
        let base_url = env::var(BASE_URL_ENV).unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::new(base_url, api_key)
    }

    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, TypesafeError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| TypesafeError::Http(e.to_string()))?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        })
    }
}

impl TypesafeClient for HttpTypesafeClient {
    fn system_one(&self, request: &SystemOneRequest) -> Result<SystemOneResponse, TypesafeError> {
        let url = format!("{}/v1/systemone", self.base_url);
        let response = self
            .http
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(CONTENT_TYPE, "application/json")
            .json(request)
            .send()
            .map_err(|e| TypesafeError::Http(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .map_err(|e| TypesafeError::Http(e.to_string()))?;

        if !status.is_success() {
            return Err(TypesafeError::Api {
                status: status.as_u16(),
                body,
            });
        }

        serde_json::from_str(&body).map_err(|e| TypesafeError::Serde(e.to_string()))
    }
}

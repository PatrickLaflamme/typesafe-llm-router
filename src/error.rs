//! Error types for the router and TypeSafe client.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("allowlist is empty")]
    EmptyAllowlist,

    #[error("model `{0}` is on the allowlist but missing from the cost/cache catalog")]
    MissingCatalogEntry(String),

    #[error("current_model `{0}` is not in the allowlist")]
    CurrentNotAllowlisted(String),

    #[error("TypeSafe client error: {0}")]
    Typesafe(#[from] TypesafeError),

    #[error("model provider error: {0}")]
    ModelProvider(#[from] ModelProviderError),

    #[error("invalid router decision: {0}")]
    InvalidDecision(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum TypesafeError {
    #[error("HTTP error: {0}")]
    Http(String),

    #[error("TypeSafe API returned HTTP {status}: {body}")]
    Api { status: u16, body: String },

    #[error("failed to serialize/deserialize TypeSafe payload: {0}")]
    Serde(String),

    #[error("missing or empty TYPESAFE_API_KEY")]
    MissingApiKey,

    #[error("unexpected answer shape: {0}")]
    UnexpectedAnswer(String),
}

#[derive(Debug, Error)]
pub enum ModelProviderError {
    #[error("missing or empty CURSOR_API_KEY")]
    MissingCursorApiKey,

    #[error("Cursor agent helper failed: {0}")]
    CursorHelper(String),

    #[error(
        "missing Databricks CLI auth — run `databricks auth login` \
         (optional profile via DATABRICKS_CONFIG_PROFILE)"
    )]
    MissingDatabricksCredentials,

    #[error("Databricks CLI error: {0}")]
    DatabricksCli(String),

    #[error("Databricks AI Gateway HTTP error: {0}")]
    DatabricksHttp(String),

    #[error("Databricks AI Gateway returned HTTP {status}: {body}")]
    DatabricksApi { status: u16, body: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

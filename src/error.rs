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

    #[error("model source error: {0}")]
    ModelSource(#[from] ModelSourceError),

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
pub enum ModelSourceError {
    #[error("missing or empty CURSOR_API_KEY")]
    MissingCursorApiKey,

    #[error("Cursor agent helper failed: {0}")]
    CursorHelper(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

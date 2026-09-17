//! TypeSafe System One client.
//!
//! Verified against <https://docs.typesafe.ai/api> (2026-09):
//! `POST https://api.typesafe.ai/v1/systemone` with Bearer auth.
//!
//! Auth via `TYPESAFE_API_KEY` (never commit secrets).

pub mod api;
mod http;
mod stub;

pub use http::{HttpTypesafeClient, API_KEY_ENV, BASE_URL_ENV, DEFAULT_BASE_URL};
pub use stub::StubTypesafeClient;

use crate::error::TypesafeError;
use api::{SystemOneRequest, SystemOneResponse};

/// Abstraction over the TypeSafe System One evaluation endpoint.
///
/// Implement this for real HTTP, stubs, or record/replay in tests.
pub trait TypesafeClient {
    fn system_one(&self, request: &SystemOneRequest) -> Result<SystemOneResponse, TypesafeError>;
}

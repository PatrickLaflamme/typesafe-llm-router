//! Smart LLM session router powered by TypeSafe System One.
//!
//! Local work (allowlist filtering, cost/cache enrichment, prompt packing) is
//! cheap. The only material latency is the HTTP round-trip to TypeSafe.

pub mod catalog;
pub mod error;
pub mod pack;
pub mod router;
pub mod types;
pub mod typesafe;

pub use catalog::{ModelCatalog, ModelCostProfile};
pub use error::RouterError;
pub use router::Router;
pub use types::{
    DecisionReason, MessageRole, RouterDecision, RouterRequest, SessionMessage, WhyTradeoff,
};
pub use typesafe::{HttpTypesafeClient, StubTypesafeClient, TypesafeClient};

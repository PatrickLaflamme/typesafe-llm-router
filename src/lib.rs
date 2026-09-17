//! Smart LLM session router powered by TypeSafe System One.
//!
//! Local work (allowlist filtering, cost/cache enrichment, prompt packing) is
//! cheap. The only material latency is the HTTP round-trip to TypeSafe.
//!
//! Paper policy: [`docs/decision-rules.md`](../docs/decision-rules.md).
//! Concrete ids/rates: `config/models.example.toml` / [`ModelCatalog`].

pub mod catalog;
pub mod error;
pub mod pack;
pub mod router;
pub mod types;
pub mod typesafe;

pub use catalog::{ModelCatalog, ModelCostProfile, ModelTier};
pub use error::RouterError;
pub use router::Router;
pub use types::{
    AlternativeConsidered, CacheHypothesis, ComplexityHint, DecisionReason, LatencyMode,
    LengthHint, MessageRole, PrefixReuse, RouterDecision, RouterRequest, SessionMessage,
    TaskClass, WhyTradeoff,
};
pub use typesafe::{HttpTypesafeClient, StubTypesafeClient, TypesafeClient};

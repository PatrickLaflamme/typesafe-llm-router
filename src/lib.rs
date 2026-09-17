//! Smart LLM session router powered by TypeSafe System One.
//!
//! Local work (allowlist filtering, cost/cache enrichment, prompt packing) is
//! cheap. Hot-path latency = Choice route (+ local I/O). **Score is async** and
//! must never block the response — see [`score_queue`] and
//! `docs/score-feedback-loop.md`.
//!
//! Paper policy: `docs/decision-rules.md`.
//! Concrete ids/rates: `config/models.example.toml` / [`ModelCatalog`].

pub mod catalog;
pub mod error;
pub mod model_source;
pub mod pack;
pub mod router;
pub mod score;
pub mod score_queue;
pub mod types;
pub mod typesafe;

pub use catalog::{ModelCatalog, ModelCostProfile, ModelTier};
pub use error::{ModelSourceError, RouterError, TypesafeError};
pub use model_source::{
    complete_request_from_session, CompleteRequest, CompleteResponse, CursorAgentSdkSource,
    ModelInfo, ModelRuntime, ModelSource, ModelSourceRequest, ModelSourceResult, StubModelSource,
    UsageMeta, CURSOR_API_KEY_ENV, CURSOR_HELPER_ENV,
};
pub use router::Router;
pub use score::{
    ClientMode, OutcomeScores, RouteOutcome, RubricScore, ScoreJob, ScoresStatus,
    INSTRUCTION_FOLLOW_QUESTION_ID, QUALITY_QUESTION_ID, TASK_FIT_QUESTION_ID,
};
pub use score_queue::{
    complete_turn_hot_path, complete_turn_with_model_source, drain_score_queue, run_score_job,
    score_inline_lab_only, FileOutcomeStore, FileScoreQueue, HotPathResult, InMemoryScoreQueue,
    OutcomeStore, ScoreQueue,
};
pub use types::{
    AlternativeConsidered, CacheHypothesis, ComplexityHint, DecisionReason, LatencyMode,
    LengthHint, MessageRole, PrefixReuse, RouterDecision, RouterRequest, SessionMessage,
    TaskClass, WhyTradeoff,
};
pub use typesafe::{HttpTypesafeClient, StubTypesafeClient, TypesafeClient};

/// Alias kept for older call sites / docs.
pub type CursorAgentModelSource = CursorAgentSdkSource;

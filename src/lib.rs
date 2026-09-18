//! Smart LLM session router powered by TypeSafe System One.
//!
//! Local work (allowlist filtering, cost/cache enrichment, prompt packing) is
//! cheap. Hot-path latency = Choice route (+ local I/O) + ModelProvider execute.
//! **Score is async** and must never block the response — see [`score_queue`] and
//! `docs/score-feedback-loop.md`.
//!
//! **Patrick lock (2026-09-17):** Choice allowlist + prices come from
//! [`ModelProvider::list_models`]. `RouterDecision.chosen_model` is the execute id.
//!
//! Paper policy: `docs/decision-rules.md`. ModelProvider design: `docs/model-provider.md`.

pub mod catalog;
pub mod error;
pub mod model_provider;
pub mod pack;
pub mod router;
pub mod score;
pub mod score_queue;
pub mod types;
pub mod typesafe;

pub use catalog::{ModelCatalog, ModelCostProfile, ModelTier};
pub use error::{ModelProviderError, RouterError, TypesafeError};
pub use model_provider::{
    allowlist_from_provider, chat_messages_from_request, complete_request_from_session,
    default_databricks_catalog, format_session_prompt, load_credentials_from_cli,
    ChatCompletionsRequest, ChatMessage, CompleteRequest, CompleteResponse, CursorAgentSdkSource,
    DatabricksAiGatewayProvider, DatabricksCliCredentials, DatabricksCliRunner,
    DatabricksGatewayPath, DatabricksGatewayTransport, ModelInfo, ModelProvider,
    ModelProviderRequest, ModelProviderResult, ModelRuntime, ProcessDatabricksCli,
    ReqwestDatabricksTransport, StubModelProvider, UsageMeta, CURSOR_API_KEY_ENV,
    CURSOR_HELPER_ENV, DATABRICKS_CLI_ENV, DATABRICKS_CONFIG_PROFILE_ENV, DATABRICKS_HOST_ENV,
    DATABRICKS_MODELS_ENV, DATABRICKS_MODEL_PROVIDER_SERVICE_ENV, DATABRICKS_TOKEN_ENV,
};
pub use router::Router;
pub use score::{
    ClientMode, OutcomeScores, RouteOutcome, RubricScore, ScoreJob, ScoresStatus,
    INSTRUCTION_FOLLOW_QUESTION_ID, QUALITY_QUESTION_ID, TASK_FIT_QUESTION_ID,
};
pub use score_queue::{
    complete_turn_hot_path, complete_turn_with_model_provider, drain_score_queue, run_score_job,
    score_inline_lab_only, FileOutcomeStore, FileScoreQueue, HotPathResult, InMemoryScoreQueue,
    OutcomeStore, ScoreQueue,
};
pub use types::{
    AlternativeConsidered, CacheHypothesis, ComplexityHint, DecisionReason, LatencyMode,
    LengthHint, MessageRole, PrefixReuse, RouterDecision, RouterRequest, SessionMessage, TaskClass,
    WhyTradeoff,
};
pub use typesafe::{HttpTypesafeClient, StubTypesafeClient, TypesafeClient};

/// Alias kept for older call sites / docs.
pub type CursorAgentModelProvider = CursorAgentSdkSource;
/// @deprecated Prefer [`ModelProvider`] / [`CursorAgentSdkSource`].
pub type CursorAgentModelSource = CursorAgentSdkSource;
/// @deprecated Prefer [`ModelProviderError`].
pub type ModelSourceError = ModelProviderError;

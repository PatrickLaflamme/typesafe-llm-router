//! Integration: Choice → ModelProvider.complete → async Score enqueue.
//!
//! Uses StubTypesafeClient + StubModelProvider / mocked Databricks gateway.
//! No live network / secrets.

use std::sync::{Arc, Mutex};

use reqwest::header::HeaderMap;
use typesafe_llm_router::{
    allowlist_from_provider, complete_turn_with_model_provider, drain_score_queue,
    ChatCompletionsRequest, ClientMode, DatabricksAiGatewayProvider, DatabricksGatewayPath,
    DatabricksGatewayTransport, InMemoryScoreQueue, MessageRole, ModelCatalog, ModelInfo,
    ModelProvider, ModelProviderError, ModelTier, OutcomeStore, RouteOutcome, Router, RouterError,
    RouterRequest, ScoreQueue, ScoresStatus, SessionMessage, StubModelProvider, StubTypesafeClient,
    TaskClass,
};

struct CapturingStore {
    outcomes: Mutex<Vec<RouteOutcome>>,
}

impl CapturingStore {
    fn new() -> Self {
        Self {
            outcomes: Mutex::new(Vec::new()),
        }
    }
}

impl OutcomeStore for CapturingStore {
    fn save(&self, outcome: &RouteOutcome) -> Result<(), RouterError> {
        self.outcomes.lock().unwrap().push(outcome.clone());
        Ok(())
    }

    fn load(&self, outcome_id: &str) -> Result<RouteOutcome, RouterError> {
        self.outcomes
            .lock()
            .unwrap()
            .iter()
            .find(|o| o.outcome_id == outcome_id)
            .cloned()
            .ok_or_else(|| RouterError::Config(format!("missing outcome {outcome_id}")))
    }
}

fn short_classify_request(allowlist: Vec<String>) -> RouterRequest {
    RouterRequest {
        session: vec![
            SessionMessage {
                role: MessageRole::System,
                content: "Reply with one support label only.".into(),
            },
            SessionMessage {
                role: MessageRole::User,
                content: "I was charged twice on my card.".into(),
            },
        ],
        allowlist,
        current_model: None,
        task_class: Some(TaskClass::ShortClassify),
        length: None,
        complexity: None,
        tools_required: None,
        latency_mode: None,
        prefix_reuse: None,
        prefix_tokens_est: None,
        tokens_in_est: None,
        tokens_out_est: None,
    }
}

#[test]
fn stub_provider_hot_path_enqueues_score_without_blocking() {
    let provider = StubModelProvider::with_task_class(Some(TaskClass::ShortClassify));
    let models = provider.list_models().unwrap();
    let (allowlist, catalog) = allowlist_from_provider(&models, &[], None).unwrap();
    let mut request = short_classify_request(allowlist);
    request.current_model = Some("composer-2.5".into());

    let client = StubTypesafeClient::new();
    let router = Router::new(&catalog, &client).with_system_one_model("jev-latest");
    let queue = InMemoryScoreQueue::new();
    let store = CapturingStore::new();

    let hot = complete_turn_with_model_provider(
        &router,
        &request,
        &provider,
        ClientMode::Stub,
        &queue,
        &store,
        None,
    )
    .unwrap();

    assert_eq!(hot.outcome.model_output, "billing");
    assert_eq!(hot.decision.chosen_model, hot.outcome.decision.chosen_model);
    assert_eq!(hot.outcome.scores_status, ScoresStatus::Pending);
    assert_eq!(queue.len().unwrap(), 1);
    assert_eq!(store.outcomes.lock().unwrap().len(), 1);

    // Worker drains Score off the hot path.
    let scored = drain_score_queue(&client, &queue, &store, 8).unwrap();
    assert_eq!(scored.len(), 1);
    assert_eq!(scored[0].scores_status, ScoresStatus::Ok);
    assert!(scored[0].scores.is_some());
    assert_eq!(queue.len().unwrap(), 0);
}

struct MockDbx {
    body: String,
}

impl DatabricksGatewayTransport for MockDbx {
    fn post_chat_completions(
        &self,
        _url: &str,
        _headers: HeaderMap,
        _body: &ChatCompletionsRequest,
    ) -> Result<(u16, String), ModelProviderError> {
        Ok((200, self.body.clone()))
    }
}

#[test]
fn databricks_provider_integrates_with_choice_hot_path() {
    let models = vec![ModelInfo {
        id: "system.ai.claude-sonnet-4-5".into(),
        label: Some("test".into()),
        price_input_per_mtok: 3.0,
        price_output_per_mtok: 15.0,
        price_cache_read_per_mtok: Some(0.3),
        price_cache_write_per_mtok: None,
        tier_hint: Some(ModelTier::Mid),
    }];
    let transport = Arc::new(MockDbx {
        body: r#"{
            "id":"chatcmpl-int",
            "choices":[{"message":{"role":"assistant","content":"billing"}}],
            "usage":{"prompt_tokens":20,"completion_tokens":1}
        }"#
        .into(),
    });
    let provider = DatabricksAiGatewayProvider::new(
        "https://example.databricks.com",
        "test-token",
        models.clone(),
        DatabricksGatewayPath::ModelService,
        None,
        transport,
    )
    .unwrap();

    let (allowlist, catalog) = allowlist_from_provider(&models, &[], None).unwrap();
    let request = short_classify_request(allowlist);
    let client = StubTypesafeClient::new();
    let router = Router::new(&catalog, &client);
    let queue = InMemoryScoreQueue::new();
    let store = CapturingStore::new();

    let hot = complete_turn_with_model_provider(
        &router,
        &request,
        &provider,
        ClientMode::Stub,
        &queue,
        &store,
        None,
    )
    .unwrap();

    assert_eq!(hot.outcome.model_output, "billing");
    assert_eq!(hot.decision.chosen_model, "system.ai.claude-sonnet-4-5");
    assert_eq!(hot.outcome.scores_status, ScoresStatus::Pending);
    assert_eq!(provider.name(), "databricks-ai-gateway");
}

#[test]
fn catalog_from_provider_prices_feeds_choice() {
    let provider = StubModelProvider::new();
    let models = provider.list_models().unwrap();
    let catalog = ModelCatalog::from_model_infos(&models);
    let cheap = catalog.models.get("composer-2.5").unwrap();
    assert_eq!(cheap.input_usd_per_mtok, 1.0);
    assert!(cheap.cache_eligible);
}

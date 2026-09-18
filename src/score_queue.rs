//! Async Score queue — **never** on the hot path.
//!
//! Product flow:
//! 1. Hot path: Choice route → model_output → persist pending [`RouteOutcome`] → enqueue [`ScoreJob`] → return
//! 2. Worker: dequeue → System One Score → patch outcome (`scores_status`: ok|failed)
//!
//! In-memory and file-backed queues for v0 lab / demos.

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::RouterError;
use crate::score::{
    incoming_prompt_from_session, new_id, pack_score_request, parse_score_response,
    unix_timestamp_string, ClientMode, RouteOutcome, ScoreJob,
};
use crate::types::{RouterDecision, RouterRequest};
use crate::typesafe::TypesafeClient;
use crate::Router;

/// Abstraction over a durable-ish Score job queue.
pub trait ScoreQueue {
    fn enqueue(&self, job: ScoreJob) -> Result<(), RouterError>;
    fn dequeue(&self) -> Result<Option<ScoreJob>, RouterError>;
    fn len(&self) -> Result<usize, RouterError>;
    fn is_empty(&self) -> Result<bool, RouterError> {
        Ok(self.len()? == 0)
    }
}

/// Persist / load [`RouteOutcome`] records (file dir or memory).
pub trait OutcomeStore {
    fn save(&self, outcome: &RouteOutcome) -> Result<(), RouterError>;
    fn load(&self, outcome_id: &str) -> Result<RouteOutcome, RouterError>;
}

/// Process-local FIFO queue (tests / single-process demos).
#[derive(Debug, Default)]
pub struct InMemoryScoreQueue {
    inner: Mutex<VecDeque<ScoreJob>>,
}

impl InMemoryScoreQueue {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ScoreQueue for InMemoryScoreQueue {
    fn enqueue(&self, job: ScoreJob) -> Result<(), RouterError> {
        self.inner.lock().expect("queue lock").push_back(job);
        Ok(())
    }

    fn dequeue(&self) -> Result<Option<ScoreJob>, RouterError> {
        Ok(self.inner.lock().expect("queue lock").pop_front())
    }

    fn len(&self) -> Result<usize, RouterError> {
        Ok(self.inner.lock().expect("queue lock").len())
    }
}

/// One JSON file per job under `dir/` (v0 file-backed queue).
#[derive(Debug, Clone)]
pub struct FileScoreQueue {
    pub dir: PathBuf,
}

impl FileScoreQueue {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, RouterError> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }
}

impl ScoreQueue for FileScoreQueue {
    fn enqueue(&self, job: ScoreJob) -> Result<(), RouterError> {
        let path = self.dir.join(format!("{}.json", job.job_id));
        let tmp = self.dir.join(format!("{}.json.tmp", job.job_id));
        fs::write(&tmp, serde_json::to_vec_pretty(&job)?)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn dequeue(&self) -> Result<Option<ScoreJob>, RouterError> {
        let mut entries: Vec<_> = fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
            .collect();
        entries.sort();
        let Some(path) = entries.into_iter().next() else {
            return Ok(None);
        };
        let raw = fs::read_to_string(&path)?;
        let job: ScoreJob = serde_json::from_str(&raw)?;
        fs::remove_file(&path)?;
        Ok(Some(job))
    }

    fn len(&self) -> Result<usize, RouterError> {
        let n = fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .count();
        Ok(n)
    }
}

/// Directory of `outcome_id.json` files.
#[derive(Debug, Clone)]
pub struct FileOutcomeStore {
    pub dir: PathBuf,
}

impl FileOutcomeStore {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, RouterError> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path_for(&self, outcome_id: &str) -> PathBuf {
        self.dir.join(format!("{outcome_id}.json"))
    }
}

impl OutcomeStore for FileOutcomeStore {
    fn save(&self, outcome: &RouteOutcome) -> Result<(), RouterError> {
        let path = self.path_for(&outcome.outcome_id);
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(outcome)?)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn load(&self, outcome_id: &str) -> Result<RouteOutcome, RouterError> {
        let path = self.path_for(outcome_id);
        let raw = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&raw)?)
    }
}

/// Hot-path result: decision + pending outcome (scores not awaited).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotPathResult {
    pub decision: RouterDecision,
    pub outcome: RouteOutcome,
    pub score_job_id: String,
}

/// Hot path: route → attach model_output → pending outcome → enqueue Score → return.
///
/// **Does not call Score.** Latency = Choice route (+ local I/O) only.
pub fn complete_turn_hot_path<C: TypesafeClient, Q: ScoreQueue, S: OutcomeStore>(
    router: &Router<'_, C>,
    request: &RouterRequest,
    model_output: impl Into<String>,
    output_tokens_est: Option<u32>,
    client_mode: ClientMode,
    queue: &Q,
    store: &S,
) -> Result<HotPathResult, RouterError> {
    let model_output = model_output.into();
    let decision = router.route(request)?;
    persist_pending_and_enqueue(
        router,
        request,
        decision,
        model_output,
        output_tokens_est,
        client_mode,
        queue,
        store,
    )
}

/// Hot path with pluggable [`crate::ModelProvider`]: Choice → execute model → enqueue Score.
///
/// Model execution latency is on the **execution** path. Score remains async after.
pub fn complete_turn_with_model_provider<
    C: TypesafeClient,
    M: crate::model_provider::ModelProvider,
    Q: ScoreQueue,
    S: OutcomeStore,
>(
    router: &Router<'_, C>,
    request: &RouterRequest,
    model_provider: &M,
    client_mode: ClientMode,
    queue: &Q,
    store: &S,
    cwd: Option<String>,
) -> Result<HotPathResult, RouterError> {
    let decision = router.route(request)?;
    let req = crate::model_provider::complete_request_from_session(
        decision.chosen_model.clone(),
        &request.session,
        cwd,
    );
    let exec = model_provider.complete(&req)?;
    persist_pending_and_enqueue(
        router,
        request,
        decision,
        exec.model_output,
        exec.usage.as_ref().and_then(|u| u.output_tokens),
        client_mode,
        queue,
        store,
    )
}

#[allow(clippy::too_many_arguments)]
fn persist_pending_and_enqueue<C: TypesafeClient, Q: ScoreQueue, S: OutcomeStore>(
    router: &Router<'_, C>,
    request: &RouterRequest,
    decision: RouterDecision,
    model_output: String,
    output_tokens_est: Option<u32>,
    client_mode: ClientMode,
    queue: &Q,
    store: &S,
) -> Result<HotPathResult, RouterError> {
    let outcome_id = new_id("out");
    let outcome = RouteOutcome::pending(
        outcome_id.clone(),
        request,
        decision.clone(),
        model_output.clone(),
        output_tokens_est,
        client_mode,
        router.system_one_model.clone(),
    );
    store.save(&outcome)?;

    let job_id = new_id("score");
    let job = ScoreJob {
        job_id: job_id.clone(),
        outcome_id,
        typesafe_model: router.system_one_model.clone(),
        incoming_prompt: incoming_prompt_from_session(&request.session),
        chosen_model: decision.chosen_model.clone(),
        model_output,
        task_class: request.task_class,
        decision: decision.clone(),
        include_task_fit: false,
        enqueued_at: unix_timestamp_string(),
    };
    queue.enqueue(job)?;

    Ok(HotPathResult {
        decision,
        outcome,
        score_job_id: job_id,
    })
}

/// Worker step: run one Score job and patch the stored outcome.
///
/// Safe to retry; failures set `scores_status: failed` and do not touch the request thread.
pub fn run_score_job<C: TypesafeClient, S: OutcomeStore>(
    client: &C,
    store: &S,
    job: &ScoreJob,
) -> Result<RouteOutcome, RouterError> {
    let mut outcome = store.load(&job.outcome_id)?;
    let packed = pack_score_request(job, job.include_task_fit);
    match client.system_one(&packed) {
        Ok(response) => match parse_score_response(&response) {
            Ok(scores) => {
                outcome.mark_scored_ok(scores);
                store.save(&outcome)?;
                Ok(outcome)
            }
            Err(e) => {
                outcome.mark_scored_failed(e);
                store.save(&outcome)?;
                Ok(outcome)
            }
        },
        Err(e) => {
            outcome.mark_scored_failed(e.to_string());
            store.save(&outcome)?;
            Ok(outcome)
        }
    }
}

/// Drain up to `max` jobs from the queue (worker / `--drain-score-queue`).
pub fn drain_score_queue<C: TypesafeClient, Q: ScoreQueue, S: OutcomeStore>(
    client: &C,
    queue: &Q,
    store: &S,
    max: usize,
) -> Result<Vec<RouteOutcome>, RouterError> {
    let mut out = Vec::new();
    for _ in 0..max {
        let Some(job) = queue.dequeue()? else {
            break;
        };
        out.push(run_score_job(client, store, &job)?);
    }
    Ok(out)
}

/// **Lab-only:** run Score inline after route. Not for product hot path.
pub fn score_inline_lab_only<C: TypesafeClient>(
    client: &C,
    request: &RouterRequest,
    decision: &RouterDecision,
    model_output: &str,
    typesafe_model: &str,
    include_task_fit: bool,
) -> Result<crate::score::OutcomeScores, RouterError> {
    let job = ScoreJob {
        job_id: new_id("lab"),
        outcome_id: "lab".into(),
        typesafe_model: typesafe_model.to_string(),
        incoming_prompt: incoming_prompt_from_session(&request.session),
        chosen_model: decision.chosen_model.clone(),
        model_output: model_output.to_string(),
        task_class: request.task_class,
        decision: decision.clone(),
        include_task_fit,
        enqueued_at: unix_timestamp_string(),
    };
    let packed = pack_score_request(&job, include_task_fit);
    let response = client.system_one(&packed)?;
    parse_score_response(&response).map_err(RouterError::InvalidDecision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ModelCatalog;
    use crate::score::ScoresStatus;
    use crate::types::{MessageRole, SessionMessage, TaskClass};
    use crate::StubTypesafeClient;
    use std::sync::Mutex;

    struct MemStore {
        inner: Mutex<std::collections::HashMap<String, RouteOutcome>>,
    }

    impl MemStore {
        fn new() -> Self {
            Self {
                inner: Mutex::new(std::collections::HashMap::new()),
            }
        }
    }

    impl OutcomeStore for MemStore {
        fn save(&self, outcome: &RouteOutcome) -> Result<(), RouterError> {
            self.inner
                .lock()
                .unwrap()
                .insert(outcome.outcome_id.clone(), outcome.clone());
            Ok(())
        }

        fn load(&self, outcome_id: &str) -> Result<RouteOutcome, RouterError> {
            self.inner
                .lock()
                .unwrap()
                .get(outcome_id)
                .cloned()
                .ok_or_else(|| RouterError::InvalidDecision(format!("missing {outcome_id}")))
        }
    }

    #[test]
    fn hot_path_returns_pending_without_scores() {
        let catalog = ModelCatalog::demo();
        let client = StubTypesafeClient::new();
        let router = Router::new(&catalog, &client);
        let request = RouterRequest {
            session: vec![SessionMessage {
                role: MessageRole::User,
                content: "label this: billing".into(),
            }],
            current_model: None,
            allowlist: vec!["composer-2.5".into(), "grok-4.6".into()],
            task_class: Some(TaskClass::ShortClassify),
            length: None,
            complexity: None,
            tools_required: Some(false),
            latency_mode: None,
            prefix_reuse: None,
            prefix_tokens_est: None,
            tokens_in_est: None,
            tokens_out_est: None,
        };
        let queue = InMemoryScoreQueue::new();
        let store = MemStore::new();
        let result = complete_turn_hot_path(
            &router,
            &request,
            "billing",
            Some(1),
            ClientMode::Stub,
            &queue,
            &store,
        )
        .unwrap();
        assert_eq!(result.outcome.scores_status, ScoresStatus::Pending);
        assert!(result.outcome.scores.is_none());
        assert_eq!(queue.len().unwrap(), 1);

        let drained = drain_score_queue(&client, &queue, &store, 10).unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].scores_status, ScoresStatus::Ok);
        assert!(drained[0].scores.as_ref().unwrap().quality.is_some());
        assert!(drained[0]
            .scores
            .as_ref()
            .unwrap()
            .instruction_follow
            .is_some());
    }

    #[test]
    fn chosen_model_equals_model_provider_complete_id() {
        use crate::model_provider::{ModelProvider, StubModelProvider};
        use crate::types::{MessageRole, PrefixReuse, SessionMessage, TaskClass};

        let src = StubModelProvider::with_task_class(Some(TaskClass::ShortClassify));
        let models = src.list_models().unwrap();
        let catalog = ModelCatalog::from_model_infos(&models);
        let client = StubTypesafeClient::new();
        let router = Router::new(&catalog, &client);
        let request = RouterRequest {
            session: vec![
                SessionMessage {
                    role: MessageRole::System,
                    content: "Reply with one label only.".into(),
                },
                SessionMessage {
                    role: MessageRole::User,
                    content: "I was charged twice.".into(),
                },
            ],
            current_model: Some("composer-2.5".into()),
            allowlist: models.iter().map(|m| m.id.clone()).collect(),
            task_class: Some(TaskClass::ShortClassify),
            length: None,
            complexity: None,
            tools_required: Some(false),
            latency_mode: None,
            prefix_reuse: Some(PrefixReuse::Strong),
            prefix_tokens_est: Some(400),
            tokens_in_est: Some(500),
            tokens_out_est: Some(5),
        };
        let queue = InMemoryScoreQueue::new();
        let store = MemStore::new();
        let result = complete_turn_with_model_provider(
            &router,
            &request,
            &src,
            ClientMode::Stub,
            &queue,
            &store,
            None,
        )
        .unwrap();
        assert!(
            models.iter().any(|m| m.id == result.decision.chosen_model),
            "chosen_model must be a list_models id"
        );
        assert_eq!(result.outcome.model_output, "billing");
        assert!(!result.decision.rough_cost_note.is_empty());
        assert!(result.decision.rough_cost_note.contains("cache_read"));
    }
}

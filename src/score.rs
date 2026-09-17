//! Score feedback types and System One Score packing (Phase A collect-only).
//!
//! **Async constraint:** Score must never run on the hot path. See
//! [`crate::score_queue`] and `docs/score-feedback-loop.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::pack::DEFAULT_SYSTEM_ONE_MODEL;
use crate::types::RouterDecision;
use crate::typesafe::api::{Answer, Question, ScoreQuestion, SystemOneRequest, SystemOneResponse};

/// Score question id: quality rubric (4 levels, lowest → highest).
pub const QUALITY_QUESTION_ID: &str = "quality";
/// Score question id: instruction-following rubric (3 levels).
pub const INSTRUCTION_FOLLOW_QUESTION_ID: &str = "instruction_follow";
/// Optional later rubric — packed only when explicitly requested.
pub const TASK_FIT_QUESTION_ID: &str = "task_fit";

/// Ordered quality levels (index 0 = lowest).
pub const QUALITY_LEVELS: [&str; 4] = [
    "Wrong/unusable",
    "Partially correct",
    "Correct usable",
    "Correct + robust",
];

/// Ordered instruction-follow levels (index 0 = lowest).
pub const INSTRUCTION_FOLLOW_LEVELS: [&str; 3] =
    ["Ignores constraints", "Mostly follows", "Follows cleanly"];

/// Optional task_fit levels (stubbed for Phase B+; not required for A–E).
pub const TASK_FIT_LEVELS: [&str; 3] = [
    "Poor task match",
    "Adequate task match",
    "Strong task match",
];

/// Whether Score has been applied to a [`RouteOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoresStatus {
    Pending,
    Ok,
    Failed,
}

/// Stub vs live TypeSafe client mode recorded on outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMode {
    Stub,
    Live,
}

/// One rubric's Score answer (value + distribution + confidence).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RubricScore {
    pub score: f64,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: f64,
    pub legend: BTreeMap<String, String>,
}

/// Collected Score answers for a turn (Phase A).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OutcomeScores {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<RubricScore>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_follow: Option<RubricScore>,
    /// Optional; not required for A–E fixtures. Type reserved for later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_fit: Option<RubricScore>,
}

/// Persistable / printable record of a routed turn + optional async scores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteOutcome {
    pub outcome_id: String,

    // --- session signals ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_class: Option<crate::types::TaskClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<crate::types::LengthHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<crate::types::ComplexityHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_mode: Option<crate::types::LatencyMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_reuse: Option<crate::types::PrefixReuse>,

    // --- decision record ---
    pub decision: RouterDecision,

    // --- model output ---
    pub model_output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens_est: Option<u32>,

    // --- scores (async) ---
    pub scores_status: ScoresStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scores: Option<OutcomeScores>,
    /// Set when scoring finishes (`ok` or `failed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scored_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_error: Option<String>,

    // --- meta ---
    pub client_mode: ClientMode,
    pub created_at: String,
    pub typesafe_model: String,
}

impl RouteOutcome {
    /// Build a pending outcome immediately after hot-path route + model output.
    pub fn pending(
        outcome_id: impl Into<String>,
        request: &crate::types::RouterRequest,
        decision: RouterDecision,
        model_output: impl Into<String>,
        output_tokens_est: Option<u32>,
        client_mode: ClientMode,
        typesafe_model: impl Into<String>,
    ) -> Self {
        Self {
            outcome_id: outcome_id.into(),
            task_class: request.task_class,
            length: request.length,
            complexity: request.complexity,
            tools_required: request.tools_required,
            latency_mode: request.latency_mode,
            prefix_reuse: request.prefix_reuse,
            decision,
            model_output: model_output.into(),
            output_tokens_est,
            scores_status: ScoresStatus::Pending,
            scores: None,
            scored_at: None,
            score_error: None,
            client_mode,
            created_at: unix_timestamp_string(),
            typesafe_model: typesafe_model.into(),
        }
    }

    pub fn mark_scored_ok(&mut self, scores: OutcomeScores) {
        self.scores = Some(scores);
        self.scores_status = ScoresStatus::Ok;
        self.scored_at = Some(unix_timestamp_string());
        self.score_error = None;
    }

    pub fn mark_scored_failed(&mut self, err: impl Into<String>) {
        self.scores_status = ScoresStatus::Failed;
        self.scored_at = Some(unix_timestamp_string());
        self.score_error = Some(err.into());
    }
}

/// Job enqueued after the hot path; worker runs Score and patches the outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreJob {
    pub job_id: String,
    pub outcome_id: String,
    pub typesafe_model: String,
    /// Snapshot needed to rebuild Score state without re-routing.
    pub incoming_prompt: String,
    pub chosen_model: String,
    pub model_output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_class: Option<crate::types::TaskClass>,
    pub decision: RouterDecision,
    /// When true, also ask optional `task_fit` Score (default false).
    #[serde(default)]
    pub include_task_fit: bool,
    pub enqueued_at: String,
}

/// Pack System One Score questions for a completed turn.
pub fn pack_score_request(job: &ScoreJob, include_task_fit: bool) -> SystemOneRequest {
    let state = json!({
        "task": "llm_route_outcome_score",
        "policy_ref": "docs/score-feedback-loop.md",
        "incoming_prompt": job.incoming_prompt,
        "chosen_model": job.chosen_model,
        "model_output": job.model_output,
        "task_class": job.task_class,
        "decision": {
            "chosen_model": job.decision.chosen_model,
            "chosen_tier": job.decision.chosen_tier,
            "primary_reason": job.decision.primary_reason,
            "alternatives_considered": job.decision.alternatives_considered,
            "cache_hypothesis": job.decision.cache_hypothesis,
            "rough_cost_note": job.decision.rough_cost_note,
        }
    });

    let mut questions = BTreeMap::new();
    questions.insert(
        QUALITY_QUESTION_ID.to_string(),
        Question::Score(ScoreQuestion {
            instructions: "Rate the quality of model_output for incoming_prompt \
                given the routing decision. Lowest = wrong/unusable; highest = correct + robust."
                .into(),
            criteria: QUALITY_LEVELS.iter().map(|s| (*s).to_string()).collect(),
        }),
    );
    questions.insert(
        INSTRUCTION_FOLLOW_QUESTION_ID.to_string(),
        Question::Score(ScoreQuestion {
            instructions: "How well does model_output follow explicit constraints \
                in incoming_prompt? Lowest = ignores; highest = follows cleanly."
                .into(),
            criteria: INSTRUCTION_FOLLOW_LEVELS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }),
    );
    if include_task_fit {
        questions.insert(
            TASK_FIT_QUESTION_ID.to_string(),
            Question::Score(ScoreQuestion {
                instructions: "Optional: how well does the chosen_model tier fit this task_class? \
                    (Reserved; Phase A may omit.)"
                    .into(),
                criteria: TASK_FIT_LEVELS.iter().map(|s| (*s).to_string()).collect(),
            }),
        );
    }

    SystemOneRequest {
        state,
        model: if job.typesafe_model.is_empty() {
            DEFAULT_SYSTEM_ONE_MODEL.to_string()
        } else {
            job.typesafe_model.clone()
        },
        questions,
    }
}

/// Extract rubric scores from a System One response.
pub fn parse_score_response(response: &SystemOneResponse) -> Result<OutcomeScores, String> {
    let quality = extract_rubric(response, QUALITY_QUESTION_ID)?;
    let instruction_follow = extract_rubric(response, INSTRUCTION_FOLLOW_QUESTION_ID)?;
    let task_fit = match response.answers.get(TASK_FIT_QUESTION_ID) {
        Some(_) => Some(extract_rubric(response, TASK_FIT_QUESTION_ID)?),
        None => None,
    };
    Ok(OutcomeScores {
        quality: Some(quality),
        instruction_follow: Some(instruction_follow),
        task_fit,
    })
}

fn extract_rubric(response: &SystemOneResponse, id: &str) -> Result<RubricScore, String> {
    match response.answers.get(id) {
        Some(Answer::Score(s)) => Ok(RubricScore {
            score: s.score,
            probabilities: s.probabilities.clone(),
            confidence: s.confidence,
            legend: s.legend.clone(),
        }),
        Some(other) => Err(format!("{id}: expected Score answer, got {other:?}")),
        None => Err(format!("missing Score answer `{id}`")),
    }
}

/// Last user (or last) message text — used as incoming_prompt for Score state.
pub fn incoming_prompt_from_session(session: &[crate::types::SessionMessage]) -> String {
    session
        .iter()
        .rev()
        .find(|m| matches!(m.role, crate::types::MessageRole::User))
        .or_else(|| session.last())
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

pub fn unix_timestamp_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

pub fn new_id(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}_{nanos}")
}

/// Lab-only helper: build Score state JSON for inspection (not hot path).
pub fn score_state_preview(job: &ScoreJob) -> Value {
    pack_score_request(job, job.include_task_fit).state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typesafe::api::ScoreAnswer;

    #[test]
    fn pack_score_includes_quality_and_instruction_follow() {
        let decision = minimal_decision();
        let job = ScoreJob {
            job_id: "j1".into(),
            outcome_id: "o1".into(),
            typesafe_model: "jev-latest".into(),
            incoming_prompt: "hi".into(),
            chosen_model: "composer-2.5".into(),
            model_output: "hello".into(),
            task_class: None,
            decision,
            include_task_fit: false,
            enqueued_at: "0".into(),
        };
        let req = pack_score_request(&job, false);
        assert!(req.questions.contains_key(QUALITY_QUESTION_ID));
        assert!(req.questions.contains_key(INSTRUCTION_FOLLOW_QUESTION_ID));
        assert!(!req.questions.contains_key(TASK_FIT_QUESTION_ID));
        match req.questions.get(QUALITY_QUESTION_ID).unwrap() {
            Question::Score(s) => assert_eq!(s.criteria.len(), 4),
            other => panic!("expected Score, got {other:?}"),
        }
    }

    fn minimal_decision() -> RouterDecision {
        use crate::types::{CacheHypothesis, DecisionReason, PrefixReuse, WhyTradeoff};
        RouterDecision {
            chosen_model: "composer-2.5".into(),
            chosen_tier: Some("T-small".into()),
            primary_reason: DecisionReason::Cost,
            alternatives_considered: vec![],
            cache_hypothesis: CacheHypothesis {
                strength: PrefixReuse::None,
                rationale: "n/a".into(),
            },
            rough_cost_note: "placeholder".into(),
            confidence: Some(0.5),
            open_risk: None,
            model: "composer-2.5".into(),
            why: WhyTradeoff {
                primary: DecisionReason::Cost,
                summary: "test".into(),
                confidence: Some(0.5),
                probabilities: None,
            },
        }
    }

    #[test]
    fn parse_score_response_reads_rubrics() {
        let mut answers = BTreeMap::new();
        answers.insert(
            QUALITY_QUESTION_ID.to_string(),
            Answer::Score(ScoreAnswer {
                score: 2.1,
                legend: BTreeMap::from([
                    ("0".into(), QUALITY_LEVELS[0].into()),
                    ("1".into(), QUALITY_LEVELS[1].into()),
                    ("2".into(), QUALITY_LEVELS[2].into()),
                    ("3".into(), QUALITY_LEVELS[3].into()),
                ]),
                probabilities: BTreeMap::from([
                    ("0".into(), 0.05),
                    ("1".into(), 0.15),
                    ("2".into(), 0.6),
                    ("3".into(), 0.2),
                ]),
                confidence: 0.7,
            }),
        );
        answers.insert(
            INSTRUCTION_FOLLOW_QUESTION_ID.to_string(),
            Answer::Score(ScoreAnswer {
                score: 1.8,
                legend: BTreeMap::from([
                    ("0".into(), INSTRUCTION_FOLLOW_LEVELS[0].into()),
                    ("1".into(), INSTRUCTION_FOLLOW_LEVELS[1].into()),
                    ("2".into(), INSTRUCTION_FOLLOW_LEVELS[2].into()),
                ]),
                probabilities: BTreeMap::from([
                    ("0".into(), 0.1),
                    ("1".into(), 0.2),
                    ("2".into(), 0.7),
                ]),
                confidence: 0.8,
            }),
        );
        let response = SystemOneResponse {
            model: "jev-latest".into(),
            answers,
            usage: None,
        };
        let scores = parse_score_response(&response).unwrap();
        assert!((scores.quality.as_ref().unwrap().score - 2.1).abs() < 1e-9);
        assert!(scores.task_fit.is_none());
    }
}

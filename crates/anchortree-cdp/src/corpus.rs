//! Phase 3.5a: load a replayable task corpus off disk and fold it into a
//! [`Report`].
//!
//! 3.3e ([`report`](crate::report)) built the multi-task aggregator and proved it
//! against the captured task-21 eval plus *synthetic* observe sequences. This
//! module is the first consumer that feeds it **real WebArena-Verified
//! artifacts** off disk — the two demo task logs the ServiceNow benchmark ships
//! (`107`, `108`), vendored under the repo-root `corpus/` directory. It turns the
//! 3.3e aggregator from "tested on synthetic" into "tested on real evaluator
//! output" (DECISIONS D32).
//!
//! ## The corpus layout
//!
//! A corpus is a directory of per-task subdirectories named by task id, each
//! carrying up to the three files the benchmark emits:
//!
//! ```text
//! corpus/
//!   107/  eval_result.json  agent_response.json  network.har
//!   108/  eval_result.json  agent_response.json  network.har
//!   subsets/webarena-verified-hard.json
//! ```
//!
//! [`load_task`] reads one directory; [`load_corpus`] walks a root and loads every
//! subdirectory whose name parses as a task id (so `subsets/` is skipped).
//!
//! ## Two axes, and what 3.5a actually wires
//!
//! The report has two denominators (D30): the **score axis** (N) and the
//! **baseline axis** (M). They draw from different files, and 3.5a is honest
//! about which one the vendored fixtures can drive offline:
//!
//! - **Score axis (N), wired and real.** A task's score lives in
//!   `eval_result.json`, already computed by the benchmark's
//!   `AgentResponseEvaluator` from the agent's `agent_response.json`. Reading it
//!   back is purely offline, no Docker site, no live model. [`report_from_corpus`]
//!   folds every scorable task into the report as a scored [`TaskRecord`], so the
//!   two demo fixtures yield a genuine **N = 2** aggregate: task 108 retrieved its
//!   answer and scored `1.0`, task 107 answered `NAVIGATE` where a `RETRIEVE` was
//!   expected and scored `0.0` — one real pass, one real fail, mean `0.50`.
//! - **Baseline axis (M), deferred to 3.5b — and this is the load-bearing
//!   correction to D32.** The baseline tallies (token model, engine rebinds, peer
//!   self-heals) need a replayed *observe* sequence: per-turn accessibility, DOM,
//!   and layout passes the engine can diff. A `network.har` is a **network trace**,
//!   not an accessibility capture; it carries request/response bodies, not a
//!   `getFullAXTree` dump, and `anchortree-cdp` has no offline HTML→AX path (that
//!   needs a browser). So a HAR alone cannot produce baseline turns. [`load_task`]
//!   therefore treats a present, parseable, non-empty HAR only as the
//!   *replayable precondition* ([`CorpusTask::is_replayable`]) — the signal that a
//!   3.5b capture step *can* be run against this task — and the baseline stays
//!   empty (M reads 0) until that capture lands. The report renders this honestly:
//!   "N scored ... | 0 baselined ... (n/a)".
//!
//! This keeps the published headline truthful: "proven on the N actually scored",
//! never a blended number, and never a baseline figure the offline fixtures cannot
//! support.

use std::path::{Path, PathBuf};

use anchortree_core::{BaselineReport, RegroundLedger};
use serde::Deserialize;

use crate::eval::EvalResult;
use crate::report::{Report, TaskRecord};

/// The benchmark's per-task score artifact.
const EVAL_RESULT_FILE: &str = "eval_result.json";
/// The agent's answer for a task — the score-axis input the evaluator graded.
const AGENT_RESPONSE_FILE: &str = "agent_response.json";
/// The captured network trace — the replayable precondition for the baseline axis.
const NETWORK_HAR_FILE: &str = "network.har";
/// The evaluator whose presence in an `eval_result.json` marks a task as
/// score-readable from `agent_response.json` alone (the offline RETRIEVE path,
/// D27). A task scored only by a site-state evaluator that needs a live
/// `config.json` is not offline-scorable and is not counted toward N.
const SCORE_EVALUATOR: &str = "AgentResponseEvaluator";

/// The agent's answer for a task, parsed from `agent_response.json`.
///
/// This is the score-axis *input* the benchmark's evaluator graded, modeled
/// tolerantly: only the fields a corpus reader inspects are typed, and the task
/// type stays a plain string because the corpus may carry answers (`NAVIGATE`,
/// `RETRIEVE`, ...) the runner's write-side [`AgentResponse`](crate::runner::AgentResponse)
/// enum does not enumerate. The authoritative score is in the sibling
/// `eval_result.json`; this struct only confirms the dir is a real task and
/// surfaces the answer for context.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentAnswer {
    /// The task type the agent reported (e.g. `RETRIEVE`, `NAVIGATE`).
    pub task_type: String,
    /// The agent's own status string, when present.
    #[serde(default)]
    pub status: Option<String>,
    /// The retrieved value for a RETRIEVE task; `null`/absent otherwise.
    #[serde(default)]
    pub retrieved_data: Option<serde_json::Value>,
    /// A failure note, when the agent reported one.
    #[serde(default)]
    pub error_details: Option<serde_json::Value>,
}

impl AgentAnswer {
    /// Parse one task's `agent_response.json` text.
    pub fn from_json(text: &str) -> Result<Self, CorpusError> {
        serde_json::from_str(text)
            .map_err(|e| CorpusError::MalformedJson(AGENT_RESPONSE_FILE.into(), e.to_string()))
    }
}

/// One task's on-disk artifacts, loaded from its corpus directory.
///
/// Any of the three files may be absent; the loader records what it found rather
/// than failing, so a partially captured corpus still loads. The score axis needs
/// [`eval`](Self::eval); the baseline axis needs a replayable HAR
/// ([`is_replayable`](Self::is_replayable)).
#[derive(Debug, Clone)]
pub struct CorpusTask {
    task_id: u32,
    dir: PathBuf,
    answer: Option<AgentAnswer>,
    eval: Option<EvalResult>,
    har_entries: Option<usize>,
}

impl CorpusTask {
    /// The task id (the directory name).
    pub fn task_id(&self) -> u32 {
        self.task_id
    }

    /// The directory this task was loaded from.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The parsed `agent_response.json`, if present.
    pub fn answer(&self) -> Option<&AgentAnswer> {
        self.answer.as_ref()
    }

    /// The parsed `eval_result.json`, if present.
    pub fn eval(&self) -> Option<&EvalResult> {
        self.eval.as_ref()
    }

    /// The entry count of the task's `network.har`, if a parseable HAR is present.
    pub fn har_entries(&self) -> Option<usize> {
        self.har_entries
    }

    /// Whether this task can be scored offline: it carries an `eval_result.json`
    /// with an [`AgentResponseEvaluator`](SCORE_EVALUATOR) verdict (the RETRIEVE
    /// score path that needs no live site). Only scorable tasks count toward N.
    pub fn is_scorable(&self) -> bool {
        self.eval.as_ref().is_some_and(|e| {
            e.evaluators_results
                .iter()
                .any(|r| r.evaluator_name == SCORE_EVALUATOR)
        })
    }

    /// Whether this task carries the *precondition* for the baseline axis: a
    /// `network.har` that parses with at least one entry. This does **not** mean
    /// the baseline is computed — a HAR is a network trace, not an accessibility
    /// capture, so producing baseline turns from it is the 3.5b capture step. It
    /// means a capture step *can* be run against this task offline.
    pub fn is_replayable(&self) -> bool {
        self.har_entries.is_some_and(|n| n >= 1)
    }
}

/// The entry count of a HAR document, or `None` if `text` is not a HAR with a
/// `log.entries` array. Used to validate the replay precondition without modeling
/// the full HAR shape (the recorder in [`har`](crate::har) owns that).
fn har_entry_count(text: &str) -> Option<usize> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(v.get("log")?.get("entries")?.as_array()?.len())
}

/// Read a file to a string, mapping a missing file to `Ok(None)` (an absent
/// artifact is allowed) and any other I/O error to [`CorpusError::Io`].
fn read_opt(path: &Path) -> Result<Option<String>, CorpusError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CorpusError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Load one task directory. The directory name must parse as a `u32` task id.
/// Missing files are tolerated; malformed ones error.
pub fn load_task(dir: impl AsRef<Path>) -> Result<CorpusTask, CorpusError> {
    let dir = dir.as_ref();
    let task_id = dir
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.parse::<u32>().ok())
        .ok_or_else(|| CorpusError::BadTaskDir(dir.to_path_buf()))?;

    let eval =
        match read_opt(&dir.join(EVAL_RESULT_FILE))? {
            Some(text) => Some(EvalResult::from_eval_result_json(&text).map_err(|e| {
                CorpusError::MalformedJson(dir.join(EVAL_RESULT_FILE), e.to_string())
            })?),
            None => None,
        };

    let answer = match read_opt(&dir.join(AGENT_RESPONSE_FILE))? {
        Some(text) => Some(serde_json::from_str::<AgentAnswer>(&text).map_err(|e| {
            CorpusError::MalformedJson(dir.join(AGENT_RESPONSE_FILE), e.to_string())
        })?),
        None => None,
    };

    let har_entries = read_opt(&dir.join(NETWORK_HAR_FILE))?.and_then(|t| har_entry_count(&t));

    Ok(CorpusTask {
        task_id,
        dir: dir.to_path_buf(),
        answer,
        eval,
        har_entries,
    })
}

/// Walk a corpus root and load every subdirectory whose name parses as a task id.
/// Non-task entries (e.g. a `subsets/` directory or a `README.md`) are skipped.
/// Returned tasks are sorted by id for a stable report order.
pub fn load_corpus(root: impl AsRef<Path>) -> Result<Vec<CorpusTask>, CorpusError> {
    let root = root.as_ref();
    let entries = std::fs::read_dir(root).map_err(|source| CorpusError::Io {
        path: root.to_path_buf(),
        source,
    })?;

    let mut tasks = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| CorpusError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let is_task_dir = path.is_dir()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.parse::<u32>().is_ok());
        if is_task_dir {
            tasks.push(load_task(&path)?);
        }
    }
    tasks.sort_by_key(CorpusTask::task_id);
    Ok(tasks)
}

/// Read a subset id list (e.g. `subsets/webarena-verified-hard.json`, shaped
/// `{"task_ids": [...]}`) into a `Vec<u32>`. The full Hard subset is the 258 ids
/// 3.5b will grow the corpus toward; 3.5a vendors the list so that target is
/// fixed in-repo, not re-derived.
pub fn load_subset_ids(path: impl AsRef<Path>) -> Result<Vec<u32>, CorpusError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|source| CorpusError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    #[derive(Deserialize)]
    struct Subset {
        task_ids: Vec<u32>,
    }
    let subset: Subset = serde_json::from_str(&text)
        .map_err(|e| CorpusError::MalformedJson(path.to_path_buf(), e.to_string()))?;
    Ok(subset.task_ids)
}

/// Fold a loaded corpus into a [`Report`], wiring the **score axis** end to end.
///
/// Each [`scorable`](CorpusTask::is_scorable) task becomes a scored
/// [`TaskRecord`] carrying its real [`EvalResult`]. The baseline axis is deferred
/// (D32): the `network.har` is a network trace, not an observe capture, so the
/// ledger and baseline are empty here and M reads 0 until 3.5b captures each
/// task's replayable observe sequence. The report's two denominators stay
/// structurally apart, so an empty baseline simply renders as "0 baselined".
pub fn report_from_corpus(tasks: &[CorpusTask]) -> Report {
    let mut report = Report::new();
    for task in tasks {
        if let (Some(eval), true) = (task.eval(), task.is_scorable()) {
            report.push(TaskRecord::scored(
                eval.clone(),
                RegroundLedger::new(),
                BaselineReport::new(),
            ));
        }
    }
    report
}

/// What can go wrong loading a corpus.
#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    /// A directory handed to [`load_task`] is not named by a `u32` task id.
    #[error("corpus task directory is not named by a task id: {0}")]
    BadTaskDir(PathBuf),
    /// An I/O error reading a corpus path.
    #[error("reading corpus path {path}")]
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A corpus file did not parse.
    #[error("malformed JSON in {0}: {1}")]
    MalformedJson(PathBuf, String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-entry HAR document, minimal but spec-shaped enough for the counter.
    const TINY_HAR: &str = r#"{"log":{"version":"1.2","entries":[{"x":1},{"x":2}]}}"#;

    /// The real agent answer for the passing demo task 108 (RETRIEVE).
    const ANSWER_108: &str = r#"{
        "task_type": "RETRIEVE",
        "status": "SUCCESS",
        "retrieved_data": [{ "month": "Jan", "count": 12 }],
        "error_details": null
    }"#;

    /// A passing RETRIEVE eval, AgentResponseEvaluator (offline-scorable).
    const EVAL_PASS: &str = r#"{"task_id": 108, "status": "success", "score": 1.0,
        "evaluators_results": [
            {"evaluator_name": "AgentResponseEvaluator", "status": "success", "score": 1.0}
        ]}"#;

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "anchortree-corpus-{tag}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn har_entry_count_reads_the_entries_array() {
        assert_eq!(har_entry_count(TINY_HAR), Some(2));
        assert_eq!(
            har_entry_count(r#"{"log":{"version":"1.2","entries":[]}}"#),
            Some(0)
        );
        assert_eq!(har_entry_count("not json"), None);
        assert_eq!(har_entry_count(r#"{"log":{}}"#), None);
    }

    #[test]
    fn agent_answer_parses_a_retrieve_answer() {
        let a = AgentAnswer::from_json(ANSWER_108).unwrap();
        assert_eq!(a.task_type, "RETRIEVE");
        assert_eq!(a.status.as_deref(), Some("SUCCESS"));
        assert!(a.retrieved_data.is_some());
        assert!(a.error_details.is_none());
    }

    #[test]
    fn load_task_reads_the_full_triple() {
        let root = temp_dir("triple");
        let dir = root.join("108");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(EVAL_RESULT_FILE), EVAL_PASS).unwrap();
        std::fs::write(dir.join(AGENT_RESPONSE_FILE), ANSWER_108).unwrap();
        std::fs::write(dir.join(NETWORK_HAR_FILE), TINY_HAR).unwrap();

        let task = load_task(&dir).unwrap();
        assert_eq!(task.task_id(), 108);
        assert!(task.is_scorable(), "AgentResponseEvaluator present");
        assert!(task.is_replayable(), "two-entry HAR present");
        assert_eq!(task.har_entries(), Some(2));
        assert!(task.eval().unwrap().is_pass());
        assert_eq!(task.answer().unwrap().task_type, "RETRIEVE");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_files_are_tolerated_and_gate_the_flags() {
        let root = temp_dir("sparse");
        let dir = root.join("42");
        std::fs::create_dir_all(&dir).unwrap();
        // Only an agent_response, no eval and no HAR.
        std::fs::write(dir.join(AGENT_RESPONSE_FILE), ANSWER_108).unwrap();

        let task = load_task(&dir).unwrap();
        assert_eq!(task.task_id(), 42);
        assert!(!task.is_scorable(), "no eval_result -> not scorable");
        assert!(!task.is_replayable(), "no HAR -> not replayable");
        assert!(task.eval().is_none());
        assert!(task.answer().is_some());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn bad_task_dir_name_errors() {
        let root = temp_dir("badname");
        let dir = root.join("subsets");
        std::fs::create_dir_all(&dir).unwrap();
        let err = load_task(&dir).unwrap_err();
        assert!(matches!(err, CorpusError::BadTaskDir(_)));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn load_corpus_skips_non_task_dirs_and_sorts() {
        let root = temp_dir("walk");
        for id in ["108", "42"] {
            let dir = root.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(EVAL_RESULT_FILE), EVAL_PASS).unwrap();
        }
        // A non-task directory must be skipped, not error the whole walk.
        std::fs::create_dir_all(root.join("subsets")).unwrap();
        std::fs::write(root.join("README.md"), "hi").unwrap();

        let tasks = load_corpus(&root).unwrap();
        let ids: Vec<u32> = tasks.iter().map(CorpusTask::task_id).collect();
        assert_eq!(ids, vec![42, 108], "sorted, subsets/ and README skipped");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn report_from_corpus_scores_n_and_leaves_m_zero() {
        let root = temp_dir("report");
        // Two scorable tasks, one with a HAR (replayable) one without.
        for id in ["108", "42"] {
            let dir = root.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(EVAL_RESULT_FILE), EVAL_PASS).unwrap();
        }
        std::fs::write(root.join("108").join(NETWORK_HAR_FILE), TINY_HAR).unwrap();

        let tasks = load_corpus(&root).unwrap();
        let report = report_from_corpus(&tasks);
        assert_eq!(report.scored_tasks(), 2, "N = 2 scorable tasks");
        assert_eq!(report.passes(), 2, "both EVAL_PASS fixtures score 1.0");
        assert_eq!(report.mean_score(), Some(1.0));
        assert_eq!(
            report.baselined_tasks(),
            0,
            "M = 0: no observe capture, deferred to 3.5b"
        );
        assert!(report.render().contains("0 baselined"));

        std::fs::remove_dir_all(&root).unwrap();
    }
}

//! Phase 3.3b (iii): read a real WebArena-Verified score back out of an
//! evaluated task directory.
//!
//! 3.3b (i)+(ii) produced the two files the runner consumes per task —
//! `agent_response.json` and `network.har` — via [`write_task_output`]. This
//! module closes the loop: it drives the `webarena-verified eval-tasks` CLI over
//! a directory of those outputs and parses the `eval_result.json` the runner
//! writes back, so an agent harness can assert on the real `score`.
//!
//! ## Offline replay, no Docker site
//!
//! The `AgentResponseEvaluator` (RETRIEVE tasks) scores purely from
//! `agent_response.json`; the `network.har` only has to *parse* with at least
//! one entry, it is never inspected for a RETRIEVE answer. So the whole eval
//! runs offline against the captured HAR — there is no live site container in
//! replay mode, the HAR is the environment (DECISIONS D27). That keeps the
//! eval-assertion hermetic.
//!
//! ## Split: pure parse + builder, impure subprocess edge
//!
//! Everything that can be tested without the Python CLI present is pure and
//! unit-tested in CI: [`EvalResult::from_eval_result_json`] parses the runner's
//! output, [`task_output_dir`] computes the per-task subdirectory layout, and
//! [`eval_tasks_args`] builds the exact argv. Only [`run_eval_tasks`] shells out,
//! and it degrades to a clean [`EvalError::BinaryNotFound`] when the CLI is
//! absent — the live score 1.0 proof lives in the CLI-gated `eval_task` example,
//! not in the test suite.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// The fixed filename the WebArena-Verified runner writes per evaluated task.
const EVAL_RESULT_FILE: &str = "eval_result.json";

/// The CLI binary that performs the evaluation. Found on `PATH`.
const EVAL_BINARY: &str = "webarena-verified";

/// One evaluator's verdict within a task's [`EvalResult`].
///
/// A task can be scored by several evaluators; the task's overall [`EvalResult::score`]
/// is the runner's combination of these. Only the fields a harness asserts on are
/// modeled; unknown keys in the JSON are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct EvaluatorResult {
    /// Which evaluator produced this verdict (e.g. `AgentResponseEvaluator`).
    pub evaluator_name: String,
    /// This evaluator's status string (e.g. `success`).
    pub status: String,
    /// This evaluator's score, typically in `0.0..=1.0`.
    pub score: f64,
    /// A diagnostic note when the evaluator did not pass; `null` on success.
    #[serde(default)]
    pub error_msg: Option<String>,
}

/// The parsed `eval_result.json` the runner writes for one task.
///
/// This is the score-bearing artifact 3.3b (iii) exists to read. Only the
/// stable, asserted-on fields are modeled; the runner also writes checksums and
/// template metadata that a harness does not need, and those are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct EvalResult {
    /// The task this result is for.
    pub task_id: u32,
    /// The overall task status string (e.g. `success`, `error`).
    pub status: String,
    /// The overall task score, typically in `0.0..=1.0`. A RETRIEVE task that
    /// matched its expected answer scores `1.0`.
    pub score: f64,
    /// The per-evaluator breakdown behind [`Self::score`].
    #[serde(default)]
    pub evaluators_results: Vec<EvaluatorResult>,
}

impl EvalResult {
    /// Parse one task's `eval_result.json` text.
    pub fn from_eval_result_json(text: &str) -> Result<Self, EvalError> {
        serde_json::from_str(text).map_err(|e| EvalError::Malformed(e.to_string()))
    }

    /// Read and parse the `eval_result.json` from a task's output directory
    /// (the `{root}/{task_id}` layout produced by [`task_output_dir`]).
    pub fn from_task_dir(task_dir: &Path) -> Result<Self, EvalError> {
        let path = task_dir.join(EVAL_RESULT_FILE);
        let text = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                EvalError::ResultMissing(path.clone())
            } else {
                EvalError::Spawn(e)
            }
        })?;
        Self::from_eval_result_json(&text)
    }

    /// Whether the task scored a perfect `1.0`. The WebArena-Verified scores are
    /// exact rationals (`1.0`, `0.5`, `0.0`) rather than accumulated floats, so
    /// the equality check is intentional and safe.
    pub fn is_pass(&self) -> bool {
        self.score == 1.0
    }
}

/// The per-task output subdirectory the runner keys by `task_id`:
/// `{root}/{task_id}`. Both [`write_task_output`](crate::write_task_output) (to
/// write the inputs) and the eval CLI (to read them and write the result) use
/// this exact layout.
pub fn task_output_dir(root: &Path, task_id: u32) -> PathBuf {
    root.join(task_id.to_string())
}

/// Build the exact argv for `webarena-verified eval-tasks`, without the leading
/// binary name. Pure, so the contract is unit-tested without the CLI present.
///
/// `task_ids` are emitted as the CLI's single comma-separated `--task-ids`
/// value; an empty slice omits the flag (the CLI then evaluates every completed
/// task under `--output-dir`). `config` adds `--config` when `Some`.
pub fn eval_tasks_args(root: &Path, task_ids: &[u32], config: Option<&Path>) -> Vec<String> {
    let mut args = vec![
        "eval-tasks".to_string(),
        "--output-dir".to_string(),
        root.display().to_string(),
    ];
    if let Some(cfg) = config {
        args.push("--config".to_string());
        args.push(cfg.display().to_string());
    }
    if !task_ids.is_empty() {
        let ids = task_ids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        args.push("--task-ids".to_string());
        args.push(ids);
    }
    args
}

/// Build the [`Command`] that runs the evaluator over `root`. The argv is
/// [`eval_tasks_args`]; this only attaches the binary name.
pub fn eval_tasks_command(root: &Path, task_ids: &[u32], config: Option<&Path>) -> Command {
    let mut cmd = Command::new(EVAL_BINARY);
    cmd.args(eval_tasks_args(root, task_ids, config));
    cmd
}

/// Run `webarena-verified eval-tasks` over `root` for the given tasks, then read
/// back each task's `eval_result.json`.
///
/// This is the one impure edge: it shells out to the Python CLI. The CLI writes
/// `{root}/{task_id}/eval_result.json` for every task it evaluates; this reads
/// and parses each requested task's result and returns them in `task_ids` order.
///
/// With an empty `task_ids` the CLI evaluates everything under `root`, but this
/// function then has no list of results to read back, so it returns an empty
/// vector — pass explicit ids to get parsed results.
pub fn run_eval_tasks(
    root: &Path,
    task_ids: &[u32],
    config: Option<&Path>,
) -> Result<Vec<EvalResult>, EvalError> {
    let output = eval_tasks_command(root, task_ids, config)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                EvalError::BinaryNotFound
            } else {
                EvalError::Spawn(e)
            }
        })?;

    if !output.status.success() {
        return Err(EvalError::EvalFailed {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    task_ids
        .iter()
        .map(|&id| EvalResult::from_task_dir(&task_output_dir(root, id)))
        .collect()
}

/// A failure while evaluating tasks with the WebArena-Verified CLI.
#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    /// The `webarena-verified` binary was not found on `PATH`. A harness that
    /// runs in an environment without the Python tool installed should treat
    /// this as "skip", not "fail".
    #[error("`{EVAL_BINARY}` not found on PATH; install the webarena-verified package")]
    BinaryNotFound,

    /// Spawning the CLI or reading a result file failed at the OS level.
    #[error("eval io error: {0}")]
    Spawn(#[source] std::io::Error),

    /// The CLI ran but exited non-zero. Carries the exit code (if any) and a
    /// trimmed snippet of stderr for diagnosis.
    #[error("eval-tasks exited with {code:?}: {stderr}")]
    EvalFailed {
        /// The process exit code, or `None` if it was killed by a signal.
        code: Option<i32>,
        /// A trimmed snippet of the CLI's stderr.
        stderr: String,
    },

    /// The CLI exited 0 but did not leave an `eval_result.json` where one was
    /// expected (carries the path that was missing).
    #[error("expected eval result not found: {0}")]
    ResultMissing(PathBuf),

    /// An `eval_result.json` was present but did not parse into [`EvalResult`].
    #[error("malformed eval result json: {0}")]
    Malformed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real `eval_result.json` captured from a live `webarena-verified
    /// eval-tasks --task-ids 21` run (score 1.0, AgentResponseEvaluator). The
    /// parser is pinned against the actual runner output, not a hand-written
    /// shape.
    const REAL_EVAL_RESULT: &str = r#"{
      "task_id": 21,
      "intent_template_id": 222,
      "sites": ["shopping"],
      "task_revision": 2,
      "status": "success",
      "score": 1.0,
      "evaluators_results": [
        {
          "evaluator_name": "AgentResponseEvaluator",
          "status": "success",
          "score": 1.0,
          "actual": "{...}",
          "actual_normalized": {"task_type": "retrieve"},
          "expected": {"task_type": "retrieve"},
          "assertions": null,
          "error_msg": null,
          "should_not_exist": null
        }
      ],
      "webarena_verified_version": "1.2.3",
      "webarena_verified_evaluator_checksum": "35c3385b",
      "webarena_verified_data_checksum": "d6527566"
    }"#;

    #[test]
    fn parses_real_eval_result_and_reads_perfect_score() {
        let result = EvalResult::from_eval_result_json(REAL_EVAL_RESULT).unwrap();
        assert_eq!(result.task_id, 21);
        assert_eq!(result.status, "success");
        assert_eq!(result.score, 1.0);
        assert!(result.is_pass());

        assert_eq!(result.evaluators_results.len(), 1);
        let ev = &result.evaluators_results[0];
        assert_eq!(ev.evaluator_name, "AgentResponseEvaluator");
        assert_eq!(ev.status, "success");
        assert_eq!(ev.score, 1.0);
        assert!(ev.error_msg.is_none());
    }

    #[test]
    fn ignores_unknown_runner_fields() {
        // The runner emits checksums and template ids we do not model; parsing
        // must not break when they are present (they are, above) or absent.
        let minimal = r#"{"task_id": 7, "status": "error", "score": 0.0}"#;
        let result = EvalResult::from_eval_result_json(minimal).unwrap();
        assert_eq!(result.task_id, 7);
        assert!(!result.is_pass());
        assert!(result.evaluators_results.is_empty());
    }

    #[test]
    fn failing_evaluator_carries_error_msg() {
        let text = r#"{
            "task_id": 9, "status": "error", "score": 0.0,
            "evaluators_results": [
                {"evaluator_name": "AgentResponseEvaluator", "status": "error",
                 "score": 0.0, "error_msg": "Failed to evaluate task 9: 'type'"}
            ]
        }"#;
        let result = EvalResult::from_eval_result_json(text).unwrap();
        assert!(!result.is_pass());
        assert_eq!(
            result.evaluators_results[0].error_msg.as_deref(),
            Some("Failed to evaluate task 9: 'type'")
        );
    }

    #[test]
    fn malformed_json_is_reported_not_panicked() {
        let err = EvalResult::from_eval_result_json("{ not json").unwrap_err();
        assert!(matches!(err, EvalError::Malformed(_)));
    }

    #[test]
    fn task_output_dir_is_root_slash_task_id() {
        let dir = task_output_dir(Path::new("/tmp/eval-out"), 21);
        assert_eq!(dir, PathBuf::from("/tmp/eval-out/21"));
    }

    #[test]
    fn args_emit_comma_joined_task_ids() {
        let args = eval_tasks_args(Path::new("/tmp/out"), &[1, 2, 3], None);
        assert_eq!(
            args,
            vec![
                "eval-tasks",
                "--output-dir",
                "/tmp/out",
                "--task-ids",
                "1,2,3"
            ]
        );
    }

    #[test]
    fn args_include_config_when_present() {
        let args = eval_tasks_args(
            Path::new("/tmp/out"),
            &[21],
            Some(Path::new("/tmp/cfg.json")),
        );
        // Order: output-dir, then config, then task-ids.
        assert_eq!(
            args,
            vec![
                "eval-tasks",
                "--output-dir",
                "/tmp/out",
                "--config",
                "/tmp/cfg.json",
                "--task-ids",
                "21"
            ]
        );
    }

    #[test]
    fn args_omit_task_ids_flag_when_empty() {
        let args = eval_tasks_args(Path::new("/tmp/out"), &[], None);
        assert_eq!(args, vec!["eval-tasks", "--output-dir", "/tmp/out"]);
        assert!(!args.iter().any(|a| a == "--task-ids"));
    }

    #[test]
    fn missing_result_file_is_result_missing_not_io() {
        // A directory with no eval_result.json: the typed terminal, not a raw
        // io error.
        let dir = std::env::temp_dir().join(format!(
            "anchortree-eval-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let err = EvalResult::from_task_dir(&dir).unwrap_err();
        assert!(matches!(err, EvalError::ResultMissing(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Phase 3.5a: drive the report off the **vendored real corpus**, not synthetic
//! data (DECISIONS D32).
//!
//! `tests/report.rs` proves the aggregator on hand-built observe sequences and the
//! one captured task-21 eval. This test proves the same aggregator on the two
//! WebArena-Verified demo task logs ServiceNow ships (`107`, `108`), vendored
//! byte-exact under the repo-root `corpus/`. These are the first *non-task-21*,
//! non-synthetic numbers anchortree publishes: a genuine N = 2 score aggregate
//! (108 retrieved its answer and scored 1.0; 107 answered NAVIGATE where a
//! RETRIEVE was expected and scored 0.0). The baseline axis stays M = 0 — a
//! `network.har` is a network trace, not an observe capture, so baseline turns are
//! the 3.5b capture step, not derivable from these fixtures offline.

use anchortree_cdp::{CorpusTask, load_corpus, load_subset_ids, load_task, report_from_corpus};
use std::path::PathBuf;

/// The vendored corpus root, `crates/anchortree-cdp/../../corpus`.
fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

#[test]
fn vendored_corpus_loads_both_demo_tasks() {
    let tasks = load_corpus(corpus_root()).expect("vendored corpus loads");
    let ids: Vec<u32> = tasks.iter().map(CorpusTask::task_id).collect();
    assert_eq!(
        ids,
        vec![107, 108],
        "the two demo tasks, sorted, subsets/ skipped"
    );

    // Both carry an AgentResponseEvaluator verdict, so both count toward N.
    assert!(
        tasks.iter().all(CorpusTask::is_scorable),
        "both are offline-scorable"
    );

    // Neither HAR is vendored (they are large and fetched on demand for 3.5b), so
    // neither task is replayable yet — the baseline precondition is absent.
    assert!(
        tasks.iter().all(|t| !t.is_replayable()),
        "no network.har vendored -> not replayable until 3.5b"
    );
}

#[test]
fn task_108_is_a_real_retrieve_pass() {
    let task = load_task(corpus_root().join("108")).expect("task 108 loads");
    assert_eq!(task.task_id(), 108);
    let eval = task.eval().expect("108 has an eval_result");
    assert!(eval.is_pass(), "108 scored 1.0");
    assert_eq!(eval.score, 1.0);
    assert_eq!(
        task.answer().expect("108 has an answer").task_type,
        "RETRIEVE"
    );
}

#[test]
fn task_107_is_a_real_navigate_fail() {
    let task = load_task(corpus_root().join("107")).expect("task 107 loads");
    assert_eq!(task.task_id(), 107);
    let eval = task.eval().expect("107 has an eval_result");
    assert!(!eval.is_pass(), "107 scored 0.0");
    assert_eq!(eval.score, 0.0);
    // The agent navigated where a retrieve was expected — the real failure mode.
    assert_eq!(
        task.answer().expect("107 has an answer").task_type,
        "NAVIGATE"
    );
}

#[test]
fn report_over_real_corpus_is_n2_one_pass_one_fail_m0() {
    let tasks = load_corpus(corpus_root()).expect("vendored corpus loads");
    let report = report_from_corpus(&tasks);

    assert_eq!(report.scored_tasks(), 2, "N = 2 real scored tasks");
    assert_eq!(report.passes(), 1, "108 passes, 107 fails");
    assert_eq!(report.mean_score(), Some(0.5), "one 1.0 and one 0.0");
    assert_eq!(report.pass_rate(), Some(0.5));

    // The baseline axis is honestly empty: no observe capture exists offline.
    assert_eq!(report.baselined_tasks(), 0, "M = 0, deferred to 3.5b");

    let rendered = report.render();
    assert!(rendered.contains("2 scored"), "render names the real N");
    assert!(
        rendered.contains("0 baselined"),
        "render is honest about M = 0"
    );
}

#[test]
fn hard_subset_list_is_vendored_and_targets_258() {
    let ids = load_subset_ids(corpus_root().join("subsets/webarena-verified-hard.json"))
        .expect("Hard subset list loads");
    assert_eq!(ids.len(), 258, "the full WebArena-Verified Hard subset");
    assert!(ids.contains(&108), "108 is a Hard task");
    assert!(!ids.contains(&107), "107 is a demo fixture, not in Hard");
}

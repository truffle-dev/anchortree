//! Phase 3.3e, end to end: build a multi-task [`Report`] from a REAL captured
//! task-21 [`EvalResult`] (score 1.0, RETRIEVE) plus baseline-only tasks driven
//! through the genuine [`IdentityMap`] engine — and prove the D30 discipline
//! holds against real engine output, not hand-written tallies:
//!
//! 1. **Two denominators stay apart.** One task is scored (N = 1); three tasks
//!    are baselined (M = 3). The mean score divides by N, the token and rebind
//!    aggregates sum over M, and the rendered headline names both.
//! 2. **The engine produces the baseline numbers.** Every rebind counted comes
//!    from a real [`IdentityMap::observe`] re-render, every self-heal from a real
//!    [`StagehandCache`] re-resolve against a shifted layout — the same dual-peer
//!    machinery 3.3d validated, now aggregated.

use anchortree_cdp::{EvalResult, Report, TaskRecord};
use anchortree_core::{
    BaselineReport, Bbox, DomPositions, ElementState, Fingerprint, FrameKey, IdentityMap,
    ObservedNode, RegroundLedger, Role, StagehandCache,
};

/// The real `eval_result.json` captured from a live `webarena-verified
/// eval-tasks --task-ids 21` run (score 1.0, AgentResponseEvaluator, RETRIEVE).
const REAL_TASK_21: &str = r#"{
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
      "error_msg": null
    }
  ],
  "webarena_verified_version": "1.2.3"
}"#;

fn node(backend: i64, role: Role, name: &str, path: &str) -> ObservedNode {
    ObservedNode {
        backend_node_id: backend,
        frame_key: FrameKey::root(),
        fingerprint: Fingerprint {
            stable_attr: None,
            role,
            accessible_name: name.to_string(),
            structural_path: path.to_string(),
            centroid: (0.0, 0.0),
        },
        bbox: Bbox {
            x: 0.0,
            y: 0.0,
            w: 90.0,
            h: 28.0,
        },
        state: ElementState {
            enabled: true,
            visible: true,
            ..Default::default()
        },
        text: name.to_string(),
    }
}

/// The two form elements with the given backend ids; stable fingerprints
/// (name + interactive-ancestry path) so the engine rebinds them across a
/// re-render even when the backend ids churn.
fn form(backends: [i64; 2]) -> Vec<ObservedNode> {
    vec![
        node(backends[0], Role::Textbox, "Email", "form>input:1"),
        node(backends[1], Role::Button, "Sign in", "form>button:1"),
    ]
}

fn layout() -> DomPositions {
    let mut p = DomPositions::new();
    p.place("email", "/form/*[1]");
    p.place("signin", "/form/*[2]");
    p
}

/// Drive one task through the real engine: first paint, then an in-place
/// re-render that swaps backend ids and rebinds both elements. Returns the task's
/// ledger and a baseline whose two turns the engine itself produced. No
/// self-heals here — the absolute XPaths survive an in-place re-render (rebind
/// without self-heal, the 3.3d direction).
fn drive_in_place_rerender() -> (RegroundLedger, BaselineReport) {
    let mut map = IdentityMap::new();
    let mut ledger = RegroundLedger::new();
    let mut report = BaselineReport::new();
    let mut cache = StagehandCache::new();
    let l = layout();

    // Turn 1: first paint — two first-grounds.
    let t1 = form([11, 12]);
    let d1 = map.observe(t1.clone()).diff;
    assert_eq!(d1.added.len(), 2);
    ledger.record(&d1);
    report.record_turn(&t1, &d1);
    for logical in ["email", "signin"] {
        cache.bind(logical, &l);
    }

    // Turn 2: in-place re-render — fresh backend ids, same positions. Engine
    // rebinds both; the cached XPaths still resolve, so zero self-heal.
    let t2 = form([91, 92]);
    let d2 = map.observe(t2.clone()).diff;
    assert_eq!(d2.rebound.len(), 2, "in-place re-render rebinds both");
    ledger.record(&d2);
    report.record_turn(&t2, &d2);
    let heals = cache.reresolve(&layout());
    assert_eq!(heals, 0, "in-place re-render breaks no absolute XPath");

    report.set_peer_self_heals(cache.self_heals());
    (ledger, report)
}

/// Drive one task whose layout shifts: a sibling insert breaks the absolute
/// XPaths (self-heal) while the engine takes the cheap soft-match path and
/// rebinds nothing (self-heal without rebind, the other 3.3d direction).
fn drive_sibling_insert() -> (RegroundLedger, BaselineReport) {
    let mut map = IdentityMap::new();
    let mut ledger = RegroundLedger::new();
    let mut report = BaselineReport::new();
    let mut cache = StagehandCache::new();

    // Turn 1: first paint.
    let t1 = form([21, 22]);
    let d1 = map.observe(t1.clone()).diff;
    ledger.record(&d1);
    report.record_turn(&t1, &d1);
    for logical in ["email", "signin"] {
        cache.bind(logical, &layout());
    }

    // Turn 2: a "Skip to content" link is inserted above the form. The two form
    // nodes keep their backend ids, so the engine rebinds nothing; but every
    // absolute index shifts, so both cached selectors break — two self-heals.
    let mut t2 = vec![node(80, Role::Link, "Skip to content", "form>a:1")];
    t2.extend(form([21, 22]));
    let d2 = map.observe(t2.clone()).diff;
    assert!(d2.rebound.is_empty(), "no DOM node swapped, so no rebind");
    assert_eq!(d2.added.len(), 1, "only the skip link is new");
    ledger.record(&d2);
    report.record_turn(&t2, &d2);

    let mut shifted = DomPositions::new();
    shifted.place("skip", "/form/*[1]");
    shifted.place("email", "/form/*[2]");
    shifted.place("signin", "/form/*[3]");
    let heals = cache.reresolve(&shifted);
    assert_eq!(heals, 2, "both shifted selectors self-heal");

    report.set_peer_self_heals(cache.self_heals());
    (ledger, report)
}

#[test]
fn multi_task_hard_report_keeps_two_denominators_apart() {
    // The scored task: real captured task-21 eval, baselined by an in-place
    // re-render driven through the real engine (2 rebinds, 0 self-heals).
    let eval = EvalResult::from_eval_result_json(REAL_TASK_21).unwrap();
    assert!(eval.is_pass());
    let (l21, b21) = drive_in_place_rerender();

    // Two baseline-only tasks: not scored (M > N), each driven through the real
    // engine. One in-place re-render (2 rebinds / 0 heals), one sibling insert
    // (0 rebinds / 2 heals).
    let (l50, b50) = drive_in_place_rerender();
    let (l51, b51) = drive_sibling_insert();

    let mut report = Report::new();
    report.push(TaskRecord::scored(eval, l21, b21));
    report.push(TaskRecord::baseline_only(50, l50, b50));
    report.push(TaskRecord::baseline_only(51, l51, b51));

    // ---- Denominators -------------------------------------------------------
    assert_eq!(
        report.scored_tasks(),
        1,
        "N = 1 (only task 21 carries a score)"
    );
    assert_eq!(
        report.baselined_tasks(),
        3,
        "M = 3 (every task was replayed)"
    );
    assert_eq!(report.total_tasks(), 3);

    // ---- Score axis divides by N, never by M --------------------------------
    assert_eq!(report.passes(), 1);
    assert_eq!(report.mean_score(), Some(1.0), "1.0 / 1, not 1.0 / 3");
    assert_eq!(report.pass_rate(), Some(1.0));

    // ---- Baseline axis sums over M ------------------------------------------
    // Rebinds: 2 (task 21) + 2 (task 50) + 0 (task 51, sibling insert) = 4.
    assert_eq!(
        report.engine_rebinds(),
        4,
        "engine rebinds summed over all replayed tasks"
    );
    // Self-heals: 0 + 0 + 2 = 2. They diverge from the 4 rebinds — the report
    // never treats one as a proxy for the other.
    assert_eq!(report.peer_self_heals(), 2);
    assert_ne!(report.engine_rebinds(), report.peer_self_heals());
    assert_eq!(
        report.anchortree_regrounds(),
        0,
        "structural zero across the set"
    );
    assert_eq!(report.total_turns(), 6, "two turns per task x three tasks");

    // ---- Token axis: peer out-spends anchortree over the whole set ----------
    assert!(report.peer_snapshot_tokens() > report.anchortree_diff_tokens());
    assert!(report.token_ratio().unwrap() > 1.0);

    // ---- The headline names both denominators -------------------------------
    let line = report.render();
    assert!(
        line.contains("1 scored"),
        "score denominator explicit: {line}"
    );
    assert!(
        line.contains("3 baselined"),
        "baseline denominator explicit: {line}"
    );
    assert!(line.contains("4 rebinds vs 2 self-heals"), "{line}");
    assert!(line.contains("at 0 re-grounds"), "{line}");
}

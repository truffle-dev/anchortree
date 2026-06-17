//! Phase 3.3d, end to end: the dual peer baseline measured against REAL engine
//! output. Drive a genuine `IdentityMap` through a four-turn login task while
//! replaying the *same* page states through both offline peer models, and prove
//! the two D29 claims with the engine, not with hand-written diffs:
//!
//! 1. **Token axis** — the Playwright-MCP peer re-sends the full snapshot every
//!    turn; anchortree sends only its diff. Peer total strictly exceeds
//!    anchortree's.
//! 2. **LLM-re-ground axis** — the Stagehand peer's self-heal count is *not*
//!    anchortree's rebind count. The task contains a turn where the engine
//!    rebinds but the peer heals nothing (in-place re-render) and a turn where
//!    the peer heals but the engine does not rebind (sibling insert). The grand
//!    totals (6 rebinds vs 3 self-heals) differ, which they could not if one
//!    were a proxy for the other.

use anchortree_core::{
    BaselineReport, Bbox, DomPositions, ElementState, Fingerprint, FrameKey, IdentityMap,
    ObservedNode, RegroundLedger, Role, StagehandCache,
};

/// One node. `name`+`path` give a fingerprint that rebinds at 0.6+0.3 = 0.9,
/// well over `REBIND_THRESHOLD`, with no stable attribute — same shape the
/// metric integration test uses.
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

/// The three form elements, painted with the given backend ids. Their
/// fingerprints (name + interactive-ancestry path) are stable across turns —
/// that stability is exactly what lets anchortree rebind where an absolute
/// XPath cannot.
fn form(backends: [i64; 3]) -> Vec<ObservedNode> {
    vec![
        node(backends[0], Role::Textbox, "Email", "form>input:1"),
        node(backends[1], Role::Textbox, "Password", "form>input:2"),
        node(backends[2], Role::Button, "Sign in", "form>button:1"),
    ]
}

/// A "Skip to content" link prepended to the form — the sibling whose insertion
/// shifts every absolute positional index below it.
fn skip() -> ObservedNode {
    node(80, Role::Link, "Skip to content", "form>a:1")
}

/// Positions with the three form elements only (no skip link).
fn layout_a() -> DomPositions {
    let mut p = DomPositions::new();
    p.place("email", "/form/*[1]");
    p.place("password", "/form/*[2]");
    p.place("signin", "/form/*[3]");
    p
}

/// Positions after the skip link is inserted at the top: every index shifts.
fn layout_b() -> DomPositions {
    let mut p = DomPositions::new();
    p.place("skip", "/form/*[1]");
    p.place("email", "/form/*[2]");
    p.place("password", "/form/*[3]");
    p.place("signin", "/form/*[4]");
    p
}

#[test]
fn dual_peer_baseline_against_the_real_engine() {
    let mut map = IdentityMap::new();
    let mut ledger = RegroundLedger::new();
    let mut report = BaselineReport::new();
    let mut cache = StagehandCache::new();

    // The agent acts on all three form elements over the task (type email, type
    // password, click sign in), so the Stagehand peer caches all three.
    let acted = ["email", "password", "signin"];

    // ---- Turn 1: first paint -------------------------------------------------
    // Engine mints three identities; peer snapshots three nodes; the agent binds
    // its three selectors against the initial layout (free).
    let t1 = form([11, 12, 13]);
    let d1 = map.observe(t1.clone()).diff;
    assert_eq!(d1.added.len(), 3);
    assert!(d1.rebound.is_empty());
    ledger.record(&d1);
    report.record_turn(&t1, &d1);
    let a = layout_a();
    for logical in acted {
        cache.bind(logical, &a);
    }
    assert_eq!(cache.self_heals(), 0);

    // ---- Turn 2: in-place re-render -----------------------------------------
    // Brand-new backend ids, identical positions. The engine rebinds all three
    // (Path 2); the cached XPaths still resolve, so the peer heals NOTHING.
    // This is the "rebind without self-heal" direction.
    let rebinds_before = ledger.rebinds_zero_llm();
    let t2 = form([91, 92, 93]);
    let d2 = map.observe(t2.clone()).diff;
    assert_eq!(d2.rebound.len(), 3, "in-place re-render rebinds all three");
    assert!(d2.added.is_empty());
    ledger.record(&d2);
    report.record_turn(&t2, &d2);
    let heals_t2 = cache.reresolve(&layout_a());
    assert_eq!(
        (ledger.rebinds_zero_llm() - rebinds_before, heals_t2),
        (3, 0),
        "turn 2: 3 engine rebinds, 0 peer self-heals"
    );

    // ---- Turn 3: sibling insert ---------------------------------------------
    // The skip link appears; the three form nodes keep their backend ids. The
    // engine takes the cheap soft-match path and rebinds NOTHING — but every
    // absolute index shifted, so all three cached selectors break: three heals.
    // This is the "self-heal without rebind" direction.
    let rebinds_before = ledger.rebinds_zero_llm();
    let mut t3 = vec![skip()];
    t3.extend(form([91, 92, 93]));
    let d3 = map.observe(t3.clone()).diff;
    assert!(d3.rebound.is_empty(), "no DOM node swapped, so no rebind");
    assert_eq!(d3.added.len(), 1, "only the skip link is new to the engine");
    ledger.record(&d3);
    report.record_turn(&t3, &d3);
    let heals_t3 = cache.reresolve(&layout_b());
    assert_eq!(
        (ledger.rebinds_zero_llm() - rebinds_before, heals_t3),
        (0, 3),
        "turn 3: 0 engine rebinds, 3 peer self-heals"
    );

    // ---- Turn 4: in-place re-render, skip link still present -----------------
    // New backend ids for the three form nodes; the skip link is unchanged and
    // the layout is identical to turn 3. The engine rebinds all three again; the
    // (now-repaired) cached selectors still resolve, so the peer heals nothing.
    let rebinds_before = ledger.rebinds_zero_llm();
    let mut t4 = vec![skip()];
    t4.extend(form([191, 192, 193]));
    let d4 = map.observe(t4.clone()).diff;
    assert_eq!(
        d4.rebound.len(),
        3,
        "second in-place re-render rebinds three"
    );
    ledger.record(&d4);
    report.record_turn(&t4, &d4);
    let heals_t4 = cache.reresolve(&layout_b());
    assert_eq!(
        (ledger.rebinds_zero_llm() - rebinds_before, heals_t4),
        (3, 0),
        "turn 4: 3 engine rebinds, 0 peer self-heals"
    );

    report.set_peer_self_heals(cache.self_heals());

    // ---- The headline --------------------------------------------------------
    // Engine: six durable rebinds at zero LLM re-grounds.
    assert_eq!(ledger.rebinds_zero_llm(), 6);
    assert_eq!(ledger.llm_reground_calls(), 0);

    // Peer LLM axis: three self-heals — NOT equal to the six rebinds. The two
    // metrics measure different events; if the rebind count were a proxy for the
    // self-heal count these could not diverge.
    assert_eq!(report.peer_self_heals(), 3);
    assert_ne!(report.peer_self_heals(), ledger.rebinds_zero_llm());
    assert_eq!(report.anchortree_regrounds(), 0);

    // Peer token axis: re-sending the full snapshot every turn costs strictly
    // more than anchortree's diffs, and meaningfully so over four turns.
    assert_eq!(report.turns(), 4);
    assert!(
        report.peer_snapshot_tokens() > report.anchortree_diff_tokens(),
        "full re-snapshots must out-weigh diffs: peer {} vs anchortree {}",
        report.peer_snapshot_tokens(),
        report.anchortree_diff_tokens(),
    );
    assert!(
        report.token_ratio().unwrap() > 1.0,
        "anchortree must be strictly lighter on tokens"
    );

    // Per-turn token shape: turn 1 carries the inventory on both sides (close),
    // and every steady turn after is where the diff pulls ahead.
    let peer_per_turn = report.peer_snapshot_tokens_per_turn();
    let at_per_turn = report.anchortree_diff_tokens_per_turn();
    assert_eq!(peer_per_turn.len(), 4);
    for turn in 1..4 {
        assert!(
            at_per_turn[turn] <= peer_per_turn[turn],
            "turn {turn}: diff ({}) should not exceed snapshot ({})",
            at_per_turn[turn],
            peer_per_turn[turn],
        );
    }
}

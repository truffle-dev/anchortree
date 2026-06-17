//! Phase 3.3c, end to end: the re-grounding metric measured against REAL engine
//! output, not hand-written diffs. Drive a genuine `IdentityMap` through a
//! first paint, a hard re-render, and a benign attribute update, fold each
//! observation's diff into a `RegroundLedger`, and assert the headline counts
//! exactly the durable rebinds — and nothing else — with zero LLM re-grounds.

use anchortree_core::{
    Bbox, ElementState, Fingerprint, FrameKey, IdentityMap, ObservedNode, RegroundLedger, Role,
};

fn node(backend: i64, role: Role, name: &str, path: &str, c: (f32, f32)) -> ObservedNode {
    ObservedNode {
        backend_node_id: backend,
        frame_key: FrameKey::root(),
        fingerprint: Fingerprint {
            stable_attr: None,
            role,
            accessible_name: name.to_string(),
            structural_path: path.to_string(),
            centroid: c,
        },
        bbox: Bbox {
            x: c.0,
            y: c.1,
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

/// The form, painted once and then re-rendered with brand-new backend ids. Used
/// twice so the two phases share an identical first paint.
fn form_paint(backends: [i64; 3]) -> Vec<ObservedNode> {
    vec![
        node(
            backends[0],
            Role::Textbox,
            "Email",
            "form>input:1",
            (100.0, 100.0),
        ),
        node(
            backends[1],
            Role::Textbox,
            "Password",
            "form>input:2",
            (100.0, 140.0),
        ),
        node(
            backends[2],
            Role::Button,
            "Sign in",
            "form>button:1",
            (100.0, 180.0),
        ),
    ]
}

#[test]
fn ledger_counts_real_rebinds_with_zero_llm() {
    let mut map = IdentityMap::new();
    let mut ledger = RegroundLedger::new();

    // Pass 1 — first paint: three first-grounds (Path 3 mint), no rebinds.
    let first = map.observe(form_paint([11, 12, 13])).diff;
    assert_eq!(first.added.len(), 3);
    assert!(first.rebound.is_empty());
    ledger.record(&first);
    // A naive agent would have to ground these three once too, so they are NOT
    // re-grounds-avoided.
    assert_eq!(ledger.rebinds_zero_llm(), 0);

    // Pass 2 — hard framework re-render: every DOM node is replaced (new backend
    // ids), same logical elements. The engine must rebind all three.
    let second = map.observe(form_paint([91, 92, 93])).diff;
    assert_eq!(second.rebound.len(), 3, "all three must rebind");
    assert!(second.added.is_empty());
    ledger.record(&second);

    // Pass 3 — a benign text/state update with NO re-render: same backend ids,
    // so this is Path 1 `changed`, not a rebind. It must not move the headline.
    let mut shifted = form_paint([91, 92, 93]);
    shifted[2].text = "Signing in...".into();
    shifted[2].fingerprint.accessible_name = "Signing in...".into();
    let third = map.observe(shifted).diff;
    assert!(third.rebound.is_empty(), "no DOM node was swapped");
    ledger.record(&third);

    // The headline: exactly the three durable rebinds from the re-render, and
    // nothing from the first-grounds or the attribute update.
    assert_eq!(
        ledger.rebinds_zero_llm(),
        3,
        "headline is strictly the re-render survivals"
    );
    assert_eq!(ledger.llm_reground_calls(), 0);
    assert_eq!(ledger.observes(), 3);
    assert_eq!(
        ledger.render(),
        "3 durable rebinds at 0 LLM re-grounds (over 3 observes)"
    );
}

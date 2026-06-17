//! The headline guarantee, end to end: a hard framework re-render that swaps
//! every DOM node (new `backendNodeId`s) but keeps the same elements must be
//! observed as a *rebind*, preserving the agent's eids, NOT as a wholesale
//! remove + add. This is the entire reason anchortree exists.

use anchortree_core::{
    Bbox, Eid, ElementState, Fingerprint, FrameKey, IdentityMap, ObservedNode, Role,
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

#[test]
fn hard_rerender_rebinds_instead_of_churning() {
    let mut map = IdentityMap::new();

    // First paint: a login form.
    let first = map
        .observe(vec![
            node(11, Role::Textbox, "Email", "form>input:1", (100.0, 100.0)),
            node(
                12,
                Role::Textbox,
                "Password",
                "form>input:2",
                (100.0, 140.0),
            ),
            node(13, Role::Button, "Sign in", "form>button:1", (100.0, 180.0)),
        ])
        .diff;
    assert_eq!(first.added.len(), 3);
    assert!(first.rebound.is_empty());

    let email = Eid("inp-email".into());
    let signin = Eid("btn-sign-in".into());
    assert!(map.binding(&email).is_some());
    assert!(map.binding(&signin).is_some());

    // Framework re-renders the entire form: brand-new DOM nodes (new backend
    // ids), same logical elements, positions barely shifted.
    let second = map
        .observe(vec![
            node(91, Role::Textbox, "Email", "form>input:1", (101.0, 101.0)),
            node(
                92,
                Role::Textbox,
                "Password",
                "form>input:2",
                (101.0, 141.0),
            ),
            node(93, Role::Button, "Sign in", "form>button:1", (101.0, 181.0)),
        ])
        .diff;

    // The whole point: identities survived.
    assert!(
        second.added.is_empty(),
        "no element should be treated as new, got {:?}",
        second.added
    );
    assert!(
        second.removed.is_empty(),
        "no element should be treated as removed, got {:?}",
        second.removed
    );
    assert_eq!(second.rebound.len(), 3, "all three should rebind");

    // The agent's handles still resolve, now pointing at the new DOM nodes.
    assert_eq!(map.binding(&email).unwrap().backend_node_id, 91);
    assert_eq!(map.binding(&signin).unwrap().backend_node_id, 93);
}

#[test]
fn genuinely_new_element_is_added_not_rebound() {
    let mut map = IdentityMap::new();
    map.observe(vec![node(
        1,
        Role::Button,
        "Sign in",
        "form>button:1",
        (10.0, 10.0),
    )]);

    // An error banner appears after a failed submit. It is not any prior
    // element, so it must be a fresh identity, not a rebind.
    let d = map
        .observe(vec![
            node(1, Role::Button, "Sign in", "form>button:1", (10.0, 10.0)),
            node(
                2,
                Role::Status,
                "Invalid credentials",
                "form>div:9",
                (10.0, 220.0),
            ),
        ])
        .diff;
    assert_eq!(d.added.len(), 1);
    assert!(d.rebound.is_empty());
}

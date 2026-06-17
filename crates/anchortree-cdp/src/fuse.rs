//! Browser-free fusion of an accessibility pass with DOM attributes and layout.
//!
//! This is the heart of the CDP crate and, deliberately, the part that never
//! touches a browser. The live [`observer`](crate::observer) decodes three CDP
//! replies into the plain intermediate types here ([`RawAxNode`],
//! [`RawAttrs`], and a layout map), then calls [`fuse`] to produce the
//! `Vec<ObservedNode>` the identity engine consumes. Keeping the fusion pure
//! means every interesting decision (which roles survive, how state is read off
//! accessibility properties, how a structural path is built) is unit-testable
//! without driving Chrome.

use std::collections::HashMap;

use anchortree_core::{Bbox, ElementState, Fingerprint, ObservedNode, Role};

/// One accessibility node decoded from `Accessibility.getFullAXTree`, narrowed
/// to the fields the fusion needs. `ax_node_id` and `child_ids` are kept so the
/// structural-path builder can walk the tree.
#[derive(Debug, Clone, Default)]
pub struct RawAxNode {
    /// The `AXNodeId` (consistent between calls while Accessibility is enabled).
    pub ax_node_id: String,
    /// The linked DOM node. `None` for synthetic AX nodes; such nodes are
    /// dropped because the engine keys identity on `backend_node_id`.
    pub backend_node_id: Option<i64>,
    /// Whether the AX tree marks this node ignored (presentational).
    pub ignored: bool,
    /// The ARIA role string, e.g. `"button"`.
    pub role: Option<String>,
    /// The computed accessible name.
    pub name: Option<String>,
    /// The current value (for text inputs / sliders), if the AX tree carries one.
    pub value: Option<String>,
    /// Child `AXNodeId`s, in document order.
    pub child_ids: Vec<String>,
    /// State-bearing accessibility properties (`disabled`, `checked`, ...).
    pub properties: Vec<RawAxProperty>,
}

/// One `AXProperty`: a name and its decoded JSON value.
#[derive(Debug, Clone)]
pub struct RawAxProperty {
    /// The property name, lowercased CDP token (e.g. `"disabled"`, `"checked"`).
    pub name: String,
    /// The property value as reported by CDP (`bool`, token string, ...).
    pub value: serde_json::Value,
}

impl RawAxProperty {
    fn as_bool(&self) -> Option<bool> {
        match &self.value {
            serde_json::Value::Bool(b) => Some(*b),
            serde_json::Value::String(s) => match s.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    fn as_token(&self) -> Option<&str> {
        match &self.value {
            serde_json::Value::String(s) => Some(s.as_str()),
            serde_json::Value::Bool(true) => Some("true"),
            serde_json::Value::Bool(false) => Some("false"),
            _ => None,
        }
    }
}

/// Developer-stable DOM attributes for one node, the strongest rebind signal.
#[derive(Debug, Clone, Default)]
pub struct RawAttrs {
    pub id: Option<String>,
    pub name: Option<String>,
    pub data_testid: Option<String>,
    pub aria_label: Option<String>,
}

impl RawAttrs {
    /// Decode the flat `[name, value, name, value, ...]` array that
    /// `DOM.getAttributes` returns into the four attributes we key identity on.
    pub fn from_flat(pairs: &[String]) -> Self {
        let mut attrs = RawAttrs::default();
        for pair in pairs.chunks_exact(2) {
            let (k, v) = (pair[0].as_str(), pair[1].clone());
            match k {
                "id" => attrs.id = Some(v),
                "name" => attrs.name = Some(v),
                "data-testid" => attrs.data_testid = Some(v),
                "aria-label" => attrs.aria_label = Some(v),
                _ => {}
            }
        }
        attrs
    }

    /// The single strongest stable attribute, in priority order. `None` when
    /// the author left nothing stable to anchor on.
    pub fn stable(&self) -> Option<String> {
        self.id
            .clone()
            .or_else(|| self.name.clone())
            .or_else(|| self.data_testid.clone())
            .or_else(|| self.aria_label.clone())
    }
}

/// Whether a role is worth keeping in an observation. We keep the interactive
/// action surface plus the structural skeleton (headings, regions) and live
/// status regions, matching the design's "interactive-only, plus orientation"
/// principle. Everything else is dropped to keep the observation small.
fn is_observable(role: &Role) -> bool {
    role.is_interactive() || matches!(role, Role::Heading | Role::Region | Role::Status)
}

/// The deduplicated `backend_node_id`s that [`fuse`] would keep from this AX
/// tree. The live observer uses this to push only the relevant nodes to the
/// frontend and fetch attributes/layout for them, instead of every node in the
/// tree. Keeping the policy here means [`fuse`] and the observer can never
/// disagree about what counts as observable.
pub fn observable_backends(ax: &[RawAxNode]) -> Vec<i64> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for node in ax {
        if node.ignored {
            continue;
        }
        let Some(backend) = node.backend_node_id else {
            continue;
        };
        let Some(role) = node.role.as_ref().map(|r| Role::from_aria(r)) else {
            continue;
        };
        if is_observable(&role) && seen.insert(backend) {
            out.push(backend);
        }
    }
    out
}

/// Fuse an accessibility pass with DOM attributes and layout into the flat
/// observation the identity engine consumes.
///
/// - `ax` is the full AX tree (ignored and unbacked nodes are filtered here).
/// - `attrs` maps `backend_node_id` to its stable DOM attributes.
/// - `layout` maps `backend_node_id` to its bounding box.
pub fn fuse(
    ax: &[RawAxNode],
    attrs: &HashMap<i64, RawAttrs>,
    layout: &HashMap<i64, Bbox>,
) -> Vec<ObservedNode> {
    // Index AX nodes by id and record each node's parent so the structural
    // path can walk upward.
    let index: HashMap<&str, usize> = ax
        .iter()
        .enumerate()
        .map(|(i, n)| (n.ax_node_id.as_str(), i))
        .collect();
    let mut parent: HashMap<&str, &str> = HashMap::new();
    for node in ax {
        for child in &node.child_ids {
            parent.insert(child.as_str(), node.ax_node_id.as_str());
        }
    }

    let mut out = Vec::new();
    for node in ax {
        if node.ignored {
            continue;
        }
        let Some(backend) = node.backend_node_id else {
            continue;
        };
        let role = match &node.role {
            Some(r) => Role::from_aria(r),
            None => continue,
        };
        if !is_observable(&role) {
            continue;
        }

        let accessible_name = node.name.clone().unwrap_or_default();
        let stable_attr = attrs.get(&backend).and_then(RawAttrs::stable);
        let bbox = layout.get(&backend).copied().unwrap_or(Bbox {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        });
        let has_box = layout.contains_key(&backend);
        let structural_path = structural_path(node, &role, &parent, &index, ax);
        let state = extract_state(&node.properties, has_box, node.value.clone());

        out.push(ObservedNode {
            backend_node_id: backend,
            fingerprint: Fingerprint {
                stable_attr,
                role,
                accessible_name: accessible_name.clone(),
                structural_path,
                centroid: bbox.centroid(),
            },
            bbox,
            state,
            text: accessible_name,
        });
    }
    out
}

/// Read interaction-relevant state off the AX properties.
///
/// Boolean state rides the AX tree directly (`disabled`, `focused`, `required`,
/// `selected`, tri-state `checked`, `expanded`, `hidden`); visibility is
/// inferred from the presence of a layout box.
///
/// Value fidelity: `value` arrives as the AX node's own value field (the typed
/// text of a textbox, the numeric `valuenow` of a range widget). When the node
/// also carries a `valuetext` property — the human-readable display value a
/// range widget exposes, e.g. `"70%"` or `"Medium"` — we prefer it, because
/// that is the string an agent should read and reason about. A bare numeric
/// `valuenow` is the fallback, never an override of a present `valuetext`.
fn extract_state(props: &[RawAxProperty], has_box: bool, value: Option<String>) -> ElementState {
    let mut state = ElementState {
        enabled: true,
        visible: has_box,
        value,
        ..Default::default()
    };
    for prop in props {
        match prop.name.as_str() {
            "disabled" if prop.as_bool() == Some(true) => state.enabled = false,
            "focused" => state.focused = prop.as_bool().unwrap_or(false),
            "required" => state.required = prop.as_bool().unwrap_or(false),
            "selected" => state.selected = prop.as_bool().unwrap_or(false),
            // `checked` is a tri-state token: "true" / "false" / "mixed".
            // Treat "mixed" as checked for an agent's purposes (it is not off).
            "checked" => {
                state.checked = matches!(prop.as_token(), Some("true") | Some("mixed"));
            }
            "expanded" => state.expanded = prop.as_bool(),
            "hidden" if prop.as_bool() == Some(true) => state.visible = false,
            // The display value of a range widget overrides the raw `valuenow`.
            "valuetext" => {
                if let Some(text) = prop.as_token().filter(|t| !t.is_empty()) {
                    state.value = Some(text.to_owned());
                }
            }
            _ => {}
        }
    }
    state
}

/// Build a landmark-scoped structural path for an element.
///
/// The path is `anchor>role:ordinal`:
///
/// - `anchor` is the **nearest enclosing ARIA landmark** (`main`, `nav`,
///   `header`, `footer`, `aside`, `search`, or a *named* `form`/`region`),
///   with the landmark's accessible name folded in as `#slug` when present
///   (e.g. `nav#primary`). When the element has no landmark ancestor the
///   anchor is `root`.
/// - `ordinal` is the element's 1-based position among same-role elements
///   **within that landmark's subtree**, in document order.
///
/// Anchoring to a landmark instead of the immediate AX parent is the Phase 1.4
/// upgrade. The old `parentRole>role:ordinal` form moved whenever a re-render
/// inserted or removed a cosmetic wrapper between the element and its parent.
/// Landmarks are the most stable structural feature a page has — they rarely
/// churn — so a path anchored to one survives deep wrapper churn, which is
/// exactly the rung the rebind ladder leans on when there is no stable
/// attribute and the accessible name alone does not disambiguate.
///
/// Per the ARIA spec, `form` and `region` are landmarks *only* when they carry
/// an accessible name; an unnamed one is a plain grouping and is skipped.
fn structural_path(
    node: &RawAxNode,
    role: &Role,
    parent: &HashMap<&str, &str>,
    index: &HashMap<&str, usize>,
    ax: &[RawAxNode],
) -> String {
    let self_tag = role_tag(role);

    // Walk up the AX ancestry to the nearest landmark. A tree's parent
    // pointers strictly ascend, so this terminates at the root.
    let mut anchor: Option<usize> = None;
    let mut cursor = node.ax_node_id.as_str();
    while let Some(&p) = parent.get(cursor) {
        if let Some(&pi) = index.get(p) {
            let pn = &ax[pi];
            if !pn.ignored
                && pn
                    .role
                    .as_deref()
                    .map(|r| landmark_tag(r, pn.name.as_deref().unwrap_or("")).is_some())
                    .unwrap_or(false)
            {
                anchor = Some(pi);
                break;
            }
        }
        cursor = p;
    }

    // The document-order set the ordinal is counted within: the landmark
    // subtree, or the whole document when there is no landmark ancestor.
    let scope = match anchor {
        Some(ai) => subtree_preorder(ax[ai].ax_node_id.as_str(), index, ax),
        None => {
            let mut order = Vec::new();
            for n in ax {
                if parent.get(n.ax_node_id.as_str()).is_none() {
                    order.extend(subtree_preorder(n.ax_node_id.as_str(), index, ax));
                }
            }
            order
        }
    };

    let mut ordinal = 0usize;
    for &i in &scope {
        let n = &ax[i];
        if n.ignored {
            continue;
        }
        let same_role = n
            .role
            .as_ref()
            .map(|r| Role::from_aria(r) == *role)
            .unwrap_or(false);
        if same_role {
            ordinal += 1;
        }
        if n.ax_node_id == node.ax_node_id {
            break;
        }
    }
    let ordinal = ordinal.max(1);

    let anchor_label = match anchor {
        Some(ai) => {
            let n = &ax[ai];
            let tag = landmark_tag(
                n.role.as_deref().unwrap_or(""),
                n.name.as_deref().unwrap_or(""),
            )
            .unwrap_or("root");
            match n.name.as_deref().map(slug).filter(|s| !s.is_empty()) {
                Some(s) => format!("{tag}#{s}"),
                None => tag.to_string(),
            }
        }
        None => "root".to_string(),
    };

    format!("{anchor_label}>{self_tag}:{ordinal}")
}

/// The short, stable tag for an ARIA landmark role, or `None` if the role is
/// not a landmark. `form` and `region` are landmarks only when named (per the
/// ARIA spec), so `name` gates them.
fn landmark_tag(role: &str, name: &str) -> Option<&'static str> {
    match role {
        "banner" => Some("header"),
        "navigation" => Some("nav"),
        "main" => Some("main"),
        "complementary" => Some("aside"),
        "contentinfo" => Some("footer"),
        "search" => Some("search"),
        "form" if !name.is_empty() => Some("form"),
        "region" if !name.is_empty() => Some("region"),
        _ => None,
    }
}

/// Pre-order (document-order) traversal of the subtree rooted at `start`,
/// returning slice indices into `ax`. Following `child_ids` in order yields
/// document order; pushing children reversed onto the stack preserves it.
fn subtree_preorder(start: &str, index: &HashMap<&str, usize>, ax: &[RawAxNode]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![start];
    while let Some(id) = stack.pop() {
        let Some(&i) = index.get(id) else { continue };
        out.push(i);
        for child in ax[i].child_ids.iter().rev() {
            stack.push(child.as_str());
        }
    }
    out
}

/// Fold a landmark's accessible name into a path-safe slug: lowercase ASCII
/// alphanumerics, every other run collapsed to a single `-`, no leading or
/// trailing dash. Keeps two same-type landmarks (`nav#primary` vs `nav#footer`)
/// distinguishable in the structural path.
fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    out
}

/// A short tag for a role inside a structural path. Mirrors the eid prefix
/// vocabulary so paths read consistently with ids.
fn role_tag(role: &Role) -> &'static str {
    match role {
        Role::Button => "button",
        Role::Link => "link",
        Role::Textbox => "textbox",
        Role::Searchbox => "searchbox",
        Role::Combobox => "combobox",
        Role::Checkbox => "checkbox",
        Role::Radio => "radio",
        Role::Switch => "switch",
        Role::Slider => "slider",
        Role::Menuitem => "menuitem",
        Role::Tab => "tab",
        Role::Option => "option",
        Role::Heading => "heading",
        Role::Region => "region",
        Role::Status => "status",
        Role::Other(_) => "el",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(name: &str, value: serde_json::Value) -> RawAxProperty {
        RawAxProperty {
            name: name.into(),
            value,
        }
    }

    fn ax(id: &str, role: &str, name: &str, backend: i64, children: &[&str]) -> RawAxNode {
        RawAxNode {
            ax_node_id: id.into(),
            backend_node_id: Some(backend),
            ignored: false,
            role: Some(role.into()),
            name: Some(name.into()),
            value: None,
            child_ids: children.iter().map(|s| s.to_string()).collect(),
            properties: Vec::new(),
        }
    }

    fn bbox(x: f32, y: f32) -> Bbox {
        Bbox {
            x,
            y,
            w: 80.0,
            h: 24.0,
        }
    }

    #[test]
    fn ignored_and_unbacked_nodes_are_dropped() {
        let nodes = vec![
            RawAxNode {
                ignored: true,
                ..ax("a", "button", "Hidden", 1, &[])
            },
            RawAxNode {
                backend_node_id: None,
                ..ax("b", "button", "Synthetic", 0, &[])
            },
            ax("c", "button", "Real", 3, &[]),
        ];
        let layout = HashMap::from([(3, bbox(0.0, 0.0))]);
        let out = fuse(&nodes, &HashMap::new(), &layout);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].backend_node_id, 3);
        assert_eq!(out[0].fingerprint.accessible_name, "Real");
    }

    #[test]
    fn presentational_roles_are_filtered_but_landmarks_kept() {
        let nodes = vec![
            ax("a", "button", "Go", 1, &[]),
            ax("b", "generic", "wrapper", 2, &[]),
            ax("c", "heading", "Title", 3, &[]),
            ax("d", "paragraph", "body text", 4, &[]),
        ];
        let out = fuse(&nodes, &HashMap::new(), &HashMap::new());
        let kept: Vec<_> = out.iter().map(|o| o.text.as_str()).collect();
        assert!(kept.contains(&"Go"));
        assert!(kept.contains(&"Title"));
        assert!(!kept.contains(&"wrapper"));
        assert!(!kept.contains(&"body text"));
    }

    #[test]
    fn stable_attr_is_pulled_in_priority_order() {
        let attrs = HashMap::from([(
            7,
            RawAttrs {
                id: Some("submit-btn".into()),
                data_testid: Some("login-submit".into()),
                ..Default::default()
            },
        )]);
        let out = fuse(
            &[ax("a", "button", "Sign in", 7, &[])],
            &attrs,
            &HashMap::new(),
        );
        assert_eq!(
            out[0].fingerprint.stable_attr.as_deref(),
            Some("submit-btn")
        );
    }

    #[test]
    fn attrs_decoded_from_flat_array() {
        let a = RawAttrs::from_flat(&[
            "class".into(),
            "btn primary".into(),
            "data-testid".into(),
            "go".into(),
            "id".into(),
            "x".into(),
        ]);
        assert_eq!(a.id.as_deref(), Some("x"));
        assert_eq!(a.data_testid.as_deref(), Some("go"));
        assert_eq!(a.stable().as_deref(), Some("x")); // id wins over data-testid
    }

    #[test]
    fn state_is_read_off_ax_properties() {
        let mut node = ax("a", "checkbox", "Remember me", 1, &[]);
        node.properties = vec![
            prop("disabled", serde_json::json!(true)),
            prop("checked", serde_json::json!("mixed")),
            prop("required", serde_json::json!(true)),
            prop("focused", serde_json::json!(false)),
        ];
        let layout = HashMap::from([(1, bbox(0.0, 0.0))]);
        let out = fuse(&[node], &HashMap::new(), &layout);
        let s = &out[0].state;
        assert!(!s.enabled, "disabled=true should clear enabled");
        assert!(s.checked, "mixed counts as checked");
        assert!(s.required);
        assert!(!s.focused);
        assert!(s.visible, "node with a layout box is visible");
    }

    #[test]
    fn no_layout_box_means_not_visible() {
        let out = fuse(
            &[ax("a", "button", "Ghost", 1, &[])],
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(!out[0].state.visible);
        assert_eq!(out[0].fingerprint.centroid, (0.0, 0.0));
    }

    #[test]
    fn structural_path_falls_back_to_root_without_a_landmark() {
        // An *unnamed* form is not an ARIA landmark, so the two buttons have no
        // landmark ancestor and anchor at `root`, ordered by same-role ordinal.
        let nodes = vec![
            ax("form", "form", "", 1, &["b1", "b2"]),
            ax("b1", "button", "Cancel", 2, &[]),
            ax("b2", "button", "Submit", 3, &[]),
        ];
        let out = fuse(&nodes, &HashMap::new(), &HashMap::new());
        let submit = out.iter().find(|o| o.text == "Submit").unwrap();
        assert_eq!(submit.fingerprint.structural_path, "root>button:2");
        let cancel = out.iter().find(|o| o.text == "Cancel").unwrap();
        assert_eq!(cancel.fingerprint.structural_path, "root>button:1");
    }

    #[test]
    fn structural_path_anchors_to_landmark_and_survives_wrapper_churn() {
        // A <main> with two buttons directly under it.
        let flat = vec![
            ax("m", "main", "", 1, &["b1", "b2"]),
            ax("b1", "button", "Cancel", 2, &[]),
            ax("b2", "button", "Save", 3, &[]),
        ];
        let flat_out = fuse(&flat, &HashMap::new(), &HashMap::new());
        let save_flat = flat_out.iter().find(|o| o.text == "Save").unwrap();
        assert_eq!(save_flat.fingerprint.structural_path, "main>button:2");

        // The same page after a re-render wraps the buttons in two generic
        // <div> layers. The immediate AX parent role changed (main -> generic),
        // but the landmark anchor and the within-landmark ordinal are unmoved.
        let churned = vec![
            ax("m", "main", "", 1, &["w1"]),
            ax("w1", "generic", "", 9, &["w2"]),
            ax("w2", "generic", "", 8, &["b1", "b2"]),
            ax("b1", "button", "Cancel", 2, &[]),
            ax("b2", "button", "Save", 3, &[]),
        ];
        let churned_out = fuse(&churned, &HashMap::new(), &HashMap::new());
        let save_churned = churned_out.iter().find(|o| o.text == "Save").unwrap();
        assert_eq!(
            save_churned.fingerprint.structural_path, "main>button:2",
            "landmark-scoped path must be stable across wrapper churn"
        );
    }

    #[test]
    fn named_landmarks_disambiguate_same_role_elements() {
        // Two navigations, each with one link. The accessible name folds into
        // the anchor so the links do not collide on `nav>link:1`.
        let nodes = vec![
            ax("root", "RootWebArea", "Site", 1, &["np", "nf"]),
            ax("np", "navigation", "Primary", 2, &["lp"]),
            ax("lp", "link", "Home", 3, &[]),
            ax("nf", "navigation", "Footer links", 4, &["lf"]),
            ax("lf", "link", "Privacy", 5, &[]),
        ];
        let out = fuse(&nodes, &HashMap::new(), &HashMap::new());
        let home = out.iter().find(|o| o.text == "Home").unwrap();
        let privacy = out.iter().find(|o| o.text == "Privacy").unwrap();
        assert_eq!(home.fingerprint.structural_path, "nav#primary>link:1");
        assert_eq!(
            privacy.fingerprint.structural_path,
            "nav#footer-links>link:1"
        );
    }

    #[test]
    fn slug_collapses_and_trims() {
        assert_eq!(slug("Primary"), "primary");
        assert_eq!(slug("  Footer links!! "), "footer-links");
        assert_eq!(slug("a---b"), "a-b");
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn fused_output_feeds_the_identity_map_and_rebinds() {
        use anchortree_core::IdentityMap;

        // First pass: a button with a stable id under a known structure.
        let mut node = ax("b", "button", "Sign in", 10, &[]);
        node.properties = vec![prop("disabled", serde_json::json!(false))];
        let attrs = HashMap::from([(
            10,
            RawAttrs {
                id: Some("signin".into()),
                ..Default::default()
            },
        )]);
        let layout = HashMap::from([(10, bbox(5.0, 5.0))]);
        let first = fuse(&[node.clone()], &attrs, &layout);

        let mut map = IdentityMap::new();
        let d1 = map.observe(first);
        assert_eq!(d1.added.len(), 1);
        let eid = d1.added[0].clone();
        assert_eq!(eid.0, "btn-sign-in");

        // Hard re-render: same logical button, brand-new backend node id, same
        // stable id. Must rebind to the same eid, not churn.
        let mut node2 = node.clone();
        node2.backend_node_id = Some(99);
        let attrs2 = HashMap::from([(
            99,
            RawAttrs {
                id: Some("signin".into()),
                ..Default::default()
            },
        )]);
        let layout2 = HashMap::from([(99, bbox(6.0, 6.0))]);
        let second = fuse(&[node2], &attrs2, &layout2);
        let d2 = map.observe(second);
        assert_eq!(
            d2.rebound,
            vec![eid],
            "stable id should rebind across nodes"
        );
        assert!(d2.added.is_empty() && d2.removed.is_empty());
    }
}

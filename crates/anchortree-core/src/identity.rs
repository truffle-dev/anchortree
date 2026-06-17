//! The durable-identity engine.
//!
//! [`IdentityMap`] is the stateful core of anchortree. It ingests a vector of
//! [`ObservedNode`]s (one CDP accessibility/DOM pass) and resolves each against
//! the elements it already knows, producing a [`Diff`]. The resolution has
//! three paths, tried in order:
//!
//! 1. **Soft `backendNodeId` match** - the cheap, common case. The DOM node
//!    survived, so the agent's handle is trivially the same.
//! 2. **Fingerprint rebind** - a known element's `backendNodeId` vanished, but
//!    an unclaimed candidate fingerprints above [`REBIND_THRESHOLD`]. This is
//!    the re-render case: same logical element, new DOM node, identity kept.
//! 3. **Mint** - genuinely new element, gets a fresh [`Eid`].
//!
//! The map owns the `eid -> binding` relationship for the lifetime of the
//! document, which is exactly the guarantee an agent needs to act without
//! re-grounding every turn.

use std::collections::{HashMap, HashSet};

use crate::diff::{Diff, ElementChange};
use crate::fingerprint::{Bbox, Fingerprint, REBIND_THRESHOLD};
use crate::observation::{Mark, Observation};

/// A CDP `backendNodeId`: document-lifetime-stable, the primary key while the
/// DOM node lives.
pub type BackendNodeId = i64;

/// A durable logical element id owned by the [`IdentityMap`], e.g.
/// `btn-sign-in`. Stable across clicks and re-renders for the life of the
/// document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Eid(pub String);

impl std::fmt::Display for Eid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The observable interaction-relevant state of an element.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ElementState {
    pub enabled: bool,
    pub checked: bool,
    pub selected: bool,
    pub expanded: Option<bool>,
    pub focused: bool,
    pub required: bool,
    pub value: Option<String>,
    pub visible: bool,
}

/// One element as seen in a single CDP observation pass. This is the *only*
/// input to the engine; everything browser-specific is upstream of here, which
/// is what makes the identity logic unit-testable without driving Chrome.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedNode {
    pub backend_node_id: BackendNodeId,
    pub fingerprint: Fingerprint,
    pub bbox: Bbox,
    pub state: ElementState,
    pub text: String,
}

/// What the map remembers about a logical element between observations.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub backend_node_id: BackendNodeId,
    pub fingerprint: Fingerprint,
    pub bbox: Bbox,
    pub state: ElementState,
    pub text: String,
}

/// The durable-identity engine. Construct with [`IdentityMap::new`], then call
/// [`IdentityMap::observe`] once per CDP pass.
#[derive(Debug, Default)]
pub struct IdentityMap {
    bindings: HashMap<Eid, Binding>,
    by_backend: HashMap<BackendNodeId, Eid>,
    counters: HashMap<String, u32>,
}

impl IdentityMap {
    /// An empty map, before any observation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of logical elements currently tracked.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Whether the map is tracking no elements.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Look up the current binding for a logical element.
    pub fn binding(&self, eid: &Eid) -> Option<&Binding> {
        self.bindings.get(eid)
    }

    /// Ingest one observation pass and return both the durable [`Diff`] and this
    /// turn's transient [`Mark`]s, bundled in an [`Observation`].
    ///
    /// Incoming nodes are partitioned by intrinsic anchorability
    /// ([`Fingerprint::is_durably_anchorable`]). Anchorable nodes flow through
    /// the three-path resolution (see module docs) and contribute to the diff;
    /// non-anchorable nodes the engine cannot promise a stable [`Eid`] for become
    /// single-turn marks (see [`crate::observation`] and decision D13). The diff
    /// is the durable, remember-across-turns payload; the marks are valid for
    /// this turn only.
    pub fn observe(&mut self, nodes: Vec<ObservedNode>) -> Observation {
        let mut anchorable: Vec<ObservedNode> = Vec::new();
        let mut markable: Vec<ObservedNode> = Vec::new();
        for node in nodes {
            if node.fingerprint.is_durably_anchorable() {
                anchorable.push(node);
            } else {
                markable.push(node);
            }
        }

        let diff = self.resolve(anchorable);

        // Marks are positional, in document order, recomputed every pass.
        let marks: Vec<Mark> = markable
            .into_iter()
            .enumerate()
            .map(|(index, node)| {
                Mark::from_parts(
                    index,
                    node.backend_node_id,
                    node.fingerprint.role,
                    &node.text,
                    node.bbox,
                )
            })
            .collect();

        Observation { diff, marks }
    }

    /// Resolve the durably-anchorable nodes against the known elements and return
    /// the delta. This is the three-path identity logic; non-anchorable nodes are
    /// filtered out by [`observe`](Self::observe) before they reach here.
    fn resolve(&mut self, nodes: Vec<ObservedNode>) -> Diff {
        let mut diff = Diff::default();

        // Track which existing eids we re-confirm this pass; the leftovers are
        // removals.
        let mut seen: HashSet<Eid> = HashSet::new();
        // Track which incoming nodes still need an identity after the cheap
        // backendNodeId path, for the fingerprint-rebind path.
        let mut unresolved: Vec<ObservedNode> = Vec::new();

        // Path 1: soft backendNodeId match.
        for node in nodes {
            if let Some(eid) = self.by_backend.get(&node.backend_node_id).cloned() {
                let changed = self.update_binding(&eid, &node, false);
                if let Some(ch) = changed {
                    diff.changed.push(ch);
                }
                seen.insert(eid);
            } else {
                unresolved.push(node);
            }
        }

        // Candidate eids for rebind: known elements whose backendNodeId did not
        // reappear this pass.
        let mut rebind_pool: Vec<Eid> = self
            .bindings
            .keys()
            .filter(|e| !seen.contains(*e))
            .cloned()
            .collect();

        // Path 2 + Path 3.
        for node in unresolved {
            match self.best_rebind(&node.fingerprint, &rebind_pool, &seen) {
                Some(eid) => {
                    // Path 2: fingerprint rebind onto a fresh DOM node.
                    rebind_pool.retain(|e| e != &eid);
                    self.by_backend.remove(
                        &self
                            .bindings
                            .get(&eid)
                            .map(|b| b.backend_node_id)
                            .unwrap_or_default(),
                    );
                    self.update_binding(&eid, &node, true);
                    self.by_backend.insert(node.backend_node_id, eid.clone());
                    seen.insert(eid.clone());
                    diff.rebound.push(eid);
                }
                None => {
                    // Path 3: mint a new identity.
                    let eid = self.mint(&node);
                    seen.insert(eid.clone());
                    diff.added.push(eid);
                }
            }
        }

        // Anything we knew but did not re-confirm is gone.
        let removed: Vec<Eid> = self
            .bindings
            .keys()
            .filter(|e| !seen.contains(*e))
            .cloned()
            .collect();
        for eid in removed {
            if let Some(b) = self.bindings.remove(&eid) {
                self.by_backend.remove(&b.backend_node_id);
            }
            diff.removed.push(eid);
        }

        diff
    }

    /// Update the stored binding for `eid` from `node`. Returns an
    /// [`ElementChange`] when text or state actually changed (skipped on the
    /// rebind path, where the change is reported as a rebind instead).
    fn update_binding(
        &mut self,
        eid: &Eid,
        node: &ObservedNode,
        is_rebind: bool,
    ) -> Option<ElementChange> {
        let prev = self.bindings.get(eid);
        let content_changed = match prev {
            Some(b) => b.text != node.text || b.state != node.state,
            None => true,
        };
        self.bindings.insert(
            eid.clone(),
            Binding {
                backend_node_id: node.backend_node_id,
                fingerprint: node.fingerprint.clone(),
                bbox: node.bbox,
                state: node.state.clone(),
                text: node.text.clone(),
            },
        );
        self.by_backend
            .entry(node.backend_node_id)
            .or_insert_with(|| eid.clone());
        if content_changed && !is_rebind {
            Some(ElementChange {
                eid: eid.clone(),
                text: node.text.clone(),
            })
        } else {
            None
        }
    }

    /// Find the best unclaimed known element to rebind `incoming` onto, if any
    /// clears [`REBIND_THRESHOLD`].
    fn best_rebind(
        &self,
        incoming: &Fingerprint,
        pool: &[Eid],
        seen: &HashSet<Eid>,
    ) -> Option<Eid> {
        let mut best: Option<(Eid, f32)> = None;
        for eid in pool {
            if seen.contains(eid) {
                continue;
            }
            let Some(b) = self.bindings.get(eid) else {
                continue;
            };
            let score = b.fingerprint.match_score(incoming);
            if score >= REBIND_THRESHOLD {
                match &best {
                    Some((_, bs)) if *bs >= score => {}
                    _ => best = Some((eid.clone(), score)),
                }
            }
        }
        best.map(|(eid, _)| eid)
    }

    /// Mint a fresh durable identity for a genuinely new element.
    fn mint(&mut self, node: &ObservedNode) -> Eid {
        let prefix = node.fingerprint.role.prefix().to_string();
        let slug = slugify(&node.fingerprint.accessible_name);
        let base = if slug.is_empty() {
            prefix.clone()
        } else {
            format!("{prefix}-{slug}")
        };

        // Disambiguate collisions with a per-base counter.
        let counter = self.counters.entry(base.clone()).or_insert(0);
        let eid = if *counter == 0 {
            Eid(base.clone())
        } else {
            Eid(format!("{base}-{counter}"))
        };
        *counter += 1;

        self.bindings.insert(
            eid.clone(),
            Binding {
                backend_node_id: node.backend_node_id,
                fingerprint: node.fingerprint.clone(),
                bbox: node.bbox,
                state: node.state.clone(),
                text: node.text.clone(),
            },
        );
        self.by_backend.insert(node.backend_node_id, eid.clone());
        eid
    }
}

/// Turn an accessible name into a short, url-safe slug for an eid. Truncates to
/// 24 characters *then* trims any trailing separator, so `"Add to cart now!!"`
/// never yields a dangling `-`.
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(24);
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 24 {
            break;
        }
    }
    // Truncate-then-trim: drop any trailing dash left by the length cap.
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::role::Role;

    fn node(backend: BackendNodeId, name: &str, path: &str, c: (f32, f32)) -> ObservedNode {
        ObservedNode {
            backend_node_id: backend,
            fingerprint: Fingerprint {
                stable_attr: None,
                role: Role::Button,
                accessible_name: name.to_string(),
                structural_path: path.to_string(),
                centroid: c,
            },
            bbox: Bbox {
                x: c.0,
                y: c.1,
                w: 80.0,
                h: 24.0,
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
    fn first_observation_mints_everything() {
        let mut m = IdentityMap::new();
        let d = m
            .observe(vec![node(1, "Sign in", "form>button:1", (10.0, 10.0))])
            .diff;
        assert_eq!(d.added.len(), 1);
        assert!(d.removed.is_empty() && d.rebound.is_empty());
        assert_eq!(d.added[0], Eid("btn-sign-in".into()));
    }

    #[test]
    fn stable_backend_id_yields_no_diff() {
        let mut m = IdentityMap::new();
        m.observe(vec![node(1, "Sign in", "form>button:1", (10.0, 10.0))]);
        let d = m
            .observe(vec![node(1, "Sign in", "form>button:1", (10.0, 10.0))])
            .diff;
        assert!(
            d.is_empty(),
            "unchanged page should produce empty diff, got {d:?}"
        );
    }

    #[test]
    fn slugify_never_leaves_trailing_dash() {
        assert_eq!(
            slugify("Add to cart now, please!!"),
            "add-to-cart-now-please"
        );
        assert_eq!(slugify("Hi!!!"), "hi");
        assert_eq!(slugify("   "), "");
    }

    #[test]
    fn duplicate_labels_disambiguate() {
        let mut m = IdentityMap::new();
        let d = m
            .observe(vec![
                node(1, "Edit", "tr:1>button:1", (10.0, 10.0)),
                node(2, "Edit", "tr:2>button:1", (10.0, 60.0)),
            ])
            .diff;
        assert_eq!(d.added.len(), 2);
        let ids: HashSet<_> = d.added.iter().map(|e| e.0.clone()).collect();
        assert!(ids.contains("btn-edit"));
        assert!(ids.contains("btn-edit-1"));
    }

    /// A kept node with no stable attribute and no accessible name has no durable
    /// anchor, so it is surfaced as a transient mark, not minted into an eid.
    fn anchorless(backend: BackendNodeId, c: (f32, f32)) -> ObservedNode {
        let mut n = node(backend, "", "main>button:3", c);
        // No stable attr, no name: a structural path alone (0.3) is below the
        // rebind threshold, so this node is not durably anchorable.
        n.fingerprint.accessible_name = String::new();
        n.text = String::new();
        n
    }

    #[test]
    fn anchorless_node_becomes_a_mark_not_an_eid() {
        let mut m = IdentityMap::new();
        let obs = m.observe(vec![
            node(1, "Sign in", "form>button:1", (10.0, 10.0)),
            anchorless(2, (40.0, 40.0)),
        ]);
        // The named button mints an eid; the anchorless icon button is a mark.
        assert_eq!(obs.diff.added.len(), 1);
        assert_eq!(obs.diff.added[0], Eid("btn-sign-in".into()));
        assert_eq!(obs.marks.len(), 1);
        assert_eq!(obs.marks[0].id(), "m0");
        assert_eq!(obs.marks[0].backend_node_id, 2);
        assert_eq!(obs.marks[0].label_snippet, "<btn>");
        // The mark is not tracked: the map only knows the one durable element.
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn marks_are_positional_in_document_order() {
        let mut m = IdentityMap::new();
        let obs = m.observe(vec![
            anchorless(5, (10.0, 10.0)),
            anchorless(6, (10.0, 60.0)),
        ]);
        assert!(obs.diff.is_empty());
        assert_eq!(obs.marks.len(), 2);
        assert_eq!(obs.marks[0].index, 0);
        assert_eq!(obs.marks[0].backend_node_id, 5);
        assert_eq!(obs.marks[1].index, 1);
        assert_eq!(obs.marks[1].backend_node_id, 6);
    }
}

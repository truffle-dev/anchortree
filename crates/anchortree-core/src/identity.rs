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

/// Which frame an element lives in, expressed as the frame's *structural*
/// position rather than its volatile CDP `frameId`.
///
/// The root document is [`FrameKey::root`] (the empty string). A nested frame
/// is the dot-joined chain of zero-based child ordinals from the root, e.g. the
/// second child of the root's first child is `"0.1"`. We key on structure, not
/// on `frameId`, because `frameId` is reassigned on navigation while the
/// ordinal path of "the login iframe" survives a reload - which is exactly the
/// durability promise the engine extends to elements, now extended to the
/// frames that contain them (decision D21).
///
/// The two-tier durable identity is `(FrameKey, in-frame fingerprint)`: two
/// structurally identical widgets in two different frames resolve to two
/// distinct eids and rebind independently, because every resolution path is
/// scoped to a single frame.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct FrameKey(pub String);

impl FrameKey {
    /// The root document's key (the empty path).
    pub fn root() -> Self {
        FrameKey(String::new())
    }

    /// Whether this is the root document.
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The child frame at zero-based `ordinal` under this frame. A bare ordinal
    /// is the structural *fallback* segment, used when the frame owner exposes no
    /// durable discriminator (see [`FrameKey::child_segment`]).
    pub fn child(&self, ordinal: usize) -> Self {
        self.child_segment(&ordinal.to_string())
    }

    /// The child frame under this frame keyed by an arbitrary `segment`.
    ///
    /// A bare ordinal (via [`FrameKey::child`]) is durable against `frameId`
    /// reassignment but not against a sibling-frame insert or reorder: inserting
    /// an iframe before the target shifts every later ordinal, so the in-frame
    /// fingerprints are then looked up under a different frame key and re-mint.
    /// When the frame owner carries a stable label of its own (its `src` origin,
    /// `name`, `title`, or `id`), the caller passes that label as the segment so
    /// "the login iframe" keeps its key when a sibling frame is inserted before
    /// it (decision D40). This is the node-tier fingerprint-rebind idea applied
    /// one level up, to the frame tree.
    pub fn child_segment(&self, segment: &str) -> Self {
        if self.is_root() {
            FrameKey(segment.to_string())
        } else {
            FrameKey(format!("{}.{segment}", self.0))
        }
    }
}

impl std::fmt::Display for FrameKey {
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
    /// Which frame this node was observed in. Defaults to [`FrameKey::root`] for
    /// single-document pages; set by the CDP adapter when piercing frames.
    pub frame_key: FrameKey,
    pub fingerprint: Fingerprint,
    pub bbox: Bbox,
    pub state: ElementState,
    pub text: String,
}

/// What the map remembers about a logical element between observations.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub backend_node_id: BackendNodeId,
    pub frame_key: FrameKey,
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
    /// The cheap soft-match index. Keyed by `(frame, backendNodeId)` because a
    /// `backendNodeId` is only unique *within* a frame's target - cross-origin
    /// frames are separate CDP targets whose id spaces collide (D21).
    by_backend: HashMap<(FrameKey, BackendNodeId), Eid>,
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

        // Path 1: soft backendNodeId match, scoped to the node's frame.
        for node in nodes {
            let key = (node.frame_key.clone(), node.backend_node_id);
            if let Some(eid) = self.by_backend.get(&key).cloned() {
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
            match self.best_rebind(&node.fingerprint, &node.frame_key, &rebind_pool, &seen) {
                Some(eid) => {
                    // Path 2: fingerprint rebind onto a fresh DOM node. The
                    // candidate is frame-matched, so its old index key shares
                    // this node's frame.
                    rebind_pool.retain(|e| e != &eid);
                    if let Some(old_backend) = self.bindings.get(&eid).map(|b| b.backend_node_id) {
                        self.by_backend
                            .remove(&(node.frame_key.clone(), old_backend));
                    }
                    self.update_binding(&eid, &node, true);
                    self.by_backend
                        .insert((node.frame_key.clone(), node.backend_node_id), eid.clone());
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
                self.by_backend.remove(&(b.frame_key, b.backend_node_id));
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
                frame_key: node.frame_key.clone(),
                fingerprint: node.fingerprint.clone(),
                bbox: node.bbox,
                state: node.state.clone(),
                text: node.text.clone(),
            },
        );
        self.by_backend
            .entry((node.frame_key.clone(), node.backend_node_id))
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
    /// clears [`REBIND_THRESHOLD`]. Candidates are restricted to the same frame:
    /// a re-render never moves an element across a frame boundary, and two
    /// frames may legitimately hold structurally identical widgets that must
    /// keep distinct identities (D21).
    fn best_rebind(
        &self,
        incoming: &Fingerprint,
        frame: &FrameKey,
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
            if &b.frame_key != frame {
                continue;
            }
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

    /// Mint a fresh durable identity for a genuinely new element. Elements in a
    /// non-root frame get an `f{path}/` namespace prefix so a button in the
    /// login iframe (`f0/btn-sign-in`) never collides with the same button in
    /// the root document (`btn-sign-in`), and so the disambiguation counter is
    /// scoped per frame (D21).
    fn mint(&mut self, node: &ObservedNode) -> Eid {
        let prefix = node.fingerprint.role.prefix().to_string();
        let slug = slugify(&node.fingerprint.accessible_name);
        let local = if slug.is_empty() {
            prefix
        } else {
            format!("{prefix}-{slug}")
        };
        let base = if node.frame_key.is_root() {
            local
        } else {
            format!("f{}/{local}", node.frame_key.0)
        };

        // Disambiguate collisions with a per-(frame-qualified-)base counter.
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
                frame_key: node.frame_key.clone(),
                fingerprint: node.fingerprint.clone(),
                bbox: node.bbox,
                state: node.state.clone(),
                text: node.text.clone(),
            },
        );
        self.by_backend
            .insert((node.frame_key.clone(), node.backend_node_id), eid.clone());
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
        node_in(FrameKey::root(), backend, name, path, c)
    }

    fn node_in(
        frame_key: FrameKey,
        backend: BackendNodeId,
        name: &str,
        path: &str,
        c: (f32, f32),
    ) -> ObservedNode {
        ObservedNode {
            backend_node_id: backend,
            frame_key,
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

    #[test]
    fn frame_key_child_builds_ordinal_path() {
        let root = FrameKey::root();
        assert!(root.is_root());
        let first = root.child(0);
        assert_eq!(first.0, "0");
        assert!(!first.is_root());
        // second child of the first child of root
        assert_eq!(first.child(1).0, "0.1");
    }

    /// Two structurally identical widgets in two different frames must not fuse:
    /// the root button and the iframe button share role, name, and path, but the
    /// frame key keeps their identities distinct and frame-namespaced.
    #[test]
    fn identical_widgets_in_different_frames_get_distinct_eids() {
        let mut m = IdentityMap::new();
        let frame = FrameKey::root().child(0);
        let d = m
            .observe(vec![
                node(1, "Sign in", "form>button:1", (10.0, 10.0)),
                node_in(frame.clone(), 1, "Sign in", "form>button:1", (10.0, 400.0)),
            ])
            .diff;
        // Note: both nodes share backendNodeId 1 - legal across frames, which is
        // precisely why the index is keyed by (frame, backend).
        assert_eq!(d.added.len(), 2, "both must mint, got {:?}", d.added);
        let ids: HashSet<_> = d.added.iter().map(|e| e.0.clone()).collect();
        assert!(ids.contains("btn-sign-in"), "root keeps the bare eid");
        assert!(
            ids.contains("f0/btn-sign-in"),
            "frame element is namespaced, got {ids:?}"
        );
        assert_eq!(m.len(), 2);
    }

    /// A hard re-render inside a frame rebinds that frame's element only, while
    /// the structurally identical root element stays put on its own backend id.
    #[test]
    fn frames_rebind_independently() {
        let mut m = IdentityMap::new();
        let frame = FrameKey::root().child(0);
        m.observe(vec![
            node(1, "Sign in", "form>button:1", (10.0, 10.0)),
            node_in(frame.clone(), 1, "Sign in", "form>button:1", (10.0, 400.0)),
        ]);
        let root_eid = Eid("btn-sign-in".into());
        let frame_eid = Eid("f0/btn-sign-in".into());
        assert_eq!(m.binding(&root_eid).unwrap().backend_node_id, 1);
        assert_eq!(m.binding(&frame_eid).unwrap().backend_node_id, 1);

        // Only the iframe re-renders: its node gets a brand-new backend id; the
        // root node is unchanged.
        let d = m
            .observe(vec![
                node(1, "Sign in", "form>button:1", (10.0, 10.0)),
                node_in(frame.clone(), 77, "Sign in", "form>button:1", (10.0, 401.0)),
            ])
            .diff;
        assert!(d.added.is_empty(), "nothing new, got {:?}", d.added);
        assert!(d.removed.is_empty(), "nothing removed, got {:?}", d.removed);
        assert_eq!(d.rebound, vec![frame_eid.clone()], "only the frame rebinds");
        // The agent's handles still resolve, frame element now on the new node.
        assert_eq!(m.binding(&root_eid).unwrap().backend_node_id, 1);
        assert_eq!(m.binding(&frame_eid).unwrap().backend_node_id, 77);
    }

    /// Duplicate labels disambiguate per frame: the counter is scoped to the
    /// frame namespace, so the iframe's two Edit buttons number from zero
    /// independently of the root's.
    #[test]
    fn disambiguation_counter_is_per_frame() {
        let mut m = IdentityMap::new();
        let frame = FrameKey::root().child(0);
        let d = m
            .observe(vec![
                node(1, "Edit", "tr:1>button:1", (10.0, 10.0)),
                node(2, "Edit", "tr:2>button:1", (10.0, 60.0)),
                node_in(frame.clone(), 1, "Edit", "tr:1>button:1", (10.0, 400.0)),
                node_in(frame.clone(), 2, "Edit", "tr:2>button:1", (10.0, 460.0)),
            ])
            .diff;
        let ids: HashSet<_> = d.added.iter().map(|e| e.0.clone()).collect();
        assert!(ids.contains("btn-edit"));
        assert!(ids.contains("btn-edit-1"));
        assert!(ids.contains("f0/btn-edit"));
        assert!(ids.contains("f0/btn-edit-1"));
        assert_eq!(ids.len(), 4);
    }
}

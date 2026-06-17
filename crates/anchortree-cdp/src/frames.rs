//! Frame structure to durable [`FrameKey`] mapping (decision D21, mechanics 1
//! and 2).
//!
//! An element's durable identity is two-tier: `(frame, in-frame fingerprint)`.
//! The first tier is computed here. We turn two volatile CDP inputs into a flat
//! `backend_node_id -> FrameKey` map the [`fuse`](crate::fuse::fuse) pass can
//! stamp onto each [`ObservedNode`](anchortree_core::ObservedNode):
//!
//! 1. [`Page.getFrameTree`] gives the frame hierarchy. [`frame_keys`] walks it
//!    and assigns each frame its *structural* key: the root document is
//!    [`FrameKey::root`], a child frame is its parent's key `.child(ordinal)`.
//!    We key on structure rather than on the `frameId` itself because a reload
//!    reassigns `frameId` while the ordinal path of "the login iframe" survives.
//! 2. The pierced `DOM.getDocument` tree carries each same-origin frame's
//!    document inline, under the iframe owner element's `contentDocument`.
//!    [`map_backends_to_frames`] walks it, threading the current frame and
//!    switching when it descends into a known child frame's document.
//!
//! Both functions are browser-free and operate on the minimal [`FrameNode`] /
//! [`DomNode`] views decoded in `observer.rs`, which keeps the frame logic
//! unit-testable without driving Chrome - the same split that makes
//! [`fuse`](crate::fuse) testable.
//!
//! Cross-origin out-of-process iframes (OOPIFs) live in a separate CDP target
//! and therefore never appear in the root target's pierced document, so they
//! contribute no backends here. Attaching to and observing them is deferred to
//! phase 3.2b; same-origin frames need neither a new session nor owning-session
//! action dispatch because they are pierced into the root target and share its
//! id space.
//!
//! [`Page.getFrameTree`]: https://chromedevtools.github.io/devtools-protocol/tot/Page/#method-getFrameTree

use std::collections::HashMap;

use anchortree_core::FrameKey;

/// A browser-free view of one `Page.FrameTree` node: a frame id and its ordered
/// child frames. Decoded from chromiumoxide in `observer.rs`.
#[derive(Debug, Clone)]
pub struct FrameNode {
    pub frame_id: String,
    pub children: Vec<FrameNode>,
}

/// A browser-free view of a pierced `DOM.Node`: the backend id (absent for some
/// structural nodes), the frame id a frame-owner element carries, the regular
/// children, and the nested document of an iframe owner element.
#[derive(Debug, Clone, Default)]
pub struct DomNode {
    pub backend_node_id: Option<i64>,
    pub frame_id: Option<String>,
    pub children: Vec<DomNode>,
    pub content_document: Option<Box<DomNode>>,
}

/// Assign every frame its structural [`FrameKey`], keyed by frame id.
///
/// The root frame is [`FrameKey::root`]; a frame is its parent's key
/// `.child(ordinal)` where `ordinal` is its zero-based position among its
/// parent's children.
pub fn frame_keys(root: &FrameNode) -> HashMap<String, FrameKey> {
    let mut out = HashMap::new();
    assign(root, FrameKey::root(), &mut out);
    out
}

fn assign(node: &FrameNode, key: FrameKey, out: &mut HashMap<String, FrameKey>) {
    out.insert(node.frame_id.clone(), key.clone());
    for (ordinal, child) in node.children.iter().enumerate() {
        assign(child, key.child(ordinal), out);
    }
}

/// Map every backend id in a pierced DOM tree to the [`FrameKey`] of the frame
/// that owns it, given the frame-id → key table from [`frame_keys`].
///
/// To keep the map small, only *non-root* backends are recorded: an element
/// absent from the result defaults to the root document, which is exactly what
/// [`fuse`](crate::fuse::fuse) does with a missing entry. The walk attributes
/// the iframe owner element itself to the *parent* frame (it lives in the parent
/// document) and only its `contentDocument` subtree to the child frame.
pub fn map_backends_to_frames(
    root: &DomNode,
    frame_keys: &HashMap<String, FrameKey>,
) -> HashMap<i64, FrameKey> {
    let mut out = HashMap::new();
    walk(root, &FrameKey::root(), frame_keys, &mut out);
    out
}

/// Collect the frame ids of the *same-origin* child frames inline in this
/// pierced DOM tree, in document order, deduplicated.
///
/// A same-origin frame is one whose document is present under an iframe owner
/// element's `contentDocument`; the owner element carries the frame's id. These
/// are the frames whose accessibility subtree the observer must fetch with a
/// per-frame `getFullAXTree(frameId)` call - the root `getFullAXTree` stops at
/// the frame boundary. Cross-origin OOPIFs have no inline document here and so
/// are not returned (deferred to 3.2b).
pub fn same_origin_frame_ids(root: &DomNode) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_frame_ids(root, &mut out, &mut seen);
    out
}

fn collect_frame_ids(
    node: &DomNode,
    out: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    if let (Some(id), Some(doc)) = (&node.frame_id, &node.content_document) {
        if seen.insert(id.clone()) {
            out.push(id.clone());
        }
        collect_frame_ids(doc, out, seen);
    }
    for child in &node.children {
        collect_frame_ids(child, out, seen);
    }
}

/// Assign every iframe owner element its structural [`FrameKey`] from the
/// *document order* of the pierced root DOM, covering same-origin and
/// cross-origin frames alike.
///
/// [`frame_keys`] derives the same keys from `Page.getFrameTree`, but that tree
/// omits out-of-process iframes entirely - verified live against Chrome
/// `--site-per-process`: a cross-origin OOPIF's frame never appears in the root
/// target's `getFrameTree`, so [`frame_keys`] cannot key it (this corrects
/// `DECISIONS.md` D22 step 3). The pierced root DOM is the source that does see
/// them: every iframe owner element - whether its document is inlined
/// (same-origin) or lives in another target (an OOPIF, with `content_document`
/// absent) - is present in its parent document and carries its child frame's id.
/// Keying off document order therefore reaches both.
///
/// A frame's key is its parent frame's key `.child(ordinal)`, where `ordinal` is
/// its zero-based position among the iframe owners in the *same* containing
/// document. For a same-origin frame this is exactly what [`frame_keys`]
/// computes; an OOPIF, which `frame_keys` cannot key at all, gets the same
/// structural slot it would have held had its document been inline. The root
/// document has no owner element and so is absent from the map, just as in
/// [`map_backends_to_frames`]. The join key back to a child CDP session is the
/// owner's frame id, which equals the OOPIF target's `targetId`
/// ([`child_frame_keys`]).
pub fn dom_frame_keys(root: &DomNode) -> HashMap<String, FrameKey> {
    let mut out = HashMap::new();
    assign_dom_frames(root, &FrameKey::root(), &mut 0, &mut out);
    out
}

/// Walk one document (rooted at `node`) in document order, numbering the iframe
/// owners it directly contains under `parent`. `ordinal` counts frames seen so
/// far in *this* document; descending into a same-origin child document resets
/// it under the child's key.
fn assign_dom_frames(
    node: &DomNode,
    parent: &FrameKey,
    ordinal: &mut usize,
    out: &mut HashMap<String, FrameKey>,
) {
    for child in &node.children {
        if let Some(frame_id) = &child.frame_id {
            // An iframe owner: it sits in `parent`'s document and hosts a child
            // frame. Number it in document order, then descend into its inline
            // document (same-origin) with a fresh per-document counter under the
            // new key. An OOPIF has no inline document, so the descent is a
            // no-op and the key still stands.
            let key = parent.child(*ordinal);
            *ordinal += 1;
            out.insert(frame_id.clone(), key.clone());
            if let Some(doc) = &child.content_document {
                assign_dom_frames(doc, &key, &mut 0, out);
            }
            // The owner's own light-dom children (iframe fallback content) stay
            // in the parent document under the same counter.
            assign_dom_frames(child, parent, ordinal, out);
        } else {
            assign_dom_frames(child, parent, ordinal, out);
        }
    }
}

/// Join cross-origin child CDP sessions to their durable [`FrameKey`].
///
/// An out-of-process iframe lives in its own CDP target whose `targetId` *is*
/// its own page `frameId` - the id its owner `<iframe>` element carries in the
/// parent's pierced DOM. [`dom_frame_keys`] keys every owner (same-origin or
/// OOPIF) off document order, so a child session's durable identity needs no new
/// computation: read its `targetId` as a frame id and look it up in that table.
/// The result maps each child's `sessionId` to the [`FrameKey`] its observed
/// nodes must be folded under.
///
/// The table must be [`dom_frame_keys`], not [`frame_keys`]: `getFrameTree`
/// omits OOPIF frames (`DECISIONS.md` D22 step 3, amended), so a `frame_keys`
/// table would never contain a cross-origin child's id and every OOPIF join
/// would silently drop.
///
/// A child whose `targetId` is not a known frame id - a dedicated worker, a
/// popup, or a race where the frame already left the tree - is dropped rather
/// than guessed: it has no structural place in this page and so contributes no
/// frame-scoped identity. Input is `(sessionId, targetId)` pairs to keep this
/// module browser-free; the caller decodes them from `Target.attachedToTarget`.
pub fn child_frame_keys<'a>(
    children: impl IntoIterator<Item = (&'a str, &'a str)>,
    frame_keys: &HashMap<String, FrameKey>,
) -> HashMap<String, FrameKey> {
    children
        .into_iter()
        .filter_map(|(session_id, target_id)| {
            frame_keys
                .get(target_id)
                .map(|key| (session_id.to_owned(), key.clone()))
        })
        .collect()
}

fn walk(
    node: &DomNode,
    current: &FrameKey,
    frame_keys: &HashMap<String, FrameKey>,
    out: &mut HashMap<i64, FrameKey>,
) {
    if let Some(backend) = node.backend_node_id
        && !current.is_root()
    {
        out.insert(backend, current.clone());
    }
    for child in &node.children {
        walk(child, current, frame_keys, out);
    }
    if let Some(doc) = &node.content_document {
        // The iframe *owner* node carries the child frame's id; resolve it to a
        // structural key. An unknown frame id (an OOPIF whose document is not in
        // this tree, or a frame missing from getFrameTree) keeps the parent
        // frame rather than inventing an identity.
        let next = node
            .frame_id
            .as_ref()
            .and_then(|id| frame_keys.get(id))
            .cloned()
            .unwrap_or_else(|| current.clone());
        walk(doc, &next, frame_keys, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(id: &str, children: Vec<FrameNode>) -> FrameNode {
        FrameNode {
            frame_id: id.to_string(),
            children,
        }
    }

    fn el(backend: i64, children: Vec<DomNode>) -> DomNode {
        DomNode {
            backend_node_id: Some(backend),
            children,
            ..Default::default()
        }
    }

    /// An iframe owner element: lives in the parent, hosts a child document.
    fn iframe(backend: i64, frame_id: &str, doc: DomNode) -> DomNode {
        DomNode {
            backend_node_id: Some(backend),
            frame_id: Some(frame_id.to_string()),
            content_document: Some(Box::new(doc)),
            ..Default::default()
        }
    }

    /// An out-of-process iframe owner: carries its child frame's id but has no
    /// inline `content_document` (that document lives in a separate CDP target).
    /// This is the node `getFrameTree` omits and `dom_frame_keys` still keys.
    fn oopif(backend: i64, frame_id: &str) -> DomNode {
        DomNode {
            backend_node_id: Some(backend),
            frame_id: Some(frame_id.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn frame_keys_assign_structural_ordinal_paths() {
        let tree = frame(
            "main",
            vec![
                frame("childA", vec![frame("grandchild", vec![])]),
                frame("childB", vec![]),
            ],
        );
        let keys = frame_keys(&tree);
        assert_eq!(keys["main"], FrameKey::root());
        assert_eq!(keys["childA"], FrameKey("0".into()));
        assert_eq!(keys["childB"], FrameKey("1".into()));
        assert_eq!(keys["grandchild"], FrameKey("0.0".into()));
    }

    #[test]
    fn root_backends_are_omitted_frame_backends_are_mapped() {
        // Root doc holds a button (backend 1) and an iframe (backend 2) whose
        // document holds a button (backend 9).
        let dom = el(
            100,
            vec![
                el(1, vec![]),
                iframe(2, "childA", el(50, vec![el(9, vec![])])),
            ],
        );
        let keys = frame_keys(&frame("main", vec![frame("childA", vec![])]));
        let map = map_backends_to_frames(&dom, &keys);
        // Root button and the iframe owner element default to root: absent.
        assert!(!map.contains_key(&1));
        assert!(!map.contains_key(&2));
        // Everything under the child document is the child frame.
        assert_eq!(map[&9], FrameKey("0".into()));
        assert_eq!(map[&50], FrameKey("0".into()));
    }

    #[test]
    fn unknown_child_frame_keeps_parent_frame() {
        // contentDocument present but its owner's frame id is not in the tree
        // (an OOPIF, or a race). The nested backends keep the parent frame
        // rather than minting a bogus key.
        let dom = el(100, vec![iframe(2, "ghost-frame", el(9, vec![]))]);
        let keys = frame_keys(&frame("main", vec![]));
        let map = map_backends_to_frames(&dom, &keys);
        // Parent is root, so unknown child stays root: omitted entirely.
        assert!(map.is_empty());
    }

    #[test]
    fn same_origin_frame_ids_collects_inline_documents_in_order() {
        // Two sibling iframes plus one nested inside the first. Document order
        // is outer-first, depth-first: childA, then its nested grandchild, then
        // childB.
        let dom = el(
            100,
            vec![
                iframe(
                    2,
                    "childA",
                    el(50, vec![iframe(3, "grandchild", el(9, vec![]))]),
                ),
                iframe(4, "childB", el(60, vec![])),
            ],
        );
        assert_eq!(
            same_origin_frame_ids(&dom),
            vec![
                "childA".to_string(),
                "grandchild".to_string(),
                "childB".to_string()
            ]
        );
    }

    #[test]
    fn same_origin_frame_ids_ignores_owners_without_inline_documents() {
        // A frame-owner element whose contentDocument is absent is an OOPIF (its
        // document lives in another target). It carries a frame id but no inline
        // document, so it is not a same-origin frame and is skipped.
        let oopif = DomNode {
            backend_node_id: Some(2),
            frame_id: Some("oopif".into()),
            ..Default::default()
        };
        let dom = el(100, vec![oopif, iframe(3, "same", el(9, vec![]))]);
        assert_eq!(same_origin_frame_ids(&dom), vec!["same".to_string()]);
    }

    #[test]
    fn same_origin_frame_ids_dedups_repeated_ids() {
        // A frame id reached twice (defensive against a malformed tree) is
        // reported once, on first sight.
        let inner = || iframe(2, "dup", el(9, vec![]));
        let dom = el(100, vec![inner(), inner()]);
        assert_eq!(same_origin_frame_ids(&dom), vec!["dup".to_string()]);
    }

    #[test]
    fn dom_frame_keys_agree_with_frame_keys_on_a_same_origin_tree() {
        // The pierced DOM for main -> [childA -> [grandchild], childB]. Every
        // frame here is same-origin, so its owner carries an inline document.
        let dom = el(
            100,
            vec![
                iframe(
                    2,
                    "childA",
                    el(50, vec![iframe(3, "grandchild", el(9, vec![]))]),
                ),
                iframe(4, "childB", el(60, vec![])),
            ],
        );
        let keys = dom_frame_keys(&dom);
        // Same structural keys getFrameTree would assign, minus the root
        // document (it has no owner element, so it is absent from the map).
        assert_eq!(keys["childA"], FrameKey("0".into()));
        assert_eq!(keys["grandchild"], FrameKey("0.0".into()));
        assert_eq!(keys["childB"], FrameKey("1".into()));
        assert_eq!(keys.len(), 3);
        // The getFrameTree path agrees on every non-root frame.
        let tree = frame_keys(&frame(
            "main",
            vec![
                frame("childA", vec![frame("grandchild", vec![])]),
                frame("childB", vec![]),
            ],
        ));
        for (id, key) in &keys {
            assert_eq!(tree[id], *key);
        }
    }

    #[test]
    fn dom_frame_keys_key_an_oopif_owner_without_an_inline_document() {
        // The node getFrameTree omits: an OOPIF owner carrying a frame id with no
        // content_document. dom_frame_keys gives it the structural slot it would
        // have held had its document been inline.
        let dom = el(100, vec![oopif(2, "oopif-A")]);
        let keys = dom_frame_keys(&dom);
        assert_eq!(keys["oopif-A"], FrameKey("0".into()));
        assert_eq!(keys.len(), 1);
        // getFrameTree never sees the OOPIF, so frame_keys cannot key it at all.
        assert!(!frame_keys(&frame("main", vec![])).contains_key("oopif-A"));
    }

    #[test]
    fn dom_frame_keys_number_oopif_and_same_origin_owners_in_document_order() {
        // A document holding an OOPIF first and a same-origin iframe second. Both
        // are numbered by document order in the same containing document; the
        // same-origin child's own nested frame keys under it.
        let dom = el(
            100,
            vec![
                oopif(2, "oopif-A"),
                iframe(
                    4,
                    "same-B",
                    el(60, vec![iframe(5, "nested-C", el(9, vec![]))]),
                ),
            ],
        );
        let keys = dom_frame_keys(&dom);
        assert_eq!(keys["oopif-A"], FrameKey("0".into()));
        assert_eq!(keys["same-B"], FrameKey("1".into()));
        assert_eq!(keys["nested-C"], FrameKey("1.0".into()));
        assert_eq!(keys.len(), 3);
    }

    #[test]
    fn child_frame_keys_join_target_id_to_structural_key() {
        // A page with a cross-origin OOPIF first and a same-origin iframe second.
        // dom_frame_keys keys both off the pierced DOM; an attached child's CDP
        // target id equals its frame id, so its session inherits the frame's key.
        let dom = el(
            100,
            vec![oopif(2, "oopif-A"), iframe(4, "same-B", el(60, vec![]))],
        );
        let keys = dom_frame_keys(&dom);
        // Two attached children: session S1 drives target "oopif-A".
        let children = [("S1", "oopif-A"), ("S2", "same-B")];
        let joined = child_frame_keys(children, &keys);
        assert_eq!(joined["S1"], FrameKey("0".into()));
        assert_eq!(joined["S2"], FrameKey("1".into()));
    }

    #[test]
    fn child_frame_keys_drops_children_without_a_known_frame() {
        // A worker or popup session attaches with a target id that is not a
        // frame in this page's pierced DOM. It has no structural place, so it is
        // dropped rather than assigned a bogus key.
        let dom = el(100, vec![oopif(2, "oopif-A")]);
        let keys = dom_frame_keys(&dom);
        let children = [("S1", "oopif-A"), ("S-worker", "worker-target")];
        let joined = child_frame_keys(children, &keys);
        assert_eq!(joined.len(), 1);
        assert_eq!(joined["S1"], FrameKey("0".into()));
        assert!(!joined.contains_key("S-worker"));
    }

    #[test]
    fn child_frame_keys_maps_the_root_target_to_the_root_key() {
        // The page target itself is keyed root; if it ever appears as a join
        // input it resolves to the root key rather than being dropped. (Frame
        // ids are unique, so this cannot collide with a child.)
        let keys = frame_keys(&frame("main", vec![frame("oopif-A", vec![])]));
        let joined = child_frame_keys([("S-page", "main")], &keys);
        assert_eq!(joined["S-page"], FrameKey::root());
    }
}

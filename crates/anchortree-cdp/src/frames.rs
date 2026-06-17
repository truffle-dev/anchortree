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
}

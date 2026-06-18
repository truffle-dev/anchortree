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
/// structural nodes), the uppercase node name, the frame id a node carries, the
/// regular children, and the nested document of an iframe owner element.
///
/// `node_name` is the upper-cased tag name (`"IFRAME"`, `"HTML"`, `"#document"`,
/// ...). It exists to tell a *frame owner* element apart from the other nodes
/// CDP stamps a `frameId` on: CDP sets `DOM.Node.frameId` on `<iframe>`/`<frame>`
/// owner elements **and also on the `<html>` document element of every frame**
/// (the document element carries its *own* frame's id, not a child's). Frame
/// ownership therefore cannot be inferred from `frame_id` alone; only an
/// `<iframe>`/`<frame>` element actually owns a child frame (see
/// [`assign_dom_frames`]).
#[derive(Debug, Clone, Default)]
pub struct DomNode {
    pub backend_node_id: Option<i64>,
    pub node_name: String,
    pub frame_id: Option<String>,
    pub children: Vec<DomNode>,
    pub content_document: Option<Box<DomNode>>,
    /// For a frame-owner element, a stable discriminator derived from its own
    /// identifying attributes (its `src` origin+path, `name`, `title`, or `id`).
    /// `None` when the owner exposes none (an anonymous `srcdoc`/`about:blank`
    /// iframe), in which case the frame falls back to its document-order ordinal.
    /// `observer.rs` populates this from the iframe's CDP attributes; it is the
    /// frame-tier analogue of an element's in-frame fingerprint (decision D40).
    pub frame_owner_label: Option<String>,
}

/// Whether a node name is a frame-owner element, i.e. a browsing-context
/// container that hosts a *child* frame. CDP also stamps `frameId` on the
/// `<html>` document element of each frame, but that carries the frame's *own*
/// id, not a child's, so it must not be counted as an owner.
pub(crate) fn is_frame_owner_element(node_name: &str) -> bool {
    node_name.eq_ignore_ascii_case("iframe") || node_name.eq_ignore_ascii_case("frame")
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
/// A frame's key is its parent frame's key `.child_segment(seg)`, where `seg` is
/// the owner's durable discriminator ([`frame_owner_label`](DomNode::frame_owner_label),
/// sanitized) when it has one, or its zero-based document-order ordinal as the
/// fallback. The discriminator is what makes the key survive a sibling-frame
/// insert/reorder: a labelled owner keeps its segment even when an iframe is
/// inserted before it and every ordinal shifts (decision D40). For an unlabelled
/// same-origin frame the ordinal segment is exactly what [`frame_keys`] computes;
/// an OOPIF, which `frame_keys` cannot key at all, gets the same slot it would
/// have held had its document been inline. The root document has no owner element
/// and so is absent from the map, just as in [`map_backends_to_frames`]. The join
/// key back to a child CDP session is the owner's frame id, which equals the
/// OOPIF target's `targetId` ([`child_frame_keys`]).
pub fn dom_frame_keys(root: &DomNode) -> HashMap<String, FrameKey> {
    let mut out = HashMap::new();
    assign_dom_frames(
        root,
        &FrameKey::root(),
        &mut FrameCounters::default(),
        &mut out,
    );
    out
}

/// Per-document numbering state for [`assign_dom_frames`]: the running
/// document-order `ordinal` (the fallback segment) and a per-label occurrence
/// count so two owners that share a discriminator (e.g. two `src`-identical ad
/// frames) get distinct keys `label` and `label#1`.
#[derive(Default)]
struct FrameCounters {
    ordinal: usize,
    label_seen: HashMap<String, usize>,
}

/// Build the durable key segment for one frame owner. A sanitized, non-empty
/// [`frame_owner_label`](DomNode::frame_owner_label) wins and is suffixed `#n`
/// on its nth repeat within this document; otherwise the document-order ordinal
/// is the fallback. The ordinal advances for every owner either way, so an
/// unlabelled sibling's fallback still reflects its true document position.
fn owner_segment(owner: &DomNode, counters: &mut FrameCounters) -> String {
    let ordinal = counters.ordinal;
    counters.ordinal += 1;
    match owner
        .frame_owner_label
        .as_deref()
        .map(sanitize_label)
        .filter(|s| !s.is_empty())
    {
        Some(label) => {
            let seen = counters.label_seen.entry(label.clone()).or_insert(0);
            let segment = if *seen == 0 {
                label.clone()
            } else {
                format!("{label}#{seen}")
            };
            *seen += 1;
            segment
        }
        None => ordinal.to_string(),
    }
}

/// Reduce a raw owner label to a stable, key-safe segment: lower-cased, with
/// every character outside `[a-z0-9-_/:]` collapsed to `_`, runs of `_` merged,
/// edges trimmed, and capped at 48 chars. Dots are folded away so a segment can
/// never be confused with the `.`-joined [`FrameKey`] path separator.
fn sanitize_label(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(48));
    let mut last_underscore = false;
    for ch in raw.trim().chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else if matches!(ch, '-' | '_' | '/' | ':') {
            ch
        } else {
            '_'
        };
        if mapped == '_' {
            if last_underscore {
                continue;
            }
            last_underscore = true;
        } else {
            last_underscore = false;
        }
        out.push(mapped);
        if out.len() >= 48 {
            break;
        }
    }
    out.trim_matches('_').to_string()
}

/// Walk one document (rooted at `node`) in document order, keying the iframe
/// owners it directly contains under `parent`. `counters` carries the running
/// ordinal and per-label occurrence counts for *this* document; descending into
/// a same-origin child document resets them under the child's key.
fn assign_dom_frames(
    node: &DomNode,
    parent: &FrameKey,
    counters: &mut FrameCounters,
    out: &mut HashMap<String, FrameKey>,
) {
    for child in &node.children {
        if let Some(frame_id) = &child.frame_id
            && is_frame_owner_element(&child.node_name)
        {
            // An iframe owner: it sits in `parent`'s document and hosts a child
            // frame. The `is_frame_owner_element` guard is load-bearing - CDP
            // also stamps `frameId` on the `<html>` document element of every
            // frame (it carries the frame's *own* id), and without the guard
            // that phantom owner would be counted at ordinal 0, shifting every
            // real iframe's key up by one (verified live against
            // `--site-per-process` Chrome: a sole OOPIF keyed "1" not "0"
            // because the main frame's `<html>` was counted at "0").
            // Key it by its durable discriminator (or document-order ordinal
            // fallback), then descend into its inline document (same-origin)
            // with fresh per-document counters under the new key. An OOPIF has
            // no inline document, so the descent is a no-op and the key stands.
            let key = parent.child_segment(&owner_segment(child, counters));
            out.insert(frame_id.clone(), key.clone());
            if let Some(doc) = &child.content_document {
                assign_dom_frames(doc, &key, &mut FrameCounters::default(), out);
            }
            // The owner's own light-dom children (iframe fallback content) stay
            // in the parent document under the same counters.
            assign_dom_frames(child, parent, counters, out);
        } else {
            assign_dom_frames(child, parent, counters, out);
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
            node_name: "DIV".to_string(),
            children,
            ..Default::default()
        }
    }

    /// An iframe owner element: lives in the parent, hosts a child document.
    fn iframe(backend: i64, frame_id: &str, doc: DomNode) -> DomNode {
        DomNode {
            backend_node_id: Some(backend),
            node_name: "IFRAME".to_string(),
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
            node_name: "IFRAME".to_string(),
            frame_id: Some(frame_id.to_string()),
            ..Default::default()
        }
    }

    /// A same-origin iframe owner carrying a durable discriminator label (its
    /// `src` origin, `name`, `title`, or `id` — observer.rs derives which).
    fn iframe_labeled(backend: i64, frame_id: &str, label: &str, doc: DomNode) -> DomNode {
        DomNode {
            frame_owner_label: Some(label.to_string()),
            ..iframe(backend, frame_id, doc)
        }
    }

    /// An OOPIF owner carrying a durable discriminator label.
    fn oopif_labeled(backend: i64, frame_id: &str, label: &str) -> DomNode {
        DomNode {
            frame_owner_label: Some(label.to_string()),
            ..oopif(backend, frame_id)
        }
    }

    /// The `<html>` document element of a frame, which CDP stamps with that
    /// frame's *own* `frameId` - exactly the phantom that wrongly counted as a
    /// frame owner before the `is_frame_owner_element` guard. It is an element
    /// node (nodeType 1) like an `<iframe>`, so only its name tells them apart.
    /// It wraps the real document content as its children.
    fn html_doc_element(frame_id: &str, children: Vec<DomNode>) -> DomNode {
        DomNode {
            node_name: "HTML".to_string(),
            frame_id: Some(frame_id.to_string()),
            children,
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
    fn dom_frame_keys_ignore_the_html_element_carrying_its_own_frame_id() {
        // The live-Chrome shape that produced the phantom "0" key (D24): the
        // main frame's `<html>` document element carries the main frame's own
        // frameId and wraps the page body, which holds the sole OOPIF owner.
        // The `<html>` is an element node (nodeType 1) just like the `<iframe>`,
        // so before the name guard it counted at ordinal 0 and the OOPIF keyed
        // "1". With `is_frame_owner_element` the `<html>` is transparent and the
        // OOPIF takes its rightful "0".
        let dom = DomNode {
            node_name: "#document".to_string(),
            children: vec![html_doc_element(
                "main-frame",
                vec![el(50, vec![oopif(2, "oopif-A")])],
            )],
            ..Default::default()
        };
        let keys = dom_frame_keys(&dom);
        assert_eq!(keys["oopif-A"], FrameKey("0".into()));
        // The `<html>` element's own frame id is never an owner key.
        assert!(!keys.contains_key("main-frame"));
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn dom_frame_keys_number_owners_across_a_nested_html_element() {
        // Two iframes split by a same-origin child document's `<html>` element
        // in between. The `<html>` carries its document's own frame id but is
        // not an owner, so both real owners number 0 and 1 in true document
        // order rather than 1 and 2.
        let dom = el(
            100,
            vec![
                oopif(2, "first"),
                html_doc_element("inner-frame", vec![iframe(4, "second", el(60, vec![]))]),
            ],
        );
        let keys = dom_frame_keys(&dom);
        assert_eq!(keys["first"], FrameKey("0".into()));
        assert_eq!(keys["second"], FrameKey("1".into()));
        assert!(!keys.contains_key("inner-frame"));
        assert_eq!(keys.len(), 2);
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

    // ---- D40: frame-tier durability via a frame-owner discriminator ----

    #[test]
    fn a_labelled_owner_keys_by_its_discriminator_not_its_ordinal() {
        // A single same-origin iframe whose owner carries a stable label keys by
        // the label, not "0". The label is the durable handle; the ordinal is
        // only the fallback.
        let dom = el(
            100,
            vec![iframe_labeled(2, "login-frame", "login", el(9, vec![]))],
        );
        let keys = dom_frame_keys(&dom);
        assert_eq!(keys["login-frame"], FrameKey("login".into()));
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn unlabelled_owner_reorder_shifts_the_ordinal_key_the_measured_gap() {
        // The D40 gap, stated as a test: with NO discriminator the key is the
        // bare ordinal, so inserting a sibling owner before the target shifts it
        // from "0" to "1" and every in-frame eid would re-mint. This is exactly
        // the fragility the label fixes; the next test shows the fix.
        let before = el(100, vec![iframe(2, "target", el(9, vec![]))]);
        assert_eq!(dom_frame_keys(&before)["target"], FrameKey("0".into()));

        let after = el(
            100,
            vec![
                iframe(3, "inserted", el(8, vec![])),
                iframe(2, "target", el(9, vec![])),
            ],
        );
        // The same frame, now second, keys "1" — the shift the discriminator removes.
        assert_eq!(dom_frame_keys(&after)["target"], FrameKey("1".into()));
    }

    #[test]
    fn labelled_owner_key_survives_a_sibling_inserted_before_it() {
        // The D40 fix: the same reorder, but the target owner carries a label.
        // Its key is "login" before AND after a sibling owner is inserted ahead
        // of it, so its in-frame fingerprints rebind under the same frame key at
        // zero LLM — the frame-tier analogue of the node-tier rebind.
        let before = el(
            100,
            vec![iframe_labeled(2, "login-frame", "login", el(9, vec![]))],
        );
        assert_eq!(
            dom_frame_keys(&before)["login-frame"],
            FrameKey("login".into())
        );

        let after = el(
            100,
            vec![
                iframe_labeled(3, "ad-frame", "ads", el(8, vec![])),
                iframe_labeled(2, "login-frame", "login", el(9, vec![])),
            ],
        );
        let keys = dom_frame_keys(&after);
        // The inserted sibling takes its own label; the target is untouched.
        assert_eq!(keys["ad-frame"], FrameKey("ads".into()));
        assert_eq!(keys["login-frame"], FrameKey("login".into()));
    }

    #[test]
    fn repeated_owner_label_is_deduped_within_a_document() {
        // Two owners sharing a discriminator (e.g. two src-identical ad frames)
        // cannot both key "ads" — the second is suffixed "#1" in document order,
        // so each still resolves to a distinct, stable frame.
        let dom = el(
            100,
            vec![
                iframe_labeled(2, "ad-1", "ads", el(8, vec![])),
                iframe_labeled(3, "ad-2", "ads", el(9, vec![])),
            ],
        );
        let keys = dom_frame_keys(&dom);
        assert_eq!(keys["ad-1"], FrameKey("ads".into()));
        assert_eq!(keys["ad-2"], FrameKey("ads#1".into()));
    }

    #[test]
    fn identical_discriminator_siblings_degrade_to_document_order_on_a_front_insert() {
        // The D41 honesty bound, encoded. Two src-identical `ads` slots key
        // `ads`/`ads#1` (the `#n` suffix is the document-order occurrence count).
        // A THIRD identical `ads` inserted AHEAD of both shifts the suffixes:
        // `ads`/`ads#1`/`ads#2`, so the frames that were `ads` and `ads#1`
        // re-mint. This is not a defect — the owners are genuinely
        // indistinguishable from any author metadata available at frame-keying
        // time (a content fingerprint would need a per-frame AX fetch). The `#n`
        // fallback is document-order parity with Playwright's `.nth()`, the
        // field's best for identical-`src` frames; the durability win is only
        // claimed for DISTINCTLY-identified frames (see the `login`/`ads` test).
        let before = el(
            100,
            vec![
                iframe_labeled(2, "ad-1", "ads", el(7, vec![])),
                iframe_labeled(3, "ad-2", "ads", el(8, vec![])),
            ],
        );
        let before_keys = dom_frame_keys(&before);
        assert_eq!(before_keys["ad-1"], FrameKey("ads".into()));
        assert_eq!(before_keys["ad-2"], FrameKey("ads#1".into()));

        let after = el(
            100,
            vec![
                iframe_labeled(4, "ad-0", "ads", el(6, vec![])),
                iframe_labeled(2, "ad-1", "ads", el(7, vec![])),
                iframe_labeled(3, "ad-2", "ads", el(8, vec![])),
            ],
        );
        let after_keys = dom_frame_keys(&after);
        assert_eq!(after_keys["ad-0"], FrameKey("ads".into()));
        assert_eq!(after_keys["ad-1"], FrameKey("ads#1".into()));
        assert_eq!(after_keys["ad-2"], FrameKey("ads#2".into()));
        // The bound, stated as an assertion: the previously-stable keys moved, so
        // those frames' eids would re-mint. Distinct labels would not have.
        assert_ne!(before_keys["ad-1"], after_keys["ad-1"]);
        assert_ne!(before_keys["ad-2"], after_keys["ad-2"]);
    }

    #[test]
    fn labelled_owners_mix_with_unlabelled_ordinal_fallbacks() {
        // A labelled owner and an unlabelled one in the same document: the label
        // wins its segment, the unlabelled one falls back to its true
        // document-order ordinal (the ordinal advances for every owner, labelled
        // or not, so the fallback reflects real position).
        let dom = el(
            100,
            vec![
                iframe_labeled(2, "login-frame", "login", el(8, vec![])),
                iframe(3, "anon-frame", el(9, vec![])),
            ],
        );
        let keys = dom_frame_keys(&dom);
        assert_eq!(keys["login-frame"], FrameKey("login".into()));
        // The anonymous frame is the second owner, so its ordinal fallback is "1".
        assert_eq!(keys["anon-frame"], FrameKey("1".into()));
    }

    #[test]
    fn labelled_frame_keys_compose_through_nesting() {
        // A labelled parent frame with a labelled child composes the segments on
        // the dot path, so the child is "shop.cart" — durable end to end.
        let dom = el(
            100,
            vec![iframe_labeled(
                2,
                "shop-frame",
                "shop",
                el(
                    50,
                    vec![iframe_labeled(3, "cart-frame", "cart", el(9, vec![]))],
                ),
            )],
        );
        let keys = dom_frame_keys(&dom);
        assert_eq!(keys["shop-frame"], FrameKey("shop".into()));
        assert_eq!(keys["cart-frame"], FrameKey("shop.cart".into()));
    }

    #[test]
    fn an_oopif_owner_label_keys_the_cross_origin_frame() {
        // The discriminator reaches OOPIFs too: a cross-origin owner with a
        // label keys by it, and its child session inherits that durable key.
        let dom = el(100, vec![oopif_labeled(2, "pay-target", "checkout")]);
        let keys = dom_frame_keys(&dom);
        assert_eq!(keys["pay-target"], FrameKey("checkout".into()));
        let joined = child_frame_keys([("S1", "pay-target")], &keys);
        assert_eq!(joined["S1"], FrameKey("checkout".into()));
    }

    #[test]
    fn sanitize_label_folds_unsafe_chars_caps_length_and_lowercases() {
        // Dots fold to "_" so a segment can never split the FrameKey path; case
        // is normalized; path-ish chars survive; runs of unsafe chars collapse;
        // edges trim; length caps at 48.
        assert_eq!(super::sanitize_label("Login Form"), "login_form");
        assert_eq!(
            super::sanitize_label("https://pay.example.com/checkout"),
            "https://pay_example_com/checkout"
        );
        assert_eq!(super::sanitize_label("  weird!!name  "), "weird_name");
        assert_eq!(super::sanitize_label("..."), "");
        assert_eq!(super::sanitize_label(&"x".repeat(80)).len(), 48);
    }
}

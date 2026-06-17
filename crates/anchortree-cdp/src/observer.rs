//! The live CDP adapter: one observation pass over a real browser.
//!
//! [`CdpObserver`] is the thin, browser-facing half of this crate. It owns a
//! [`chromiumoxide::Page`], enables the Accessibility and DOM domains, and on
//! each [`observe`](anchortree_core::ObservationSource::observe) call issues the
//! three CDP requests an observation needs:
//!
//! 1. `Accessibility.getFullAXTree` for roles, names, values, and state.
//! 2. `DOM.getAttributes` for the developer-stable id/name/test-id attributes
//!    that anchor the strongest rebind rung.
//! 3. `DOM.getBoxModel` for layout geometry (the bounding box and centroid).
//!
//! All three replies are decoded into the plain [`fuse`](crate::fuse) inputs and
//! handed to [`fuse`](crate::fuse::fuse), which does the actual identity-bearing
//! work without any knowledge of a browser. That keeps every interesting policy
//! decision unit-testable in `fuse.rs`; this module stays a mechanical
//! request/decode loop.
//!
//! Only attributes and layout for *observable* nodes are fetched (see
//! [`observable_backends`](crate::fuse::observable_backends)), so a page with
//! thousands of DOM nodes still costs one AX call plus a bounded handful of
//! per-element calls.
//!
//! ## The listener pass (Phase 2.5)
//!
//! The ARIA-role filter misses custom widgets: a `<div>` wired with a click
//! handler and no role is invisible to it. Before deciding the keep-set, the
//! observer takes a *secondary* pass over the role-less residual of the tree
//! ([`residual_backends`](crate::fuse::residual_backends)) and asks
//! `DOMDebugger.getEventListeners` which of those nodes carry interactive
//! listeners. `getEventListeners` is keyed on a `Runtime.RemoteObjectId`, not a
//! backend id, so each residual node costs a `DOM.resolveNode` hop first. That
//! is why this is a residual-only pass and never a whole-tree scan: it touches
//! only the nodes the cheap role filter could not already classify.

use std::collections::HashMap;

use anchortree_core::{
    Bbox, Binding, Eid, FrameKey, IdentityMap, Observation, ObservationSource, ObservedNode,
};
use chromiumoxide::cdp::browser_protocol::accessibility::{
    AxNode, AxPropertyName, EnableParams as AxEnableParams, GetFullAxTreeParams,
};
use chromiumoxide::cdp::browser_protocol::dom::{
    BackendNodeId, EnableParams as DomEnableParams, GetAttributesParams, GetBoxModelParams,
    GetDocumentParams, Node, PushNodesByBackendIdsToFrontendParams, ResolveNodeParams,
};
use chromiumoxide::cdp::browser_protocol::dom_debugger::GetEventListenersParams;
use chromiumoxide::cdp::browser_protocol::page::{FrameId, FrameTree, GetFrameTreeParams};
use chromiumoxide::cdp::js_protocol::runtime::ReleaseObjectGroupParams;
use chromiumoxide::{Browser, Command, Page};
use futures::StreamExt as _;

use crate::actions::{ActError, Action};
use crate::channel::CdpChannel;
use crate::error::CdpError;
use crate::frames::{
    DomNode, FrameNode, dom_frame_keys, frame_keys, map_backends_to_frames, same_origin_frame_ids,
};
use crate::fuse::{
    ListenerRoles, RawAttrs, RawAxNode, RawAxProperty, fuse, observable_backends,
    residual_backends, role_for_listeners,
};

/// CDP object group the listener pass resolves nodes into, released wholesale at
/// the end of each pass so the renderer does not retain a handle per residual
/// node across observations.
const LISTENER_OBJECT_GROUP: &str = "anchortree-listeners";

/// A live observation source backed by a CDP transport.
///
/// Generic over the [`CdpChannel`] it drives, so the entire observation
/// pipeline — the AX/DOM requests, the listener pass, the decode and fuse — runs
/// unchanged whether the underlying transport is a locally launched
/// [`chromiumoxide::Page`] (the default, see [`connect`]) or a hosted
/// [`RawCdpSession`](crate::channel::RawCdpSession) flat-attached to a page a
/// gateway already has open (see [`connect_hosted`](crate::channel::connect_hosted)).
///
/// Construct one with [`CdpObserver::attach`] from any channel, or use
/// [`connect`] to get a fully wired [`Session`] from just a CDP WebSocket URL.
pub struct CdpObserver<C = Page> {
    channel: C,
    /// Live out-of-process child sessions, keyed by target id (== the OOPIF's
    /// page frame id). Refreshed every pass from
    /// [`auto_attach_children`](CdpChannel::auto_attach_children): Chrome
    /// announces a child once, on the `setAutoAttach` call that first sees it,
    /// and does not re-announce it on later calls, so a child observed across
    /// two passes (e.g. an `innerHTML` swap inside the frame, which keeps the
    /// target alive) must be remembered here rather than re-discovered. Empty
    /// for a local [`Page`], whose `auto_attach_children` yields none.
    oopif_sessions: HashMap<String, String>,
    /// Routing table for action dispatch: each live out-of-process frame's
    /// durable structural [`FrameKey`] to the child `sessionId` that owns it.
    /// Rebuilt every observation from the same `(target_id, session_id) ->
    /// frame_key` join that drives [`observe_oopif_children`](Self::observe_oopif_children),
    /// so it never names a frame the last pass did not see. Root and in-process
    /// frames are deliberately absent (they are addressable from the page
    /// session by `backendNodeId`); a lookup miss therefore means "dispatch on
    /// the page session", which is exactly right for them. This is the dispatch
    /// half of D23: an action on an OOPIF eid lands in the frame it was observed
    /// in. Empty for a local [`Page`].
    frame_sessions: HashMap<FrameKey, String>,
}

impl CdpObserver<Page> {
    /// Borrow the underlying page, e.g. to navigate or run actions before
    /// observing. Only the local (`chromiumoxide::Page`) transport exposes a
    /// `Page`; the hosted transport drives commands through
    /// [`channel`](CdpObserver::channel) instead.
    pub fn page(&self) -> &Page {
        &self.channel
    }
}

impl<C: CdpChannel> CdpObserver<C> {
    /// Enable the Accessibility and DOM domains on `channel` and return an
    /// observer bound to it.
    ///
    /// Both domains are idempotent to enable, so attaching twice to the same
    /// page is harmless.
    pub async fn attach(channel: C) -> Result<Self, CdpError> {
        channel.run(AxEnableParams::default()).await?;
        channel.run(DomEnableParams::default()).await?;
        Ok(Self {
            channel,
            oopif_sessions: HashMap::new(),
            frame_sessions: HashMap::new(),
        })
    }

    /// Resolve `eid` against `map` and perform `action`, routing the dispatch to
    /// the CDP session that owns the eid's frame.
    ///
    /// This is the consumer-facing action entry point and the dispatch half of
    /// D23. An agent holds an [`Eid`] (which may be OOPIF-namespaced, e.g.
    /// `f0/btn-buy-now`) and the live [`IdentityMap`]; this resolves it now,
    /// through the durable `backendNodeId`, and tags the trusted gesture with
    /// the right session so it lands in the frame the identity was observed in:
    ///
    /// * a root or in-process eid is addressable from the page session, so it
    ///   dispatches there (`session = None`);
    /// * an out-of-process iframe eid dispatches on its owning child session,
    ///   looked up from the [`frame_sessions`](Self::frame_sessions) table the
    ///   last observation refreshed.
    ///
    /// The routing reads the eid's [`frame_key`](Binding::frame_key) from its
    /// live binding; an unbound eid falls through to the page session, where the
    /// primitive [`act`](crate::actions::act) reports it as
    /// [`ActError::UnknownEid`].
    pub async fn act(&self, map: &IdentityMap, eid: &Eid, action: Action) -> Result<(), ActError> {
        let session = self.session_for_binding(map.binding(eid));
        crate::actions::act(&self.channel, session, map, eid, action).await
    }

    /// Perform `action` on the transient [`Mark`](anchortree_core::Mark) at
    /// `index` within `obs`, dispatched on the page session.
    ///
    /// A [`Mark`](anchortree_core::Mark) is the single-turn handle for an
    /// element the engine could not give a durable eid, and it carries only a
    /// `backendNodeId`, never a frame — so there is nothing to route on, and
    /// this dispatches on the page session. OOPIF marks are out of scope at this
    /// level; an unanchorable OOPIF element is a corner the engine does not yet
    /// reach across the process boundary.
    pub async fn act_mark(
        &self,
        obs: &Observation,
        index: usize,
        action: Action,
    ) -> Result<(), ActError> {
        crate::actions::act_mark(&self.channel, None, obs, index, action).await
    }

    /// Owning child session for a binding's frame, or `None` for the root and
    /// in-process frames (which the page session already addresses).
    fn session_for_binding(&self, binding: Option<&Binding>) -> Option<&str> {
        binding
            .and_then(|b| self.frame_sessions.get(&b.frame_key))
            .map(String::as_str)
    }

    /// Borrow the underlying CDP channel, e.g. to issue a navigate or evaluate
    /// command alongside observations.
    pub(crate) fn channel(&self) -> &C {
        &self.channel
    }

    /// Infer roles for the role-less residual of `ax` from their bound DOM event
    /// listeners. For each residual backend: `DOM.resolveNode` to a JS object,
    /// `DOMDebugger.getEventListeners` for that object, then
    /// [`role_for_listeners`] over the listener types attached to *that* node.
    ///
    /// Every step is tolerant: a node that fails to resolve or has no object id
    /// simply contributes nothing, so one odd element never sinks the pass. The
    /// resolved objects share one CDP object group, released at the end so the
    /// renderer keeps no per-node handle between observations.
    async fn listener_roles(&self, ax: &[RawAxNode]) -> ListenerRoles {
        let residual = residual_backends(ax);
        let mut roles = ListenerRoles::new();
        if residual.is_empty() {
            return roles;
        }

        for backend in residual {
            let Ok(resolved) = self
                .channel
                .run(
                    ResolveNodeParams::builder()
                        .backend_node_id(BackendNodeId::new(backend))
                        .object_group(LISTENER_OBJECT_GROUP)
                        .build(),
                )
                .await
            else {
                continue;
            };
            let Some(object_id) = resolved.object.object_id else {
                continue;
            };

            let Ok(listeners) = self
                .channel
                .run(GetEventListenersParams::new(object_id))
                .await
            else {
                continue;
            };

            // `getEventListeners` can report listeners on descendant nodes too;
            // count only those bound to the node we resolved (a listener with no
            // backend id is reported against the resolved object itself).
            let types: Vec<String> = listeners
                .listeners
                .iter()
                .filter(|l| {
                    l.backend_node_id
                        .as_ref()
                        .map(|b| *b.inner() == backend)
                        .unwrap_or(true)
                })
                .map(|l| l.r#type.clone())
                .collect();

            if let Some(role) = role_for_listeners(&types) {
                roles.insert(backend, role);
            }
        }

        // Drop the renderer-side handles for the whole pass in one call. A
        // failure here is non-fatal: it only means a few JS objects linger until
        // the next navigation, never a wrong observation.
        let _ = self
            .channel
            .run(ReleaseObjectGroupParams::new(LISTENER_OBJECT_GROUP))
            .await;

        roles
    }

    /// Run `cmd` on a specific child session, or on the channel's own page
    /// session when `session` is `None`.
    ///
    /// The `None` arm is the root path, byte-identical to `self.channel.run`;
    /// the `Some` arm tags an out-of-process child session via
    /// [`run_on`](CdpChannel::run_on). One seam lets [`attrs_and_layout`] and
    /// the per-frame fetches serve the root and an OOPIF child unchanged.
    async fn run_sel<T>(&self, session: Option<&str>, cmd: T) -> Result<T::Response, CdpError>
    where
        T: Command + Send + 'static,
        T::Response: Send,
    {
        match session {
            None => self.channel.run(cmd).await,
            Some(sid) => self.channel.run_on(Some(sid), cmd).await,
        }
    }

    /// Fetch stable DOM attributes and layout boxes for `backends` on the given
    /// session (`None` = the page session, `Some` = an OOPIF child session).
    ///
    /// Empty `backends` returns empty maps without a round-trip. Per-node
    /// attribute and box failures are tolerated (a node may legitimately have
    /// neither; a detached node fails outright) so one odd element never sinks
    /// the pass; a missing layout entry is exactly how [`fuse`] encodes "not
    /// visible".
    async fn attrs_and_layout(
        &self,
        session: Option<&str>,
        backends: &[i64],
    ) -> Result<(HashMap<i64, RawAttrs>, HashMap<i64, Bbox>), CdpError> {
        let mut attrs: HashMap<i64, RawAttrs> = HashMap::new();
        let mut layout: HashMap<i64, Bbox> = HashMap::new();
        if backends.is_empty() {
            return Ok((attrs, layout));
        }

        // Resolve backend ids to frontend node ids in one round-trip so we can
        // ask for DOM attributes (which are keyed on the frontend id).
        let node_ids = self
            .run_sel(
                session,
                PushNodesByBackendIdsToFrontendParams::new(
                    backends.iter().map(|b| BackendNodeId::new(*b)).collect(),
                ),
            )
            .await?
            .node_ids;

        for (backend, node_id) in backends.iter().zip(node_ids.iter()) {
            if let Ok(resp) = self
                .run_sel(session, GetAttributesParams::new(*node_id))
                .await
            {
                attrs.insert(*backend, RawAttrs::from_flat(&resp.attributes));
            }
            if let Ok(resp) = self
                .run_sel(
                    session,
                    GetBoxModelParams::builder()
                        .backend_node_id(BackendNodeId::new(*backend))
                        .build(),
                )
                .await
            {
                if let Some(bbox) = quad_to_bbox(resp.model.content.inner()) {
                    layout.insert(*backend, bbox);
                }
            }
        }
        Ok((attrs, layout))
    }

    /// Run the CDP requests for the root page pass plus one pass per live
    /// out-of-process child frame, decoding each into the [`fuse`] inputs.
    ///
    /// Returns one [`FramePass`] per CDP session — the root document first, then
    /// one per OOPIF child — which [`observe`](ObservationSource::observe) fuses
    /// independently and concatenates. Fusing per-session is what keeps a child
    /// target's separate `backendNodeId` and AX-node-id spaces from colliding
    /// with the root's; the identities still compose globally because each child
    /// node is stamped with the OOPIF's durable [`FrameKey`] and the core map
    /// keys by `(FrameKey, backendNodeId)` (`DECISIONS.md` D21, D23).
    async fn raw_pass(&mut self) -> Result<Vec<FramePass>, CdpError> {
        // 1. The root pierced DOM tree, fetched first because it does double
        //    duty. It primes the DOM agent (`pushNodesByBackendIdsToFrontend`
        //    and the attribute fetch answer `-32000 "Document needs to be
        //    requested first"` until the tree has been requested at least once
        //    this session) and it is the first tier of durable identity: it
        //    carries every same-origin frame's document inline, so we derive the
        //    `backend -> FrameKey` map from it together with the frame hierarchy
        //    (D21). We re-request each pass because a navigation or re-render
        //    invalidates the frontend node-id space the push hands back.
        //
        //    A cross-origin OOPIF is a separate target absent from this pierced
        //    tree; it is observed below as its own session and merged as a
        //    distinct [`FramePass`] (D22/D23).
        let document = self
            .channel
            .run(GetDocumentParams::builder().depth(-1).pierce(true).build())
            .await?
            .root;
        let dom = decode_dom_node(&document);
        let frame_tree = self
            .channel
            .run(GetFrameTreeParams::default())
            .await?
            .frame_tree;
        let frame_map = map_backends_to_frames(&dom, &frame_keys(&decode_frame_tree(&frame_tree)));

        // 2. The accessibility tree. `getFullAXTree` with no frame id stops at
        //    every frame boundary, so it only yields the root document's nodes.
        //    Each same-origin frame's elements live behind a per-frame
        //    `getFullAXTree(frameId)` call; we issue one per inline frame and
        //    concatenate. Backend ids are unique across the root target's pierced
        //    id space, so the frame_map above can attribute each merged node to
        //    its frame with no risk of collision (D21, AX-per-frame correction).
        let mut ax: Vec<RawAxNode> = self
            .channel
            .run(GetFullAxTreeParams::default())
            .await?
            .nodes
            .iter()
            .map(decode_ax_node)
            .collect();
        for frame_id in same_origin_frame_ids(&dom) {
            // A frame whose AX tree fails to fetch (mid-navigation, just
            // detached) simply contributes no nodes; one odd frame never sinks
            // the pass.
            if let Ok(resp) = self
                .channel
                .run(
                    GetFullAxTreeParams::builder()
                        .frame_id(FrameId::new(frame_id))
                        .build(),
                )
                .await
            {
                ax.extend(resp.nodes.iter().map(decode_ax_node));
            }
        }

        // 2a. Promote role-less custom widgets via their event listeners, so the
        //     keep-set below includes them alongside the ARIA-role nodes.
        let listener_roles = self.listener_roles(&ax).await;

        // 3. Attributes and layout for the observable keep-set only, never the
        //    whole tree.
        let backends = observable_backends(&ax, &listener_roles);
        let (attrs, layout) = self.attrs_and_layout(None, &backends).await?;

        let mut passes = vec![FramePass {
            ax,
            attrs,
            layout,
            listener_roles,
            frame_map,
        }];

        // 4. Out-of-process child frames, each its own session. A local `Page`
        //    surfaces none, so this is a no-op there.
        self.observe_oopif_children(&dom, &mut passes).await;

        Ok(passes)
    }

    /// Discover and observe the page's out-of-process child frames, appending a
    /// [`FramePass`] for each live one.
    ///
    /// Chrome announces a child target exactly once — on the `setAutoAttach`
    /// call that first sees it — so the session ids are cached in
    /// `oopif_sessions` across passes and a child that persists through a
    /// re-render (e.g. an `innerHTML` swap inside the frame) is re-observed from
    /// the cache rather than re-discovered. A child whose owner `<iframe>` has
    /// left the root DOM, or whose session has gone stale, is dropped from the
    /// cache and contributes nothing — one dead child never sinks the pass.
    async fn observe_oopif_children(&mut self, dom: &DomNode, passes: &mut Vec<FramePass>) {
        // Fold any newly-attached iframe children into the persistent cache. A
        // failure here means only that no *new* child surfaced this pass; the
        // cache (and any already-known OOPIF) still stands.
        if let Ok(children) = self.channel.auto_attach_children().await {
            for child in children {
                if child.target_type == "iframe" {
                    self.oopif_sessions
                        .insert(child.target_id, child.session_id);
                }
            }
        }
        // Rebuilt from scratch each pass so a frame that left the DOM (or whose
        // session went stale) does not linger as a stale dispatch route. Cleared
        // before the early return below so an empty OOPIF set empties the table.
        self.frame_sessions.clear();
        if self.oopif_sessions.is_empty() {
            return;
        }

        // Join each known child target to its durable structural frame key via
        // the root DOM's iframe-owner document order (D22, amended).
        let keys = dom_frame_keys(dom);
        let known: Vec<(String, String)> = self
            .oopif_sessions
            .iter()
            .map(|(t, s)| (t.clone(), s.clone()))
            .collect();
        for (target_id, session_id) in known {
            let Some(frame_key) = keys.get(&target_id).cloned() else {
                // The OOPIF owner is gone from the root DOM: the frame was
                // removed. Forget the stale session.
                self.oopif_sessions.remove(&target_id);
                continue;
            };
            // Record the route before observing the child: an OOPIF with no
            // observable nodes this pass (Ok(None)) is still a live, dispatchable
            // frame and must stay routable.
            self.frame_sessions
                .insert(frame_key.clone(), session_id.clone());
            match self.child_pass(&session_id, frame_key.clone()).await {
                Ok(Some(pass)) => passes.push(pass),
                Ok(None) => {}
                Err(_) => {
                    self.oopif_sessions.remove(&target_id);
                    self.frame_sessions.remove(&frame_key);
                }
            }
        }
    }

    /// Observe one out-of-process child frame as its own CDP session.
    ///
    /// Enables the AX + DOM domains on the child session (idempotent), fetches
    /// its pierced DOM (which primes the child DOM agent) and its root AX tree,
    /// then attributes and layout for the observable nodes. Every node is
    /// stamped with `frame_key` — the OOPIF's durable structural key — so the
    /// fused identities land in the right frame namespace.
    ///
    /// Scoped to one OOPIF level: a frame nested *inside* the OOPIF is not yet
    /// walked, and listener-inferred roles inside the child are deferred (pure
    /// ARIA roles for now). Returns `Ok(None)` when the child has no nodes.
    async fn child_pass(
        &self,
        session_id: &str,
        frame_key: FrameKey,
    ) -> Result<Option<FramePass>, CdpError> {
        let sid = Some(session_id);
        // Enable on the child session; idempotent, and a child that refuses them
        // fails the document fetch below and is skipped by the caller.
        let _ = self.channel.run_on(sid, AxEnableParams::default()).await;
        let _ = self.channel.run_on(sid, DomEnableParams::default()).await;

        // Prime the child DOM agent (the attribute and push calls need it). The
        // decoded tree itself is not walked at this one-level scope.
        self.channel
            .run_on(
                sid,
                GetDocumentParams::builder().depth(-1).pierce(true).build(),
            )
            .await?;

        let ax: Vec<RawAxNode> = self
            .channel
            .run_on(sid, GetFullAxTreeParams::default())
            .await?
            .nodes
            .iter()
            .map(decode_ax_node)
            .collect();
        if ax.is_empty() {
            return Ok(None);
        }

        let listener_roles = ListenerRoles::new();
        let backends = observable_backends(&ax, &listener_roles);
        let (attrs, layout) = self.attrs_and_layout(sid, &backends).await?;

        // Every node in this one-level child belongs to the OOPIF frame.
        let frame_map: HashMap<i64, FrameKey> = ax
            .iter()
            .filter_map(|n| n.backend_node_id)
            .map(|b| (b, frame_key.clone()))
            .collect();

        Ok(Some(FramePass {
            ax,
            attrs,
            layout,
            listener_roles,
            frame_map,
        }))
    }
}

/// One CDP session's worth of decoded [`fuse`] inputs: the root document, or one
/// out-of-process child frame. Fused independently of every other pass so a
/// child target's separate `backendNodeId` and AX-node-id spaces never collide
/// with the root's.
struct FramePass {
    ax: Vec<RawAxNode>,
    attrs: HashMap<i64, RawAttrs>,
    layout: HashMap<i64, Bbox>,
    listener_roles: ListenerRoles,
    frame_map: HashMap<i64, FrameKey>,
}

impl<C: CdpChannel> ObservationSource for CdpObserver<C> {
    type Error = CdpError;

    async fn observe(&mut self) -> Result<Vec<ObservedNode>, Self::Error> {
        let passes = self.raw_pass().await?;
        let mut out = Vec::new();
        for pass in &passes {
            out.extend(fuse(
                &pass.ax,
                &pass.attrs,
                &pass.layout,
                &pass.listener_roles,
                &pass.frame_map,
            ));
        }
        Ok(out)
    }
}

/// Decode a chromiumoxide `Page.FrameTree` into the browser-free
/// [`FrameNode`](crate::frames::FrameNode) the frame-key logic consumes.
pub(crate) fn decode_frame_tree(tree: &FrameTree) -> FrameNode {
    FrameNode {
        frame_id: tree.frame.id.inner().clone(),
        children: tree
            .child_frames
            .iter()
            .flatten()
            .map(decode_frame_tree)
            .collect(),
    }
}

/// Decode a chromiumoxide pierced `DOM.Node` into the browser-free
/// [`DomNode`](crate::frames::DomNode), keeping only the fields the frame walk
/// needs: backend id, frame-owner id, children, and the nested content document.
pub(crate) fn decode_dom_node(node: &Node) -> DomNode {
    DomNode {
        backend_node_id: Some(*node.backend_node_id.inner()),
        node_name: node.node_name.clone(),
        frame_id: node.frame_id.as_ref().map(|f| f.inner().clone()),
        children: node
            .children
            .iter()
            .flatten()
            .map(decode_dom_node)
            .collect(),
        content_document: node
            .content_document
            .as_ref()
            .map(|d| Box::new(decode_dom_node(d))),
    }
}

/// Decode one CDP [`AxNode`] into the browser-free [`RawAxNode`] the fusion
/// understands. Only the fields the engine keys on are carried across.
fn decode_ax_node(node: &AxNode) -> RawAxNode {
    RawAxNode {
        ax_node_id: node.node_id.inner().clone(),
        backend_node_id: node.backend_dom_node_id.as_ref().map(|b| *b.inner()),
        ignored: node.ignored,
        role: ax_value_string(node.role.as_ref()),
        name: ax_value_string(node.name.as_ref()),
        value: ax_value_string(node.value.as_ref()),
        child_ids: node
            .child_ids
            .as_ref()
            .map(|ids| ids.iter().map(|c| c.inner().clone()).collect())
            .unwrap_or_default(),
        properties: node
            .properties
            .as_ref()
            .map(|props| {
                props
                    .iter()
                    .filter_map(|p| {
                        property_token(&p.name).map(|name| RawAxProperty {
                            name: name.to_owned(),
                            value: p.value.value.clone().unwrap_or(serde_json::Value::Null),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Pull the string payload out of an `AXValue`. The role/name/value of an AX
/// node are reported as JSON values; the engine only wants the string form.
fn ax_value_string(
    value: Option<&chromiumoxide::cdp::browser_protocol::accessibility::AxValue>,
) -> Option<String> {
    value.and_then(|v| v.value.as_ref()).and_then(|j| match j {
        serde_json::Value::String(s) => Some(s.clone()),
        // An explicit JSON null is "no value", not the literal text "null".
        serde_json::Value::Null => None,
        // Numbers/booleans (a slider's `valuenow`, a pressed state) render
        // to their compact form; `valuetext` overrides this in fuse.
        other => Some(other.to_string()),
    })
}

/// Map the strongly-typed [`AxPropertyName`] to the lowercase token
/// [`extract_state`](crate::fuse) reads. Only the state-bearing properties the
/// engine acts on are kept; everything else is dropped here so the fusion never
/// sees noise.
fn property_token(name: &AxPropertyName) -> Option<&'static str> {
    match name {
        AxPropertyName::Disabled => Some("disabled"),
        AxPropertyName::Focused => Some("focused"),
        AxPropertyName::Required => Some("required"),
        AxPropertyName::Selected => Some("selected"),
        AxPropertyName::Checked => Some("checked"),
        AxPropertyName::Expanded => Some("expanded"),
        AxPropertyName::Hidden => Some("hidden"),
        AxPropertyName::Valuetext => Some("valuetext"),
        _ => None,
    }
}

/// Turn a `DOM.getBoxModel` content quad (eight interleaved `x,y` coordinates,
/// clockwise from the top-left) into an axis-aligned [`Bbox`]. Returns `None`
/// for a degenerate quad so a zero-area element reads as "no box".
fn quad_to_bbox(quad: &[f64]) -> Option<Bbox> {
    if quad.len() < 8 {
        return None;
    }
    let xs = [quad[0], quad[2], quad[4], quad[6]];
    let ys = [quad[1], quad[3], quad[5], quad[7]];
    let min_x = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let max_x = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min_y = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let max_y = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (w, h) = (max_x - min_x, max_y - min_y);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    Some(Bbox {
        x: min_x as f32,
        y: min_y as f32,
        w: w as f32,
        h: h as f32,
    })
}

/// A connected browser plus an observer, with the CDP event handler driven for
/// you.
///
/// `chromiumoxide` splits a connection into a [`Browser`] handle and a
/// [`Handler`](chromiumoxide::Handler) stream that must be polled for any
/// command to make progress. [`connect`] spawns that polling onto the current
/// Tokio runtime and hands back this struct, which keeps the browser alive and
/// aborts the handler task on drop.
pub struct Session {
    /// The observation source. This is what you call `observe()` on.
    pub observer: CdpObserver,
    /// Kept alive so the connection is not dropped while the session lives.
    _browser: Browser,
    handler_task: tokio::task::JoinHandle<()>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.handler_task.abort();
    }
}

/// True if `url` is a TLS WebSocket endpoint (`wss://`) rather than a plain
/// `ws://` one.
///
/// Hosted CDP gateways are `wss://` — Cloudflare Browser Run
/// (`wss://api.cloudflare.com/client/v4/accounts/<id>/browser-rendering/devtools/browser`)
/// and Browserbase both terminate TLS — while a locally launched Chrome exposes
/// a plain `ws://` `webSocketDebuggerUrl`. The scheme match is case-insensitive
/// and tolerates leading whitespace, so a URL pasted from a console still
/// classifies correctly.
pub fn is_tls_endpoint(url: &str) -> bool {
    let url = url.trim_start();
    url.len() >= 6 && url[..6].eq_ignore_ascii_case("wss://")
}

/// Install the `ring` rustls crypto provider as the process default, once.
///
/// `async-tungstenite`'s rustls connector builds its `ClientConfig` through
/// `rustls::ClientConfig::builder()`, which reads the process-default
/// [`CryptoProvider`](rustls::crypto::CryptoProvider). This crate compiles
/// rustls with the `ring` provider only (DECISIONS D10: ring builds in this
/// toolchain, aws-lc-rs does not), so that builder already resolves to ring in
/// isolation. But if a *downstream* crate in the final binary also links
/// `aws-lc-rs`, two providers exist and the unqualified `builder()` panics
/// unless a default has been installed. Installing ring up front makes the
/// `wss://` path deterministic regardless of the wider dependency graph.
///
/// Idempotent and race-tolerant: a second call, or one losing a race to another
/// crate that already installed a default, is silently ignored.
///
/// Shared with [`crate::gateway`], whose reqwest client is built on the same
/// provider-less rustls (`rustls-no-provider`) and needs the same default
/// installed before it can negotiate TLS to a hosted gateway's HTTP API.
pub(crate) fn ensure_ring_provider() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Connect to a CDP browser over a WebSocket URL, open a blank page, and return
/// a ready [`Session`].
///
/// `ws_url` may be either a plain `ws://` endpoint (e.g. the
/// `webSocketDebuggerUrl` from a local Chrome's `/json/version`) or a TLS
/// `wss://` endpoint exposed by a hosted gateway such as Cloudflare Browser Run
/// or Browserbase. TLS is handled by rustls on the `ring` provider; for
/// `wss://` URLs the provider is installed automatically before the handshake
/// (see [`is_tls_endpoint`]). The TLS stack trusts the bundled Mozilla
/// `webpki-roots`, so no system certificate store is required.
///
/// Must be called from within a Tokio runtime: the CDP event handler is driven
/// by a spawned task.
pub async fn connect(ws_url: impl Into<String>) -> Result<Session, CdpError> {
    let ws_url = ws_url.into();
    if is_tls_endpoint(&ws_url) {
        ensure_ring_provider();
    }
    let (browser, mut handler) = Browser::connect(ws_url).await?;
    let handler_task = tokio::spawn(async move { while handler.next().await.is_some() {} });
    let page = browser.new_page("about:blank").await?;
    let observer = CdpObserver::attach(page).await?;
    Ok(Session {
        observer,
        _browser: browser,
        handler_task,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_to_bbox_reads_axis_aligned_extent() {
        // A 80x24 box at (10, 0), clockwise from top-left.
        let quad = vec![10.0, 0.0, 90.0, 0.0, 90.0, 24.0, 10.0, 24.0];
        let bbox = quad_to_bbox(&quad).expect("non-degenerate quad yields a box");
        assert_eq!((bbox.x, bbox.y, bbox.w, bbox.h), (10.0, 0.0, 80.0, 24.0));
    }

    #[test]
    fn quad_to_bbox_rejects_degenerate_and_short_quads() {
        // Zero-area: a collapsed (display:none-style) box reads as no box.
        let zero = vec![5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0];
        assert!(quad_to_bbox(&zero).is_none());
        // A malformed short quad never panics; it is simply absent.
        assert!(quad_to_bbox(&[1.0, 2.0]).is_none());
    }

    #[test]
    fn property_token_keeps_only_state_bearing_names() {
        assert_eq!(property_token(&AxPropertyName::Checked), Some("checked"));
        assert_eq!(property_token(&AxPropertyName::Disabled), Some("disabled"));
        assert_eq!(
            property_token(&AxPropertyName::Valuetext),
            Some("valuetext")
        );
        // A non-state property (e.g. live-region politeness) is dropped.
        assert_eq!(property_token(&AxPropertyName::Live), None);
    }

    #[test]
    fn ax_value_string_reads_strings_numbers_and_treats_null_as_absent() {
        use chromiumoxide::cdp::browser_protocol::accessibility::{AxValue, AxValueType};
        let val = |json: serde_json::Value| AxValue {
            r#type: AxValueType::String,
            value: Some(json),
            related_nodes: None,
            sources: None,
        };
        assert_eq!(
            ax_value_string(Some(&val(serde_json::json!("hello")))),
            Some("hello".to_owned())
        );
        assert_eq!(
            ax_value_string(Some(&val(serde_json::json!(70)))),
            Some("70".to_owned())
        );
        // An explicit JSON null is "no value", never the literal text "null".
        assert_eq!(ax_value_string(Some(&val(serde_json::Value::Null))), None);
        assert_eq!(ax_value_string(None), None);
    }

    /// The heart of Phase 1.3: decode a *recorded* `Accessibility.getFullAXTree`
    /// reply through the real `chromiumoxide` types and the live decode path,
    /// then fuse it. This exercises `decode_ax_node` + `ax_value_string` +
    /// `property_token` against the exact JSON shape Chrome puts on the wire,
    /// without driving a browser. The fixture below is a trimmed but faithful
    /// capture: a root web area, a text input with a typed value, a range
    /// slider whose `valuetext` differs from its numeric `valuenow`, a
    /// tri-state checkbox, and an ignored presentational node.
    #[test]
    fn recorded_ax_tree_decodes_and_fuses_with_value_fidelity() {
        use anchortree_core::Role;

        // A real getFullAXTree reply is `{ "nodes": [ ... ] }`; we deserialize
        // the node array straight into chromiumoxide's `AxNode`.
        let recorded = serde_json::json!([
            {
                "nodeId": "1", "ignored": false,
                "role": { "type": "internalRole", "value": "RootWebArea" },
                "name": { "type": "computedString", "value": "Settings" },
                "childIds": ["2", "3", "4", "5"],
                "backendDOMNodeId": 1
            },
            {
                "nodeId": "2", "ignored": false,
                "role": { "type": "role", "value": "textbox" },
                "name": { "type": "computedString", "value": "Email" },
                "value": { "type": "string", "value": "jane@example.com" },
                "properties": [
                    { "name": "focused", "value": { "type": "boolean", "value": true } },
                    { "name": "required", "value": { "type": "booleanOrUndefined", "value": true } }
                ],
                "backendDOMNodeId": 2
            },
            {
                "nodeId": "3", "ignored": false,
                "role": { "type": "role", "value": "slider" },
                "name": { "type": "computedString", "value": "Volume" },
                "value": { "type": "number", "value": 70 },
                "properties": [
                    { "name": "valuemin", "value": { "type": "number", "value": 0 } },
                    { "name": "valuemax", "value": { "type": "number", "value": 100 } },
                    { "name": "valuetext", "value": { "type": "computedString", "value": "70%" } }
                ],
                "backendDOMNodeId": 3
            },
            {
                "nodeId": "4", "ignored": false,
                "role": { "type": "role", "value": "checkbox" },
                "name": { "type": "computedString", "value": "Select all" },
                "properties": [
                    { "name": "checked", "value": { "type": "tristate", "value": "mixed" } }
                ],
                "backendDOMNodeId": 4
            },
            {
                "nodeId": "5", "ignored": true,
                "role": { "type": "role", "value": "presentation" },
                "name": { "type": "computedString", "value": "" },
                "backendDOMNodeId": 5
            }
        ]);

        let nodes: Vec<AxNode> =
            serde_json::from_value(recorded).expect("recorded reply deserializes into AxNode");
        let ax: Vec<RawAxNode> = nodes.iter().map(decode_ax_node).collect();

        // The keep-set is exactly the three interactive widgets; the root web
        // area and the ignored presentational node are out.
        let mut backends = observable_backends(&ax, &ListenerRoles::new());
        backends.sort_unstable();
        assert_eq!(backends, vec![2, 3, 4]);

        // Layout + attributes the observer would have fetched for the keep-set.
        let mut layout = HashMap::new();
        layout.insert(
            2,
            Bbox {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 24.0,
            },
        );
        layout.insert(
            3,
            Bbox {
                x: 0.0,
                y: 40.0,
                w: 200.0,
                h: 16.0,
            },
        );
        layout.insert(
            4,
            Bbox {
                x: 0.0,
                y: 70.0,
                w: 16.0,
                h: 16.0,
            },
        );
        let mut attrs = HashMap::new();
        attrs.insert(2, RawAttrs::from_flat(&["id".into(), "email".into()]));

        let observed = fuse(&ax, &attrs, &layout, &ListenerRoles::new(), &HashMap::new());
        assert_eq!(observed.len(), 3, "only the three widgets survive fusion");

        let by_backend = |b: i64| observed.iter().find(|n| n.backend_node_id == b).unwrap();

        let textbox = by_backend(2);
        assert_eq!(textbox.fingerprint.role, Role::Textbox);
        assert_eq!(textbox.state.value.as_deref(), Some("jane@example.com"));
        assert!(textbox.state.focused);
        assert!(textbox.state.required);
        assert_eq!(textbox.fingerprint.stable_attr.as_deref(), Some("email"));

        // Value fidelity: the slider's human `valuetext` ("70%") wins over its
        // raw numeric `valuenow` (70).
        let slider = by_backend(3);
        assert_eq!(slider.fingerprint.role, Role::Slider);
        assert_eq!(slider.state.value.as_deref(), Some("70%"));

        // Tri-state "mixed" reads as checked.
        let checkbox = by_backend(4);
        assert_eq!(checkbox.fingerprint.role, Role::Checkbox);
        assert!(checkbox.state.checked);
    }

    #[test]
    fn is_tls_endpoint_classifies_by_scheme() {
        assert!(is_tls_endpoint("wss://api.cloudflare.com/.../browser"));
        assert!(is_tls_endpoint("wss://connect.browserbase.com?apiKey=x"));
        // Case-insensitive and whitespace-tolerant, since URLs get pasted.
        assert!(is_tls_endpoint("WSS://host/path"));
        assert!(is_tls_endpoint("  wss://host/path"));
        // Plain ws:// is not TLS; nor is anything else.
        assert!(!is_tls_endpoint("ws://127.0.0.1:9222/devtools/browser/abc"));
        assert!(!is_tls_endpoint("https://example.com"));
        assert!(!is_tls_endpoint("wss:/host")); // malformed, missing a slash
        assert!(!is_tls_endpoint(""));
    }

    #[test]
    fn ensure_ring_provider_is_idempotent_and_leaves_a_default_installed() {
        // Installing twice must not panic, and a process-default crypto provider
        // must exist afterwards so async-tungstenite's `ClientConfig::builder()`
        // can resolve one on the wss:// path.
        ensure_ring_provider();
        ensure_ring_provider();
        assert!(
            rustls::crypto::CryptoProvider::get_default().is_some(),
            "a default CryptoProvider is installed after ensure_ring_provider"
        );
    }
}

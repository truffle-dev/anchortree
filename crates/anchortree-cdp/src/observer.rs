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

use anchortree_core::{Bbox, FrameKey, ObservationSource, ObservedNode};
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
use chromiumoxide::{Browser, Page};
use futures::StreamExt as _;

use crate::channel::CdpChannel;
use crate::error::CdpError;
use crate::frames::{
    DomNode, FrameNode, frame_keys, map_backends_to_frames, same_origin_frame_ids,
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
        Ok(Self { channel })
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

    /// Run the CDP requests for one pass and decode them into the [`fuse`]
    /// inputs, including the listener-inferred roles for the role-less residual.
    async fn raw_pass(
        &self,
    ) -> Result<
        (
            Vec<RawAxNode>,
            HashMap<i64, RawAttrs>,
            HashMap<i64, Bbox>,
            ListenerRoles,
            HashMap<i64, FrameKey>,
        ),
        CdpError,
    > {
        // 1. The pierced DOM tree, fetched first because it does double duty.
        //    It primes the DOM agent (`pushNodesByBackendIdsToFrontend` and the
        //    attribute fetch below answer `-32000 "Document needs to be
        //    requested first"` until the tree has been requested at least once
        //    this session) and it is the first tier of durable identity: it
        //    carries every same-origin frame's document inline, so we derive the
        //    `backend -> FrameKey` map from it together with the frame hierarchy
        //    (D21). We re-request each pass because a navigation or re-render
        //    invalidates the frontend node-id space the push hands back.
        //
        //    Cross-origin OOPIFs are a separate target and never appear in this
        //    pierced tree, so they contribute no frame backends and no per-frame
        //    AX fetch (deferred to 3.2b).
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

        // The keep-set fuse would use: fetch attributes and layout only for
        // these, never for the whole tree.
        let backends = observable_backends(&ax, &listener_roles);

        let mut attrs: HashMap<i64, RawAttrs> = HashMap::new();
        let mut layout: HashMap<i64, Bbox> = HashMap::new();
        if backends.is_empty() {
            return Ok((ax, attrs, layout, listener_roles, frame_map));
        }

        // 3. Resolve backend ids to frontend node ids in one round-trip so we
        //    can ask for DOM attributes (which are keyed on the frontend id).
        let node_ids = self
            .channel
            .run(PushNodesByBackendIdsToFrontendParams::new(
                backends.iter().map(|b| BackendNodeId::new(*b)).collect(),
            ))
            .await?
            .node_ids;

        for (backend, node_id) in backends.iter().zip(node_ids.iter()) {
            // 3a. Stable DOM attributes. A node may legitimately have none, and
            //     a detached node can fail outright; tolerate both so one odd
            //     element never sinks the whole pass.
            if let Ok(resp) = self.channel.run(GetAttributesParams::new(*node_id)).await {
                let raw = RawAttrs::from_flat(&resp.attributes);
                attrs.insert(*backend, raw);
            }

            // 3b. Layout geometry. `getBoxModel` errors for nodes with no box
            //     (display:none, detached); a missing entry is exactly how
            //     fuse encodes "not visible", so we simply skip on error.
            if let Ok(resp) = self
                .channel
                .run(
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

        Ok((ax, attrs, layout, listener_roles, frame_map))
    }
}

impl<C: CdpChannel> ObservationSource for CdpObserver<C> {
    type Error = CdpError;

    async fn observe(&mut self) -> Result<Vec<ObservedNode>, Self::Error> {
        let (ax, attrs, layout, listener_roles, frame_map) = self.raw_pass().await?;
        Ok(fuse(&ax, &attrs, &layout, &listener_roles, &frame_map))
    }
}

/// Decode a chromiumoxide `Page.FrameTree` into the browser-free
/// [`FrameNode`](crate::frames::FrameNode) the frame-key logic consumes.
fn decode_frame_tree(tree: &FrameTree) -> FrameNode {
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
fn decode_dom_node(node: &Node) -> DomNode {
    DomNode {
        backend_node_id: Some(*node.backend_node_id.inner()),
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

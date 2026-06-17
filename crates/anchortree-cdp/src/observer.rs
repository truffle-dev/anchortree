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

use std::collections::HashMap;

use anchortree_core::{Bbox, ObservationSource, ObservedNode};
use chromiumoxide::cdp::browser_protocol::accessibility::{
    AxNode, AxPropertyName, EnableParams as AxEnableParams, GetFullAxTreeParams,
};
use chromiumoxide::cdp::browser_protocol::dom::{
    BackendNodeId, EnableParams as DomEnableParams, GetAttributesParams, GetBoxModelParams,
    PushNodesByBackendIdsToFrontendParams,
};
use chromiumoxide::{Browser, Page};
use futures::StreamExt as _;

use crate::error::CdpError;
use crate::fuse::{RawAttrs, RawAxNode, RawAxProperty, fuse, observable_backends};

/// A live observation source backed by a CDP page.
///
/// Construct one with [`CdpObserver::attach`] from an already-open
/// [`chromiumoxide::Page`], or use [`connect`] to get a fully wired
/// [`Session`] from just a CDP WebSocket URL.
pub struct CdpObserver {
    page: Page,
}

impl CdpObserver {
    /// Enable the Accessibility and DOM domains on `page` and return an
    /// observer bound to it.
    ///
    /// Both domains are idempotent to enable, so attaching twice to the same
    /// page is harmless.
    pub async fn attach(page: Page) -> Result<Self, CdpError> {
        page.execute(AxEnableParams::default()).await?;
        page.execute(DomEnableParams::default()).await?;
        Ok(Self { page })
    }

    /// Borrow the underlying page, e.g. to navigate before observing.
    pub fn page(&self) -> &Page {
        &self.page
    }

    /// Run the three CDP requests and decode them into the [`fuse`] inputs.
    async fn raw_pass(
        &self,
    ) -> Result<(Vec<RawAxNode>, HashMap<i64, RawAttrs>, HashMap<i64, Bbox>), CdpError> {
        // 1. The full accessibility tree.
        let tree = self
            .page
            .execute(GetFullAxTreeParams::default())
            .await?
            .result
            .nodes;
        let ax: Vec<RawAxNode> = tree.iter().map(decode_ax_node).collect();

        // The keep-set fuse would use: fetch attributes and layout only for
        // these, never for the whole tree.
        let backends = observable_backends(&ax);

        let mut attrs: HashMap<i64, RawAttrs> = HashMap::new();
        let mut layout: HashMap<i64, Bbox> = HashMap::new();
        if backends.is_empty() {
            return Ok((ax, attrs, layout));
        }

        // 2. Resolve backend ids to frontend node ids in one round-trip so we
        //    can ask for DOM attributes (which are keyed on the frontend id).
        let node_ids = self
            .page
            .execute(PushNodesByBackendIdsToFrontendParams::new(
                backends.iter().map(|b| BackendNodeId::new(*b)).collect(),
            ))
            .await?
            .result
            .node_ids;

        for (backend, node_id) in backends.iter().zip(node_ids.iter()) {
            // 2a. Stable DOM attributes. A node may legitimately have none, and
            //     a detached node can fail outright; tolerate both so one odd
            //     element never sinks the whole pass.
            if let Ok(resp) = self.page.execute(GetAttributesParams::new(*node_id)).await {
                let raw = RawAttrs::from_flat(&resp.result.attributes);
                attrs.insert(*backend, raw);
            }

            // 2b. Layout geometry. `getBoxModel` errors for nodes with no box
            //     (display:none, detached); a missing entry is exactly how
            //     fuse encodes "not visible", so we simply skip on error.
            if let Ok(resp) = self
                .page
                .execute(
                    GetBoxModelParams::builder()
                        .backend_node_id(BackendNodeId::new(*backend))
                        .build(),
                )
                .await
            {
                if let Some(bbox) = quad_to_bbox(resp.result.model.content.inner()) {
                    layout.insert(*backend, bbox);
                }
            }
        }

        Ok((ax, attrs, layout))
    }
}

impl ObservationSource for CdpObserver {
    type Error = CdpError;

    async fn observe(&mut self) -> Result<Vec<ObservedNode>, Self::Error> {
        let (ax, attrs, layout) = self.raw_pass().await?;
        Ok(fuse(&ax, &attrs, &layout))
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
    value.and_then(|v| v.value.as_ref()).map(|j| match j {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
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

/// Connect to a CDP browser over a WebSocket URL, open a blank page, and return
/// a ready [`Session`].
///
/// `ws_url` is a non-TLS CDP endpoint (`ws://...`), e.g. the
/// `webSocketDebuggerUrl` from a local Chrome's `/json/version`. TLS endpoints
/// (`wss://`) are not yet supported; see `DECISIONS.md`.
///
/// Must be called from within a Tokio runtime: the CDP event handler is driven
/// by a spawned task.
pub async fn connect(ws_url: impl Into<String>) -> Result<Session, CdpError> {
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
        // A non-state property (e.g. live-region politeness) is dropped.
        assert_eq!(property_token(&AxPropertyName::Live), None);
    }
}

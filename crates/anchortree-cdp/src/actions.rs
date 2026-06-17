//! The action space: turn a durable [`Eid`] into a real, trusted browser action.
//!
//! This is the other half of the loop. [`observer`](crate::observer) reads the
//! page into logical identities; this module spends one of those identities to
//! *do* something — click a button, type into a field, choose an option — and
//! does it in a way that survives the exact churn the engine was built for.
//!
//! ## Why resolve through the identity map
//!
//! An agent decides to click `btn-save` while looking at one render of the
//! page. By the time the click is dispatched the framework may have torn that
//! button down and rebuilt it: same logical control, brand-new DOM node, brand
//! -new frontend `nodeId`. A handle captured at decision time would be stale.
//!
//! So the action carries only the [`Eid`]. We resolve it *at action time*
//! through the live [`IdentityMap`] to the element's `backendNodeId` — the one
//! handle CDP keeps stable across a re-render — and re-ground from there. The
//! agent never holds a node; it holds an identity, and the identity is resolved
//! against the freshest binding the map has.
//!
//! ## Why the CDP Input domain, not `element.click()`
//!
//! Calling `element.click()` from page script produces an event with
//! `isTrusted: false`. Real sites gate real behaviour (form submits, file
//! pickers, drag starts, some analytics) on trusted events. We therefore drive
//! the pointer and keyboard through the CDP **Input** domain, which synthesises
//! events the page cannot distinguish from a human's: `isTrusted: true`.
//!
//! The one deliberate exception is [`Action::Select`]. There is no portable,
//! trusted gesture for "choose option *N* of a native `<select>`" — the real
//! interaction is an OS-drawn popup. The robust path is to set `.value` and
//! dispatch `input`/`change` in page context via `Runtime.callFunctionOn`,
//! which is exactly what a framework's own controlled-component handler
//! listens for. This is documented as decision D12.

use anchortree_core::{BackendNodeId as LogicalBackendId, Eid, IdentityMap, Observation};
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::dom::{
    BackendNodeId, FocusParams, GetContentQuadsParams, ResolveNodeParams,
    ScrollIntoViewIfNeededParams,
};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventType, InsertTextParams, MouseButton,
};
use chromiumoxide::cdp::js_protocol::runtime::CallFunctionOnParams;
use chromiumoxide::error::CdpError as ChromeCdpError;

/// What to do to an element, once it is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// A trusted left click at the centre of the element's first content quad.
    Click,
    /// Focus the element and insert `text`. With `clear`, the element's current
    /// value is emptied (and an `input` event fired) before the text goes in,
    /// so typing into a pre-filled field replaces rather than appends.
    Type {
        /// The text to insert as trusted keyboard input.
        text: String,
        /// Whether to clear the field's existing value first.
        clear: bool,
    },
    /// Set a native `<select>` (or any element with a `.value`) to `value` and
    /// fire `input` + `change`, mirroring a user's choice. See the module note
    /// on why this one action runs in page context.
    Select {
        /// The option value to select.
        value: String,
    },
}

/// Why an action could not be carried out.
#[derive(Debug, thiserror::Error)]
pub enum ActError {
    /// The [`Eid`] is not bound in the [`IdentityMap`]. Either it was never
    /// observed, or it was removed and never rebound. The caller should
    /// re-observe and look again rather than retry blindly.
    #[error("no live binding for eid `{0}`")]
    UnknownEid(String),

    /// No [`Mark`](anchortree_core::Mark) with this positional index exists in
    /// the [`Observation`] the action was issued against. A mark is valid only
    /// for the turn that produced it; reusing one from a stale observation, or
    /// passing an out-of-range index, lands here. Re-observe and act on a fresh
    /// mark rather than retrying.
    #[error("no mark with index `{0}` in this observation")]
    UnknownMark(usize),

    /// The element resolved to a node with no hittable area: `getContentQuads`
    /// came back empty. The element is off-screen, collapsed to zero size,
    /// `display:none`, or detached. Not a transport failure — a state the agent
    /// should react to (scroll, wait, or pick a different target).
    #[error("eid `{0}` resolved but has no hittable content quad")]
    NotHittable(String),

    /// `Runtime.resolveNode` returned without a remote object id, so the element
    /// could not be addressed in page context for a [`Action::Select`] or a
    /// clearing [`Action::Type`].
    #[error("eid `{0}` could not be resolved to a page-context object")]
    Unresolvable(String),

    /// The underlying CDP transport failed.
    #[error("cdp transport error: {0}")]
    Cdp(#[from] ChromeCdpError),
}

/// Resolve `eid` against the live map and perform `action` on the element it is
/// bound to, re-grounding through the durable `backendNodeId` at call time.
///
/// The element is never addressed by a handle captured earlier; it is addressed
/// by identity, resolved now. That is what makes an action issued against a
/// just-re-rendered page land on the right control instead of a dead node.
pub async fn act(
    page: &Page,
    map: &IdentityMap,
    eid: &Eid,
    action: Action,
) -> Result<(), ActError> {
    let backend: LogicalBackendId = map
        .binding(eid)
        .ok_or_else(|| ActError::UnknownEid(eid.0.clone()))?
        .backend_node_id;

    act_on_backend(page, &eid.0, backend, action).await
}

/// Perform `action` on the transient [`Mark`](anchortree_core::Mark) at `index`
/// within `obs`.
///
/// This is the action counterpart for the unanchorable elements the engine
/// could not give a durable [`Eid`] (see [`anchortree_core::observation`] and
/// decision D13). Unlike [`act`], a mark carries its own `backendNodeId` — it is
/// resolved straight from the observation, **not** through the identity map,
/// because a mark is single-turn by design and was never bound.
///
/// A mark is valid only for the [`Observation`] that produced it. If the page
/// re-rendered since `obs` was taken, the captured node may be gone and the
/// action fails loudly ([`ActError::NotHittable`] or [`ActError::UnknownMark`]),
/// which is the correct single-turn contract, not a bug. Re-observe and act on a
/// fresh mark.
pub async fn act_mark(
    page: &Page,
    obs: &Observation,
    index: usize,
    action: Action,
) -> Result<(), ActError> {
    let mark = obs.mark(index).ok_or(ActError::UnknownMark(index))?;
    act_on_backend(page, &mark.id(), mark.backend_node_id, action).await
}

/// Dispatch `action` against a resolved `backend` node, using `label` (an eid
/// like `btn-save` or a mark id like `m3`) only for error messages. The shared
/// core of [`act`] and [`act_mark`]: both resolve a handle to a `backendNodeId`
/// their own way, then funnel through here so the trusted-input machinery lives
/// in exactly one place.
async fn act_on_backend(
    page: &Page,
    label: &str,
    backend: LogicalBackendId,
    action: Action,
) -> Result<(), ActError> {
    match action {
        Action::Click => click(page, label, backend).await,
        Action::Type { text, clear } => type_text(page, label, backend, &text, clear).await,
        Action::Select { value } => select_value(page, label, backend, &value).await,
    }
}

/// Trusted left click at the centre of the element's first content quad.
///
/// We re-fetch the quad immediately before dispatching so the coordinates are
/// the element's *current* on-screen box, not a remembered one. The sequence is
/// move → press → release, which is what a real pointer emits and what hover and
/// active-state handlers expect to see in order.
async fn click(page: &Page, label: &str, backend: LogicalBackendId) -> Result<(), ActError> {
    let id = BackendNodeId::new(backend);

    page.execute(
        ScrollIntoViewIfNeededParams::builder()
            .backend_node_id(id)
            .build(),
    )
    .await?;

    let quads = page
        .execute(GetContentQuadsParams::builder().backend_node_id(id).build())
        .await?;

    let (x, y) = quads
        .result
        .quads
        .first()
        .and_then(|q| quad_centroid(q.inner()))
        .ok_or_else(|| ActError::NotHittable(label.to_string()))?;

    // Move first so hover/active handlers see a pointer arrive before it presses.
    page.execute(DispatchMouseEventParams::new(
        DispatchMouseEventType::MouseMoved,
        x,
        y,
    ))
    .await?;

    let mut press = DispatchMouseEventParams::new(DispatchMouseEventType::MousePressed, x, y);
    press.button = Some(MouseButton::Left);
    press.buttons = Some(1);
    press.click_count = Some(1);
    page.execute(press).await?;

    let mut release = DispatchMouseEventParams::new(DispatchMouseEventType::MouseReleased, x, y);
    release.button = Some(MouseButton::Left);
    release.buttons = Some(1);
    release.click_count = Some(1);
    page.execute(release).await?;

    Ok(())
}

/// Focus the element and insert `text` as trusted keyboard input, optionally
/// clearing the existing value first.
///
/// `Input.insertText` is the trusted analogue of typing: it produces the same
/// `beforeinput`/`input` events a keyboard would, without us having to model
/// every keystroke. The optional clear runs in page context because there is no
/// single trusted gesture for "select all and delete" that is portable across
/// inputs, textareas, and contenteditables; setting `.value=''` and firing
/// `input` is what a controlled component reacts to.
async fn type_text(
    page: &Page,
    label: &str,
    backend: LogicalBackendId,
    text: &str,
    clear: bool,
) -> Result<(), ActError> {
    let id = BackendNodeId::new(backend);

    page.execute(
        ScrollIntoViewIfNeededParams::builder()
            .backend_node_id(id)
            .build(),
    )
    .await?;
    page.execute(FocusParams::builder().backend_node_id(id).build())
        .await?;

    if clear {
        call_on_backend(page, label, backend, CLEAR_SCRIPT).await?;
    }

    page.execute(InsertTextParams::new(text.to_string()))
        .await?;
    Ok(())
}

/// Set the element's `.value` to `value` and fire `input` + `change` in page
/// context — the documented exception to the trusted-events rule (D12).
async fn select_value(
    page: &Page,
    label: &str,
    backend: LogicalBackendId,
    value: &str,
) -> Result<(), ActError> {
    call_on_backend(page, label, backend, &select_script(value)).await
}

/// Resolve `backend` to a page-context remote object and invoke
/// `function_declaration` with that element as `this`.
///
/// Used for the two page-context actions (clear-before-type and select). The
/// resolve is by `backendNodeId`, so it re-grounds to whatever node currently
/// carries the identity, consistent with the click path.
async fn call_on_backend(
    page: &Page,
    label: &str,
    backend: LogicalBackendId,
    function_declaration: &str,
) -> Result<(), ActError> {
    let resolved = page
        .execute(
            ResolveNodeParams::builder()
                .backend_node_id(BackendNodeId::new(backend))
                .build(),
        )
        .await?;
    let object_id = resolved
        .result
        .object
        .object_id
        .clone()
        .ok_or_else(|| ActError::Unresolvable(label.to_string()))?;

    let mut call = CallFunctionOnParams::new(function_declaration.to_string());
    call.object_id = Some(object_id);
    page.execute(call).await?;
    Ok(())
}

/// Page-context function that empties a value-bearing element and fires `input`,
/// so a clearing type replaces a controlled component's state instead of
/// appending to it.
const CLEAR_SCRIPT: &str =
    "function(){ this.value = ''; this.dispatchEvent(new Event('input', { bubbles: true })); }";

/// Build the page-context function that sets `.value` to `value` and fires the
/// `input` + `change` pair a framework listens for.
///
/// `value` is embedded as a JSON-encoded string literal, so any quotes,
/// backslashes, or newlines in it are escaped into a safe JavaScript string and
/// cannot break out of the literal or inject code.
fn select_script(value: &str) -> String {
    let literal = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        "function(){{ this.value = {literal}; \
         this.dispatchEvent(new Event('input', {{ bubbles: true }})); \
         this.dispatchEvent(new Event('change', {{ bubbles: true }})); }}"
    )
}

/// Centre of a CDP content quad: the mean of its four corner points.
///
/// `getContentQuads` returns each quad as eight numbers — `x1,y1,…,x4,y4`. The
/// centroid of the four corners is a stable interior point to aim a click at,
/// robust to rotation and to the quad not being axis-aligned. Returns `None`
/// for a malformed (too-short) quad so the caller can report it as not hittable
/// rather than aiming at the origin.
fn quad_centroid(quad: &[f64]) -> Option<(f64, f64)> {
    if quad.len() < 8 {
        return None;
    }
    let (sx, sy) = (0..4).fold((0.0, 0.0), |(sx, sy), i| {
        (sx + quad[i * 2], sy + quad[i * 2 + 1])
    });
    Some((sx / 4.0, sy / 4.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_centroid_is_the_mean_of_four_corners() {
        // A 10x20 axis-aligned rectangle anchored at (100, 200).
        let quad = [100.0, 200.0, 110.0, 200.0, 110.0, 220.0, 100.0, 220.0];
        assert_eq!(quad_centroid(&quad), Some((105.0, 210.0)));
    }

    #[test]
    fn quad_centroid_handles_a_rotated_quad() {
        // A diamond: corners at the midpoints of a 0..2 square's edges.
        let quad = [1.0, 0.0, 2.0, 1.0, 1.0, 2.0, 0.0, 1.0];
        assert_eq!(quad_centroid(&quad), Some((1.0, 1.0)));
    }

    #[test]
    fn quad_centroid_rejects_a_short_quad() {
        assert_eq!(quad_centroid(&[1.0, 2.0, 3.0]), None);
        assert_eq!(quad_centroid(&[]), None);
    }

    #[test]
    fn quad_centroid_ignores_extra_trailing_numbers() {
        // Only the first four points define the box; a longer slice still works.
        let quad = [0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0, 999.0, 999.0];
        assert_eq!(quad_centroid(&quad), Some((2.0, 2.0)));
    }

    #[test]
    fn select_script_escapes_the_value_into_a_safe_literal() {
        let script = select_script("a\"; alert(1); //");
        // The dangerous characters survive only inside a JSON string literal.
        assert!(script.contains(r#"this.value = "a\"; alert(1); //""#));
        // And the event-dispatch tail is intact.
        assert!(script.contains("new Event('change'"));
        assert!(script.contains("new Event('input'"));
    }

    #[test]
    fn select_script_handles_plain_values() {
        let script = select_script("medium");
        assert!(script.contains(r#"this.value = "medium""#));
    }

    #[test]
    fn clear_script_empties_and_fires_input() {
        assert!(CLEAR_SCRIPT.contains("this.value = ''"));
        assert!(CLEAR_SCRIPT.contains("new Event('input'"));
    }
}

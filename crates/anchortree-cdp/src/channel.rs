//! A thin CDP command channel, and the hosted-connect leg built on it.
//!
//! ## Why this exists
//!
//! The local `ws://` path ([`connect`](crate::connect)) drives a freshly opened
//! `about:blank` page through [`chromiumoxide::Page`]. That is the right tool for
//! a browser we launched. It is the *wrong* tool for a browser a hosted gateway
//! (Cloudflare Browser Run, Browserbase) already has running with a page open:
//! chromiumoxide 0.9.1 cannot cleanly attach to that pre-existing page (see
//! `DECISIONS.md` D19). `new_page` panics on a `Target.createTarget` /
//! `targetCreated` race, `fetch_targets` attaches a *non-flat* session whose
//! commands fail `-32001`, and target discovery alone never materializes the
//! page. There is no `HandlerConfig` lever for flat auto-attach, the crate's
//! `Page` only builds from a crate-private `PageInner`, and `Browser::execute`
//! is sessionless — so neither bumping the dependency nor wrapping its session
//! is reachable (`DECISIONS.md` D20).
//!
//! ## What this is
//!
//! A self-contained CDP channel that does the flat attach itself. It connects
//! the WebSocket directly (the `wss://` TLS lift from Phase 1.5b already pulled
//! `async-tungstenite` + rustls into the tree), issues
//! `Target.attachToTarget { flatten: true }` once against the page the browser
//! already has open, captures the returned `sessionId`, and tags every later
//! command with it (the "flat" protocol mode where one socket multiplexes the
//! browser session and a page session by `sessionId`). It reuses the typed
//! [`chromiumoxide_cdp`](chromiumoxide::cdp) command structs for
//! (de)serialization, so there is no second copy of the protocol — only a second
//! *transport*.
//!
//! The whole point is to share the observation logic, not fork it. [`CdpChannel`]
//! abstracts "run one typed command and hand back its typed response"; both
//! [`chromiumoxide::Page`] (local) and [`RawCdpSession`] (hosted) implement it,
//! and [`CdpObserver`](crate::observer::CdpObserver) is generic over it. The
//! Phase 1.3–2.5 fusion, the listener pass, the decode helpers — all of it runs
//! unchanged over either transport.
//!
//! ## Scope and limits
//!
//! This drives the observe → re-render → observe rebind loop against a hosted
//! page. It is deliberately minimal: the read side drains and discards CDP
//! *events* (the observer subscribes to none), so a command's response is found
//! by matching its `id`. The socket is only polled inside [`CdpChannel::run`],
//! so an idle hosted session does not answer server pings — fine for the short,
//! request-driven observation loops this serves, not a long-lived event sink.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use async_tungstenite::WebSocketStream;
use async_tungstenite::tokio::ConnectStream;
use async_tungstenite::tungstenite::Message;
use chromiumoxide::Command;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::target::{
    AttachToTargetParams, CreateTargetParams, GetTargetsParams, SetDiscoverTargetsParams, TargetId,
    TargetInfo,
};
use futures::StreamExt as _;
use tokio::sync::Mutex;

/// Seals [`CdpChannel`] so it is a public trait (the public, generic
/// [`CdpObserver`](crate::observer::CdpObserver) is bound by it) that no
/// downstream crate can implement. The only two transports are the ones this
/// crate ships: a local [`Page`] and a hosted [`RawCdpSession`].
mod sealed {
    pub trait Sealed {}
    impl Sealed for chromiumoxide::Page {}
    impl Sealed for super::RawCdpSession {}
}

use crate::error::CdpError;
use crate::observer::{CdpObserver, ensure_ring_provider};

/// Run one typed CDP command and decode its typed response.
///
/// This is the single seam the observer talks through, so the observation
/// pipeline does not care whether it is driving a locally launched
/// [`chromiumoxide::Page`] or a hosted page over a [`RawCdpSession`]. The `.result`
/// envelope chromiumoxide wraps responses in is unwrapped here, so callers get
/// `T::Response` directly.
pub trait CdpChannel: sealed::Sealed + Send + Sync {
    /// Execute `cmd` and return its decoded response.
    fn run<T>(&self, cmd: T) -> impl Future<Output = Result<T::Response, CdpError>> + Send
    where
        T: Command + Send + 'static,
        T::Response: Send;

    /// Execute `cmd` tagged with an explicit child `session_id`, rather than the
    /// channel's own (page) session.
    ///
    /// A cross-origin out-of-process iframe (OOPIF) lives in a *different* CDP
    /// session, so observing or acting on it means tagging the request with that
    /// child `sessionId`. The default ignores the session id and runs on the
    /// channel's own session: it is what a local [`Page`] (which surfaces no
    /// separate OOPIF sessions through this seam) wants, and it is never reached
    /// with a real child session there because [`auto_attach_children`] returns
    /// none. [`RawCdpSession`] overrides it with the real session-tagged write.
    #[allow(clippy::manual_async_fn)]
    fn run_on<T>(
        &self,
        _session_id: Option<&str>,
        cmd: T,
    ) -> impl Future<Output = Result<T::Response, CdpError>> + Send
    where
        T: Command + Send + 'static,
        T::Response: Send,
    {
        async move { self.run(cmd).await }
    }

    /// Turn on flat auto-attach and return the sessions for the page's existing
    /// out-of-process child targets (cross-origin iframes, workers).
    ///
    /// The default returns none: a local [`Page`] drives its own targets and
    /// does not expose separate OOPIF sessions through this channel.
    /// [`RawCdpSession`] overrides it to actually drive
    /// `Target.setAutoAttach { flatten: true }` and collect the announced
    /// children (`DECISIONS.md` D22 step 2).
    #[allow(clippy::manual_async_fn)]
    fn auto_attach_children(
        &self,
    ) -> impl Future<Output = Result<Vec<ChildSession>, CdpError>> + Send {
        async move { Ok(Vec::new()) }
    }
}

/// The local path: a chromiumoxide page already drives its own handler, so a
/// command is just `execute`, with the `CommandResponse` envelope unwrapped to
/// match the trait.
impl CdpChannel for Page {
    // Spelled with an explicit `-> impl Future + Send` rather than `async fn`:
    // the `+ Send` bound is load-bearing (the generic `observe` over this trait
    // must itself stay `Send`), and an `async fn` in a trait does not carry it.
    #[allow(clippy::manual_async_fn)]
    fn run<T>(&self, cmd: T) -> impl Future<Output = Result<T::Response, CdpError>> + Send
    where
        T: Command + Send + 'static,
        T::Response: Send,
    {
        async move { Ok(self.execute(cmd).await?.result) }
    }
}

/// A flat CDP session over a raw WebSocket.
///
/// Built by [`connect_hosted`]. Holds the socket behind a mutex (commands are
/// issued sequentially by the observer, so contention is nil) and the page
/// `sessionId` captured at attach time. Every [`run`](CdpChannel::run) tags its
/// request with that session so the command lands on the page, not the browser.
pub struct RawCdpSession {
    ws: Mutex<WebSocketStream<ConnectStream>>,
    /// The flat page session every command is tagged with. `None` only during
    /// the browser-level handshake in [`connect_hosted`] (discover → getTargets
    /// → attach), before the page session exists.
    session_id: Option<String>,
    next_id: AtomicU64,
}

impl RawCdpSession {
    fn new(ws: WebSocketStream<ConnectStream>) -> Self {
        Self {
            ws: Mutex::new(ws),
            session_id: None,
            // Start above zero so a stray `{"id":0}` never collides with a live
            // request id.
            next_id: AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

impl CdpChannel for RawCdpSession {
    // See the `Page` impl: explicit `+ Send` is required and `async fn` cannot
    // express it here.
    #[allow(clippy::manual_async_fn)]
    fn run<T>(&self, cmd: T) -> impl Future<Output = Result<T::Response, CdpError>> + Send
    where
        T: Command + Send + 'static,
        T::Response: Send,
    {
        // Default page path: tag every request with the held page session.
        // The OOPIF path reaches the same write loop through `run_on` with a
        // child session instead (D22 step 1).
        async move { self.run_on(self.session_id.as_deref(), cmd).await }
    }

    /// Run one typed command tagged with an explicit session, rather than the
    /// default page session this struct holds.
    ///
    /// [`run`](CdpChannel::run) always tags requests with `self.session_id` (the
    /// page). A cross-origin out-of-process iframe lives in a *different* CDP
    /// session, so observing or acting on it means tagging the request with that
    /// child `sessionId` instead. This is the single write-path generalization
    /// the multi-session OOPIF path turns on (`DECISIONS.md` D22 step 1): the
    /// read path is untouched because `next_id` is one shared monotonic counter
    /// and [`response_for`] demuxes purely by request id, regardless of which
    /// session the response came back on. Passing `self.session_id.as_deref()`
    /// reproduces the default page path byte-for-byte.
    #[allow(clippy::manual_async_fn)]
    fn run_on<T>(
        &self,
        session_id: Option<&str>,
        cmd: T,
    ) -> impl Future<Output = Result<T::Response, CdpError>> + Send
    where
        T: Command + Send + 'static,
        T::Response: Send,
    {
        async move {
            let id = self.next_id();
            let params = serde_json::to_value(&cmd)
                .map_err(|e| CdpError::Malformed(format!("serialize {}: {e}", cmd.identifier())))?;
            let envelope = build_envelope(id, cmd.identifier().as_ref(), params, session_id);

            let mut ws = self.ws.lock().await;
            ws.send(Message::text(envelope.to_string()))
                .await
                .map_err(ws_error)?;

            // Read until our id comes back, discarding CDP events and any
            // message addressed to a different request along the way.
            loop {
                let Some(frame) = ws.next().await else {
                    return Err(CdpError::Malformed(
                        "cdp websocket closed before a response arrived".into(),
                    ));
                };
                let text = match frame.map_err(ws_error)? {
                    Message::Text(t) => t,
                    Message::Close(_) => {
                        return Err(CdpError::Malformed(
                            "cdp websocket closed before a response arrived".into(),
                        ));
                    }
                    // Ping/Pong/Binary carry no CDP payload; keep reading.
                    _ => continue,
                };
                let value: serde_json::Value = serde_json::from_str(text.as_str())
                    .map_err(|e| CdpError::Malformed(format!("non-JSON cdp frame: {e}")))?;

                match response_for(&value, id) {
                    ResponseFor::Result(result) => {
                        return serde_json::from_value::<T::Response>(result).map_err(|e| {
                            CdpError::Malformed(format!(
                                "decode {} response: {e}",
                                cmd.identifier()
                            ))
                        });
                    }
                    ResponseFor::Error(msg) => {
                        return Err(CdpError::Malformed(format!(
                            "{} failed: {msg}",
                            cmd.identifier()
                        )));
                    }
                    // An event or a response to some other request — skip it.
                    ResponseFor::Other => continue,
                }
            }
        }
    }

    /// Turn on flat auto-attach for child targets and collect the sessions
    /// Chrome announces for the ones that already exist.
    ///
    /// A cross-origin out-of-process iframe is unreachable from the page
    /// session: it lives in its own CDP target with its own backend-node id
    /// space, and `getDocument { pierce: true }` stops at its boundary (see
    /// [`frames`](crate::frames)). The way in is
    /// `Target.setAutoAttach { flatten: true }`, which makes Chrome announce
    /// each existing child target with a `Target.attachedToTarget` *event*
    /// carrying a fresh `sessionId`. Unlike every other command this channel
    /// runs, the payload we want rides those events, not the command response —
    /// so this drains the socket, gathering each child via
    /// [`parse_attached_to_target`], until the `setAutoAttach` ack for our
    /// request id arrives. Chrome emits the events for already-attached children
    /// before that ack, so one drain captures the current child set
    /// (`DECISIONS.md` D22 step 2).
    ///
    /// Runs against the page session this struct holds. Each returned
    /// [`ChildSession`] is later joined to its durable
    /// [`FrameKey`](anchortree_core::FrameKey) by
    /// [`child_frame_keys`](crate::frames::child_frame_keys) and observed with
    /// [`run_on`](CdpChannel::run_on) tagged to that child session (D22 steps
    /// 3–4).
    #[allow(clippy::manual_async_fn)]
    fn auto_attach_children(
        &self,
    ) -> impl Future<Output = Result<Vec<ChildSession>, CdpError>> + Send {
        use chromiumoxide::cdp::browser_protocol::target::SetAutoAttachParams;

        async move {
            let cmd = SetAutoAttachParams::builder()
                .auto_attach(true)
                .flatten(true)
                .wait_for_debugger_on_start(false)
                .build()
                .map_err(CdpError::Malformed)?;

            let method = SetAutoAttachParams::IDENTIFIER;
            let id = self.next_id();
            let params = serde_json::to_value(&cmd)
                .map_err(|e| CdpError::Malformed(format!("serialize {method}: {e}")))?;
            let envelope = build_envelope(id, method, params, self.session_id.as_deref());

            let mut ws = self.ws.lock().await;
            ws.send(Message::text(envelope.to_string()))
                .await
                .map_err(ws_error)?;

            let mut children = Vec::new();
            loop {
                let Some(frame) = ws.next().await else {
                    return Err(CdpError::Malformed(
                        "cdp websocket closed before setAutoAttach acked".into(),
                    ));
                };
                let text = match frame.map_err(ws_error)? {
                    Message::Text(t) => t,
                    Message::Close(_) => {
                        return Err(CdpError::Malformed(
                            "cdp websocket closed before setAutoAttach acked".into(),
                        ));
                    }
                    _ => continue,
                };
                let value: serde_json::Value = serde_json::from_str(text.as_str())
                    .map_err(|e| CdpError::Malformed(format!("non-JSON cdp frame: {e}")))?;

                if let Some(child) = parse_attached_to_target(&value) {
                    children.push(child);
                    continue;
                }
                match response_for(&value, id) {
                    ResponseFor::Result(_) => return Ok(children),
                    ResponseFor::Error(msg) => {
                        return Err(CdpError::Malformed(format!("{method} failed: {msg}")));
                    }
                    ResponseFor::Other => continue,
                }
            }
        }
    }
}

/// Map a WebSocket transport failure into our error type. There is no `From`
/// impl because `tungstenite::Error` is not part of this crate's public surface.
fn ws_error(e: async_tungstenite::tungstenite::Error) -> CdpError {
    CdpError::Malformed(format!("cdp websocket transport error: {e}"))
}

/// Build the flat-protocol request envelope: `id` + `method` + `params`, plus a
/// `sessionId` when one is held. Kept a free function so the exact wire shape is
/// unit-testable without a socket.
fn build_envelope(
    id: u64,
    method: &str,
    params: serde_json::Value,
    session_id: Option<&str>,
) -> serde_json::Value {
    let mut envelope = serde_json::Map::new();
    envelope.insert("id".into(), serde_json::Value::from(id));
    envelope.insert("method".into(), serde_json::Value::from(method));
    envelope.insert("params".into(), params);
    if let Some(session) = session_id {
        envelope.insert("sessionId".into(), serde_json::Value::from(session));
    }
    serde_json::Value::Object(envelope)
}

/// Classification of one inbound CDP frame against the request `id` we are
/// waiting on.
enum ResponseFor {
    /// Our response, carrying its `result` payload (absent `result` reads as
    /// `null`, which decodes fine for the unit-returning commands).
    Result(serde_json::Value),
    /// Our response, but an error.
    Error(String),
    /// An event, or a response to a different request.
    Other,
}

/// Decide whether `value` is the response to request `id`, and if so whether it
/// succeeded. A CDP response has a numeric `id`; an event has none. Free
/// function so the matching rule is testable.
fn response_for(value: &serde_json::Value, id: u64) -> ResponseFor {
    if value.get("id").and_then(serde_json::Value::as_u64) != Some(id) {
        return ResponseFor::Other;
    }
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown cdp error");
        return ResponseFor::Error(message.to_owned());
    }
    ResponseFor::Result(
        value
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    )
}

/// A child CDP session announced by `Target.attachedToTarget` under flat
/// auto-attach: its session id, the target it drives, and that target's type.
///
/// For a cross-origin out-of-process iframe the `target_id` is the join key to a
/// durable [`FrameKey`](anchortree_core::FrameKey): it equals the child's own
/// page `frameId`, which the root pierced DOM already keyed off its owner
/// `<iframe>` element (`DECISIONS.md` D22 step 3, amended). So
/// [`child_frame_keys`](crate::frames::child_frame_keys) resolves it against a
/// [`dom_frame_keys`](crate::frames::dom_frame_keys) table without a fresh
/// frame-id round-trip. `target_type` is kept so a caller can tell an `iframe`
/// child (observe it) from a `worker` or `service_worker` child (skip it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildSession {
    /// The fresh `sessionId` to tag this child's commands with via
    /// [`run_on`](RawCdpSession::run_on).
    pub session_id: String,
    /// The child target's id. For an OOPIF this is its page frame id.
    pub target_id: String,
    /// The child target's `type` (e.g. `"iframe"`, `"worker"`).
    pub target_type: String,
}

/// Parse one `Target.attachedToTarget` event into a [`ChildSession`].
///
/// Returns `None` for any other frame — a different event, a response, or a
/// payload missing a field. Free function so the wire-shape parse is unit-
/// testable without a socket, mirroring [`response_for`] and
/// [`select_page_target`].
fn parse_attached_to_target(value: &serde_json::Value) -> Option<ChildSession> {
    if value.get("method").and_then(serde_json::Value::as_str) != Some("Target.attachedToTarget") {
        return None;
    }
    let params = value.get("params")?;
    let session_id = params.get("sessionId")?.as_str()?.to_owned();
    let target_info = params.get("targetInfo")?;
    let target_id = target_info.get("targetId")?.as_str()?.to_owned();
    let target_type = target_info.get("type")?.as_str()?.to_owned();
    Some(ChildSession {
        session_id,
        target_id,
        target_type,
    })
}

/// Choose the page target to attach to from a `Target.getTargets` reply.
///
/// Prefer a `page` target (the document a real agent observes); fall back to
/// nothing so the caller can create one. `tab` and `iframe` subtypes and
/// service workers are skipped. Free function so the selection rule is testable
/// without a browser.
fn select_page_target(infos: &[TargetInfo]) -> Option<TargetId> {
    infos
        .iter()
        .find(|t| t.r#type == "page")
        .map(|t| t.target_id.clone())
}

/// A connected hosted browser plus an observer over its existing page.
///
/// Unlike [`Session`](crate::Session), there is no background handler task: the
/// [`RawCdpSession`] drives the socket itself, inside each command. Dropping this
/// closes the WebSocket.
pub struct HostedSession {
    /// The observation source. Call [`observe`](anchortree_core::ObservationSource::observe)
    /// on it exactly as you would the local [`Session`](crate::Session)'s.
    pub observer: CdpObserver<RawCdpSession>,
}

impl HostedSession {
    /// Point the attached page at `url` and wait for the load to commit.
    ///
    /// A hosted browser hands you a page that is open but not necessarily where
    /// you want it; this is the hosted equivalent of opening a tab. It issues
    /// `Page.navigate` over the flat session.
    pub async fn navigate(&self, url: &str) -> Result<(), CdpError> {
        use chromiumoxide::cdp::browser_protocol::page::NavigateParams;
        self.observer
            .channel()
            .run(NavigateParams::new(url.to_owned()))
            .await
            .map(|_| ())
    }

    /// Evaluate `expression` in the page's main world, e.g. to force a
    /// re-render between two observations. Issues `Runtime.evaluate` over the
    /// flat session and discards the result value.
    pub async fn evaluate(&self, expression: &str) -> Result<(), CdpError> {
        use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
        self.observer
            .channel()
            .run(EvaluateParams::new(expression.to_owned()))
            .await
            .map(|_| ())
    }

    /// Auto-attach to this page's out-of-process child targets and return the
    /// sessions Chrome announces for the ones that already exist.
    ///
    /// Thin pass-through to [`RawCdpSession::auto_attach_children`]; see it for
    /// the mechanism. Each [`ChildSession`] is a cross-origin iframe (or worker)
    /// living in its own CDP target, reachable only by tagging commands with its
    /// `session_id`. Join it to a durable
    /// [`FrameKey`](anchortree_core::FrameKey) with [`frame_keys`](Self::frame_keys)
    /// + [`child_frame_keys`](crate::frames::child_frame_keys).
    pub async fn auto_attach_children(&self) -> Result<Vec<ChildSession>, CdpError> {
        self.observer.channel().auto_attach_children().await
    }

    /// Assign every frame its durable structural
    /// [`FrameKey`](anchortree_core::FrameKey), keyed by frame id, by walking the
    /// pierced DOM in document order.
    ///
    /// It would be tempting to read `Page.getFrameTree` here, but a live OOPIF
    /// proof falsified that path (`DECISIONS.md` D22 step 3, amended): a
    /// cross-origin iframe's frame is *absent* from the root target's frame tree.
    /// Its owner `<iframe>` element, however, is present in the root pierced DOM
    /// carrying `frameId` == the child target's id, just with its
    /// `contentDocument` stripped. So we key off DOM document order via
    /// [`dom_frame_keys`](crate::frames::dom_frame_keys), which includes those
    /// OOPIF owners. This table is exactly what
    /// [`child_frame_keys`](crate::frames::child_frame_keys) joins an
    /// [`auto_attach_children`](Self::auto_attach_children) result against
    /// (`child.target_id` -> structural key) to learn which frame each child
    /// session belongs to.
    pub async fn frame_keys(
        &self,
    ) -> Result<std::collections::HashMap<String, anchortree_core::FrameKey>, CdpError> {
        use chromiumoxide::cdp::browser_protocol::dom::GetDocumentParams;
        let document = self
            .observer
            .channel()
            .run(GetDocumentParams::builder().depth(-1).pierce(true).build())
            .await?
            .root;
        let dom = crate::observer::decode_dom_node(&document);
        Ok(crate::frames::dom_frame_keys(&dom))
    }
}

/// Connect to a browser the way a *hosted gateway* exposes it: a `wss://` (or
/// `ws://`) endpoint to a browser that already has a page open, which we flat-
/// attach to rather than opening our own.
///
/// This is the connect leg in front of the [`gateway`](crate::gateway) acquire
/// leg: feed it the `connect_url` from
/// [`gateway::browserbase::acquire`](crate::gateway::browserbase::acquire) or
/// [`gateway::cloudflare::devtools_ws_url`](crate::gateway::cloudflare::devtools_ws_url).
/// The sequence is:
///
/// 1. open the WebSocket (TLS via rustls/ring for `wss://`, see
///    [`is_tls_endpoint`](crate::is_tls_endpoint));
/// 2. enable target discovery and list targets;
/// 3. attach to the existing `page` target with `flatten: true`, capturing the
///    `sessionId` (creating an `about:blank` page first only if the browser has
///    none);
/// 4. enable Accessibility + DOM on that session and hand back a ready
///    [`HostedSession`].
///
/// Must be called from within a Tokio runtime.
pub async fn connect_hosted(ws_url: impl AsRef<str>) -> Result<HostedSession, CdpError> {
    let ws_url = ws_url.as_ref();
    if crate::is_tls_endpoint(ws_url) {
        ensure_ring_provider();
    }

    let (ws, _response) = async_tungstenite::tokio::connect_async(ws_url)
        .await
        .map_err(ws_error)?;
    let mut channel = RawCdpSession::new(ws);

    // Browser-level handshake (no session tag yet): discover and list targets,
    // then flat-attach to the page the browser already has open.
    channel.run(SetDiscoverTargetsParams::new(true)).await?;
    let targets = channel.run(GetTargetsParams::builder().build()).await?;

    let target_id = match select_page_target(&targets.target_infos) {
        Some(id) => id,
        // A browser context with no page yet — open one so there is something
        // to observe. (The common hosted case already has a page; this is the
        // empty-context fallback.)
        None => {
            channel
                .run(CreateTargetParams::new("about:blank"))
                .await?
                .target_id
        }
    };

    let attached = channel
        .run(
            AttachToTargetParams::builder()
                .target_id(target_id)
                .flatten(true)
                .build()
                .map_err(CdpError::Malformed)?,
        )
        .await?;
    channel.session_id = Some(attached.session_id.into());

    // From here every command is tagged with the page session.
    let observer = CdpObserver::attach(channel).await?;
    Ok(HostedSession { observer })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_carries_session_when_held() {
        let env = build_envelope(
            7,
            "Accessibility.getFullAXTree",
            serde_json::json!({}),
            Some("S-123"),
        );
        assert_eq!(env["id"], 7);
        assert_eq!(env["method"], "Accessibility.getFullAXTree");
        assert_eq!(env["sessionId"], "S-123");
        assert!(env.get("params").is_some());
    }

    #[test]
    fn envelope_omits_session_during_browser_handshake() {
        let env = build_envelope(1, "Target.getTargets", serde_json::json!({}), None);
        assert_eq!(env["id"], 1);
        // A browser-level command must NOT carry a sessionId key at all, or
        // Chrome routes it to a (nonexistent) session.
        assert!(env.get("sessionId").is_none());
    }

    #[test]
    fn response_for_matches_our_id_and_extracts_result() {
        let reply = serde_json::json!({"id": 4, "result": {"sessionId": "abc"}});
        match response_for(&reply, 4) {
            ResponseFor::Result(r) => assert_eq!(r["sessionId"], "abc"),
            _ => panic!("expected a result for our id"),
        }
    }

    #[test]
    fn response_for_surfaces_error_message() {
        let reply = serde_json::json!({"id": 9, "error": {"code": -32001, "message": "Session with given id not found."}});
        match response_for(&reply, 9) {
            ResponseFor::Error(m) => assert!(m.contains("Session with given id not found")),
            _ => panic!("expected an error for our id"),
        }
    }

    #[test]
    fn response_for_skips_events_and_foreign_ids() {
        // A CDP event has no id.
        let event = serde_json::json!({"method": "Target.attachedToTarget", "params": {}});
        assert!(matches!(response_for(&event, 1), ResponseFor::Other));
        // A response to a different request.
        let other = serde_json::json!({"id": 2, "result": {}});
        assert!(matches!(response_for(&other, 1), ResponseFor::Other));
    }

    #[test]
    fn response_for_treats_missing_result_as_null() {
        // Commands that return nothing (e.g. enables) come back with no
        // `result`; that must decode as JSON null, not an error.
        let reply = serde_json::json!({"id": 3});
        match response_for(&reply, 3) {
            ResponseFor::Result(r) => assert!(r.is_null()),
            _ => panic!("a result-less response is still a success"),
        }
    }

    fn target(kind: &str, id: &str) -> TargetInfo {
        TargetInfo {
            target_id: TargetId::new(id),
            r#type: kind.to_owned(),
            title: String::new(),
            url: String::new(),
            attached: false,
            opener_id: None,
            can_access_opener: false,
            opener_frame_id: None,
            parent_frame_id: None,
            browser_context_id: None,
            subtype: None,
        }
    }

    #[test]
    fn select_page_target_prefers_a_page_over_other_types() {
        let infos = vec![
            target("service_worker", "sw-1"),
            target("page", "page-1"),
            target("browser", "b-1"),
        ];
        assert_eq!(select_page_target(&infos), Some(TargetId::new("page-1")));
    }

    #[test]
    fn select_page_target_returns_none_when_no_page_exists() {
        let infos = vec![target("service_worker", "sw-1"), target("browser", "b-1")];
        assert_eq!(select_page_target(&infos), None);
    }

    #[test]
    fn parse_attached_to_target_extracts_child_session() {
        // The shape Chrome emits for each child under flat auto-attach. For an
        // OOPIF the targetId equals the page frameId.
        let event = serde_json::json!({
            "method": "Target.attachedToTarget",
            "params": {
                "sessionId": "CHILD-SESS-1",
                "targetInfo": {
                    "targetId": "FRAME-OOPIF-A",
                    "type": "iframe",
                    "title": "",
                    "url": "https://other.example/",
                    "attached": true,
                    "canAccessOpener": false
                },
                "waitingForDebugger": false
            }
        });
        assert_eq!(
            parse_attached_to_target(&event),
            Some(ChildSession {
                session_id: "CHILD-SESS-1".into(),
                target_id: "FRAME-OOPIF-A".into(),
                target_type: "iframe".into(),
            })
        );
    }

    #[test]
    fn parse_attached_to_target_ignores_other_events_and_responses() {
        // A different event.
        let other_event = serde_json::json!({
            "method": "Target.targetInfoChanged",
            "params": {"targetInfo": {"targetId": "x", "type": "iframe"}}
        });
        assert_eq!(parse_attached_to_target(&other_event), None);
        // A command response (no method).
        let response = serde_json::json!({"id": 5, "result": {}});
        assert_eq!(parse_attached_to_target(&response), None);
    }

    #[test]
    fn parse_attached_to_target_rejects_a_payload_missing_a_field() {
        // attachedToTarget but with no sessionId — a malformed frame must not
        // mint a half-formed child.
        let no_session = serde_json::json!({
            "method": "Target.attachedToTarget",
            "params": {
                "targetInfo": {"targetId": "FRAME-A", "type": "iframe"}
            }
        });
        assert_eq!(parse_attached_to_target(&no_session), None);
    }
}

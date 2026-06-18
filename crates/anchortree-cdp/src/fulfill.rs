//! The fulfill leg: turn a matcher verdict into a ready-to-dispatch CDP action.
//!
//! [`crate::replay`] is the transport-neutral matcher — it answers "does this
//! request have a recorded response, and where does its body live?" without ever
//! naming a CDP type. This module is the other half: it takes a
//! [`MatchOutcome`](crate::replay::MatchOutcome) and produces the exact
//! `Fetch.fulfillRequest` / `Fetch.failRequest` parameters the live event loop
//! dispatches when a `Fetch.requestPaused` fires.
//!
//! The split keeps the matcher in the fusion path (CI-tested, browser-free) and
//! confines every CDP type to this adapter file. The param-building done here is
//! pure and deterministic, so it is fully unit-tested in CI. The live
//! [`ReplayFulfiller`] event loop (subscribe `Fetch.requestPaused` → decode →
//! [`replay_action`] → dispatch) also lives here, since it names CDP types; it
//! compiles and is clippy-checked in CI, and its end-to-end proof against a real
//! browser rides an example (`examples/webarena_replay.rs`).
//!
//! ## Body encoding (DECISIONS D35)
//!
//! CDP `Fetch.fulfillRequest`'s `body` is base64 on the wire — `Binary` is a
//! transparent newtype that serializes its inner string verbatim, so the
//! fulfiller must hand it an already-base64 string. The recorder stores text
//! bodies raw (`content.encoding` absent) so a captured HAR stays human-readable
//! for debugging, and binary bodies base64 (`content.encoding == "base64"`).
//! This module closes that asymmetry: a raw [`ReplayBody::Inline`] body is
//! base64-encoded here; an already-base64 one passes straight through. The
//! encode runs once per intercepted request (not a hot path), so the cost buys a
//! readable on-disk artifact.

use std::fmt;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::fetch::{
    DisableParams, EnableParams, EventRequestPaused, FailRequestParams, FulfillRequestParams,
    HeaderEntry, RequestId, RequestPattern, RequestStage,
};
use chromiumoxide::cdp::browser_protocol::network::ErrorReason;
use chromiumoxide::types::Binary;
use futures::stream::{self, BoxStream};
use futures::{FutureExt as _, StreamExt as _};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::error::CdpError;
use crate::replay::{MatchOutcome, ReplayBody, ReplayEntry, ReplayHar, ReplayRequest};

/// A matcher verdict resolved into the CDP action to dispatch for a paused
/// request.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplayAction {
    /// Serve the recorded response via `Fetch.fulfillRequest`.
    Fulfill(FulfillRequestParams),
    /// Refuse the request via `Fetch.failRequest`. Produced for an unmatched
    /// request (an honest abort rather than a guessed response) and for an entry
    /// whose body lives in an external sidecar file the matcher will not open.
    Fail(FailRequestParams),
}

/// Map a matcher [`MatchOutcome`] to the CDP action for the paused request
/// identified by `request_id`.
///
/// - [`MatchOutcome::Fulfill`] becomes a [`ReplayAction::Fulfill`] carrying the
///   recorded status, headers, and body.
/// - [`MatchOutcome::Abort`] becomes a [`ReplayAction::Fail`] with
///   [`ErrorReason::Failed`] — no match means no response, never a fabricated
///   one.
///
/// An entry whose body is [`ReplayBody::External`] also becomes a `Fail`: the
/// matcher does not open sidecar files, so its bytes are unavailable here. A HAR
/// captured by anchortree's own recorder always inlines its bodies, so this only
/// arises for foreign HARs (e.g. the ServiceNow demo capture).
pub fn replay_action(request_id: impl Into<RequestId>, outcome: &MatchOutcome) -> ReplayAction {
    let request_id = request_id.into();
    match outcome {
        MatchOutcome::Fulfill(entry) => fulfill_action(request_id, entry),
        MatchOutcome::Abort => {
            ReplayAction::Fail(FailRequestParams::new(request_id, ErrorReason::Failed))
        }
    }
}

fn fulfill_action(request_id: RequestId, entry: &ReplayEntry) -> ReplayAction {
    let body = match entry.body() {
        ReplayBody::Empty => None,
        // Already base64: pass the stored string straight to `Binary` (no
        // re-encode — `Binary` serializes verbatim).
        ReplayBody::Inline { text, base64: true } => Some(Binary::from(text.to_string())),
        // Raw text: encode to the base64 the wire expects.
        ReplayBody::Inline {
            text,
            base64: false,
        } => Some(Binary::from(BASE64.encode(text.as_bytes()))),
        // The matcher never opens sidecar files, so the bytes are unavailable.
        // Fail honestly rather than serve an empty body as if it were the page.
        ReplayBody::External(_) => {
            return ReplayAction::Fail(FailRequestParams::new(request_id, ErrorReason::Failed));
        }
    };

    let headers: Vec<HeaderEntry> = entry
        .response_headers()
        .map(|(name, value)| HeaderEntry::new(name, value))
        .collect();

    let mut params = FulfillRequestParams::new(request_id, entry.status());
    if !headers.is_empty() {
        params.response_headers = Some(headers);
    }
    params.body = body;
    ReplayAction::Fulfill(params)
}

// ---------------------------------------------------------------------------
// The live fulfiller (Phase 3.5b, DECISIONS D36)
// ---------------------------------------------------------------------------
//
// The param builder above is pure and CI-tested. This half is the
// transport-touching event loop that drives it against a real browser: it
// subscribes to `Fetch.requestPaused`, decodes each paused request into the
// transport-neutral [`ReplayRequest`] the matcher keys on, runs it through
// [`replay_action`], and dispatches the resulting `Fetch.fulfillRequest` /
// `Fetch.failRequest` back over the same `Page`.
//
// ## Why mirror `NetworkCapture`, not a raw-WS pump (a D36 correction)
//
// D36 proposed building the pump on a raw-WS `TcpStream` frame reader, citing
// `examples/webarena_capture.rs`. That citation is imprecise: those lines are
// the one-shot HTTP `/json/version` lookup, not a WS event pump. The real
// non-discarding event tap already in the tree is chromiumoxide's
// `Page::event_listener::<T>()` `EventStream`, which
// [`NetworkCapture`](crate::runner::NetworkCapture) uses for live HAR capture.
// The hosted channel's `run_on` read loop *does* discard events (DECISIONS D26),
// so `requestPaused` must ride the `event_listener` path — exactly the shape
// this fulfiller borrows. D36's constraint (sequence the event-sink, never let a
// paused event drop) is honored; only its pump citation is corrected.
//
// ## Sequencing (D36)
//
// `Fetch.requestPaused` blocks its request until the client answers, so the
// fulfiller `start`s before navigation: subscribe, then `Fetch.enable` at the
// `Request` stage for every URL. The caller navigates; every paused request is
// answered (recognized -> fulfill, unrecognized/external -> fail, hermetic per
// D30) until load settles. `finish` stops the pump and `Fetch.disable`s, after
// which the caller runs the observe loop over the static replayed DOM. Observe
// and interception never overlap.

/// Decode a `Fetch.requestPaused` event into the transport-neutral
/// [`ReplayRequest`] the matcher keys on.
///
/// This is the one place a CDP `EventRequestPaused` becomes a plain value; the
/// matcher never sees a CDP type. Headers arrive as a JSON object
/// (`network::Headers`) and are flattened to `(name, value)` pairs, dropping any
/// non-string value (CDP only ever sends string header values).
///
/// `post_data` is left `None`: the proof target is a GET/RETRIEVE trajectory,
/// and `network::Request` carries a POST body only as base64 `post_data_entries`,
/// not a decoded string. A POST replay path would decode those here; it is out
/// of scope for the first M=1.
pub fn request_from_paused(paused: &EventRequestPaused) -> ReplayRequest {
    let headers = paused
        .request
        .headers
        .inner()
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(name, value)| value.as_str().map(|v| (name.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default();
    ReplayRequest {
        method: paused.request.method.clone(),
        url: paused.request.url.clone(),
        post_data: None,
        headers,
    }
}

/// Tally of how a [`ReplayFulfiller`] answered the paused requests it saw.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FulfillStats {
    /// Requests answered with a recorded response (`Fetch.fulfillRequest`).
    pub fulfilled: usize,
    /// Requests refused (`Fetch.failRequest`) — no match, or an external body the
    /// matcher will not open.
    pub failed: usize,
    /// Paused events whose dispatch call itself errored (e.g. the request was
    /// already torn down). Counted apart so `fulfilled + failed` stays the honest
    /// count of requests actually answered.
    pub errors: usize,
}

/// Either a paused event or the stop signal, unified so the pump reads one
/// `select`ed stream (mirrors `NetworkCapture`'s `Control`).
enum PausedControl {
    Event(Arc<EventRequestPaused>),
    Stop,
}

/// A live `Fetch` interception in flight against a local [`Page`], answering
/// every paused request from a [`ReplayHar`].
///
/// Created by [`start`](ReplayFulfiller::start), closed by
/// [`finish`](ReplayFulfiller::finish). Between the two, navigate the page; every
/// request is answered from the HAR on a background task. Mirrors
/// [`NetworkCapture`](crate::runner::NetworkCapture)'s start/pump/finish shape.
pub struct ReplayFulfiller {
    pump: JoinHandle<FulfillStats>,
    stop: oneshot::Sender<()>,
    page: Page,
}

impl fmt::Debug for ReplayFulfiller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReplayFulfiller").finish_non_exhaustive()
    }
}

impl ReplayFulfiller {
    /// Subscribe to `Fetch.requestPaused`, enable interception, and start
    /// answering paused requests from `har` on a background task.
    ///
    /// Subscribes BEFORE `Fetch.enable` so no early request can slip between the
    /// enable ack and the listener being installed. Interception opens at the
    /// `Request` stage for every URL (`urlPattern: "*"`), so the page never
    /// touches the network: every request pauses and is answered from the HAR.
    ///
    /// Must be called from within a Tokio runtime. The page's CDP handler must be
    /// driven for events to flow — a [`Session`](crate::Session) from
    /// [`connect`](crate::connect) already does this.
    pub async fn start(page: &Page, har: ReplayHar) -> Result<Self, CdpError> {
        let events: BoxStream<'static, Arc<EventRequestPaused>> =
            page.event_listener::<EventRequestPaused>().await?.boxed();

        page.execute(EnableParams {
            patterns: Some(vec![RequestPattern {
                url_pattern: Some("*".to_string()),
                request_stage: Some(RequestStage::Request),
                ..Default::default()
            }]),
            handle_auth_requests: None,
        })
        .await?;

        let (stop_tx, stop_rx) = oneshot::channel();
        let pump = tokio::spawn(fulfill_pump(page.clone(), har, events, stop_rx));
        Ok(Self {
            pump,
            stop: stop_tx,
            page: page.clone(),
        })
    }

    /// Stop answering, disable interception, and return the [`FulfillStats`].
    ///
    /// Signals the pump to stop, lets it drain any paused events already queued,
    /// then `Fetch.disable`s so the caller's observe loop runs over the static
    /// replayed DOM with no interception live.
    pub async fn finish(self) -> Result<FulfillStats, CdpError> {
        // If the pump already exited (its stream closed), the receiver is gone and
        // this send is a no-op; either way we then await the pump and disable.
        let _ = self.stop.send(());
        let stats = self
            .pump
            .await
            .map_err(|e| CdpError::Malformed(format!("replay fulfiller pump task failed: {e}")))?;
        self.page.execute(DisableParams::default()).await?;
        Ok(stats)
    }
}

/// Background task: answer paused requests from `har` until stopped, then drain.
async fn fulfill_pump(
    page: Page,
    har: ReplayHar,
    events: BoxStream<'static, Arc<EventRequestPaused>>,
    stop: oneshot::Receiver<()>,
) -> FulfillStats {
    let mut stats = FulfillStats::default();

    // Fold the stop signal into the same stream as the events so the loop reads
    // one source (mirrors `NetworkCapture::pump`). `stop` yields once, then the
    // events stream carries the loop.
    let stop_stream = stream::once(async move {
        let _ = stop.await;
    })
    .map(|()| PausedControl::Stop)
    .boxed();
    let mut combined = stream::select(events.map(PausedControl::Event), stop_stream);

    while let Some(control) = combined.next().await {
        match control {
            PausedControl::Event(paused) => answer_one(&page, &har, &paused, &mut stats).await,
            PausedControl::Stop => {
                // Drain whatever is already buffered without awaiting new arrivals
                // (a paused event that landed just before the stop still gets
                // answered), then finish.
                while let Some(Some(PausedControl::Event(paused))) = combined.next().now_or_never()
                {
                    answer_one(&page, &har, &paused, &mut stats).await;
                }
                break;
            }
        }
    }

    stats
}

/// Decode one paused request, match it against `har`, and dispatch the verdict.
async fn answer_one(
    page: &Page,
    har: &ReplayHar,
    paused: &EventRequestPaused,
    stats: &mut FulfillStats,
) {
    let req = request_from_paused(paused);
    let outcome = har.outcome(&req);
    let action = replay_action(paused.request_id.clone(), &outcome);
    // Both params types map to `Result<Verdict, _>`; the verdict records which
    // CDP command we dispatched so a dispatch error is counted apart.
    let dispatched = match action {
        ReplayAction::Fulfill(params) => page.execute(params).await.map(|_| Verdict::Fulfilled),
        ReplayAction::Fail(params) => page.execute(params).await.map(|_| Verdict::Failed),
    };
    match dispatched {
        Ok(Verdict::Fulfilled) => stats.fulfilled += 1,
        Ok(Verdict::Failed) => stats.failed += 1,
        Err(_) => stats.errors += 1,
    }
}

/// Which CDP command [`answer_one`] dispatched for a paused request.
enum Verdict {
    Fulfilled,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{ReplayHar, ReplayRequest};

    /// A one-entry HAR whose single GET response carries the given content
    /// fields, built through the public `from_json` matcher entry point.
    fn har_with_content(content: serde_json::Value) -> ReplayHar {
        let har = serde_json::json!({
            "log": {
                "entries": [{
                    "request": { "method": "GET", "url": "https://example.test/", "postData": null },
                    "response": {
                        "status": 200,
                        "content": content,
                        "headers": [
                            { "name": "content-type", "value": "text/html" },
                            { "name": "x-trace", "value": "abc" }
                        ]
                    }
                }]
            }
        });
        ReplayHar::from_json(&har.to_string()).expect("valid HAR")
    }

    fn fulfill_params(action: ReplayAction) -> FulfillRequestParams {
        match action {
            ReplayAction::Fulfill(p) => p,
            ReplayAction::Fail(_) => panic!("expected Fulfill, got Fail"),
        }
    }

    #[test]
    fn raw_text_body_is_base64_encoded_into_params_body() {
        let har = har_with_content(serde_json::json!({
            "mimeType": "text/html",
            "text": "<html>hi</html>"
        }));
        let outcome = har.outcome(&ReplayRequest::get("https://example.test/"));
        let params = fulfill_params(replay_action(RequestId::new("req-1"), &outcome));
        let body = params.body.expect("body present");
        let encoded: &str = body.as_ref();
        assert_eq!(encoded, BASE64.encode(b"<html>hi</html>"));
        // And it round-trips back to the original bytes.
        assert_eq!(BASE64.decode(encoded).unwrap(), b"<html>hi</html>");
    }

    #[test]
    fn already_base64_body_passes_through_unchanged() {
        let stored = BASE64.encode(b"\x00\x01\x02binary");
        let har = har_with_content(serde_json::json!({
            "mimeType": "application/octet-stream",
            "text": stored,
            "encoding": "base64"
        }));
        let outcome = har.outcome(&ReplayRequest::get("https://example.test/"));
        let params = fulfill_params(replay_action(RequestId::new("req-1"), &outcome));
        let body = params.body.expect("body present");
        let encoded: &str = body.as_ref();
        // Verbatim: no double-encode.
        assert_eq!(encoded, stored);
        assert_eq!(BASE64.decode(encoded).unwrap(), b"\x00\x01\x02binary");
    }

    #[test]
    fn empty_body_yields_no_params_body() {
        let har = har_with_content(serde_json::json!({ "mimeType": "text/html" }));
        let outcome = har.outcome(&ReplayRequest::get("https://example.test/"));
        let params = fulfill_params(replay_action(RequestId::new("req-1"), &outcome));
        assert!(params.body.is_none());
    }

    #[test]
    fn recorded_headers_map_one_to_one() {
        let har = har_with_content(serde_json::json!({
            "mimeType": "text/html",
            "text": "x"
        }));
        let outcome = har.outcome(&ReplayRequest::get("https://example.test/"));
        let params = fulfill_params(replay_action(RequestId::new("req-1"), &outcome));
        let headers = params.response_headers.expect("headers present");
        assert_eq!(
            headers,
            vec![
                HeaderEntry::new("content-type", "text/html"),
                HeaderEntry::new("x-trace", "abc"),
            ]
        );
    }

    #[test]
    fn response_code_is_the_recorded_status() {
        let har = har_with_content(serde_json::json!({ "mimeType": "text/html", "text": "x" }));
        let outcome = har.outcome(&ReplayRequest::get("https://example.test/"));
        let params = fulfill_params(replay_action(RequestId::new("req-1"), &outcome));
        assert_eq!(params.response_code, 200);
        assert_eq!(params.request_id, RequestId::new("req-1"));
    }

    #[test]
    fn unmatched_request_aborts_with_failed_reason() {
        let har = har_with_content(serde_json::json!({ "mimeType": "text/html", "text": "x" }));
        let outcome = har.outcome(&ReplayRequest::get("https://example.test/missing"));
        let action = replay_action(RequestId::new("req-9"), &outcome);
        match action {
            ReplayAction::Fail(p) => {
                assert_eq!(p.request_id, RequestId::new("req-9"));
                assert_eq!(p.error_reason, ErrorReason::Failed);
            }
            ReplayAction::Fulfill(_) => panic!("expected Fail for unmatched request"),
        }
    }

    #[test]
    fn external_body_fails_rather_than_serving_empty() {
        let har = har_with_content(serde_json::json!({
            "mimeType": "text/html",
            "_file": "resources/page.html"
        }));
        let outcome = har.outcome(&ReplayRequest::get("https://example.test/"));
        let action = replay_action(RequestId::new("req-1"), &outcome);
        match action {
            ReplayAction::Fail(p) => assert_eq!(p.error_reason, ErrorReason::Failed),
            ReplayAction::Fulfill(_) => panic!("external body must Fail, not serve empty"),
        }
    }

    // `EventRequestPaused` derives `Deserialize`, so the live decode is testable
    // in CI without a browser: build a synthetic paused event from JSON and check
    // it flattens to the transport-neutral `ReplayRequest` the matcher keys on.

    /// A `Fetch.requestPaused` event with the given method, url, and header
    /// object, carrying the minimal required CDP fields so it deserializes.
    fn paused_event(method: &str, url: &str, headers: serde_json::Value) -> EventRequestPaused {
        let json = serde_json::json!({
            "requestId": "req-paused-1",
            "request": {
                "url": url,
                "method": method,
                "headers": headers,
                "initialPriority": "VeryHigh",
                "referrerPolicy": "no-referrer"
            },
            "frameId": "FRAME-1",
            "resourceType": "Document"
        });
        serde_json::from_value(json).expect("valid Fetch.requestPaused")
    }

    #[test]
    fn paused_decodes_method_and_url() {
        let paused = paused_event("GET", "https://example.test/page", serde_json::json!({}));
        let req = request_from_paused(&paused);
        assert_eq!(req.method, "GET");
        assert_eq!(req.url, "https://example.test/page");
        assert!(req.post_data.is_none());
    }

    #[test]
    fn paused_flattens_string_headers_to_pairs() {
        let paused = paused_event(
            "GET",
            "https://example.test/",
            serde_json::json!({ "accept": "text/html", "x-trace": "abc" }),
        );
        let req = request_from_paused(&paused);
        // JSON object order is preserved by serde_json's `preserve_order`? Not
        // guaranteed — assert as a set the matcher would key on.
        assert!(
            req.headers
                .contains(&("accept".to_string(), "text/html".to_string()))
        );
        assert!(
            req.headers
                .contains(&("x-trace".to_string(), "abc".to_string()))
        );
        assert_eq!(req.headers.len(), 2);
    }

    #[test]
    fn paused_drops_non_string_header_values() {
        let paused = paused_event(
            "GET",
            "https://example.test/",
            serde_json::json!({ "accept": "text/html", "x-count": 7 }),
        );
        let req = request_from_paused(&paused);
        // The numeric value is not a CDP-shaped header; it is dropped, not coerced.
        assert_eq!(
            req.headers,
            vec![("accept".to_string(), "text/html".to_string())]
        );
    }

    #[test]
    fn decoded_paused_request_matches_recorded_entry() {
        // The decode feeds the matcher: a paused GET for a recorded URL fulfills.
        let har = har_with_content(serde_json::json!({ "mimeType": "text/html", "text": "x" }));
        let paused = paused_event("GET", "https://example.test/", serde_json::json!({}));
        let req = request_from_paused(&paused);
        let outcome = har.outcome(&req);
        let action = replay_action(paused.request_id.clone(), &outcome);
        match action {
            ReplayAction::Fulfill(p) => {
                assert_eq!(p.request_id, RequestId::new("req-paused-1"));
                assert_eq!(p.response_code, 200);
            }
            ReplayAction::Fail(_) => panic!("a paused request for a recorded URL must fulfill"),
        }
    }

    #[test]
    fn decoded_paused_request_for_unknown_url_fails() {
        let har = har_with_content(serde_json::json!({ "mimeType": "text/html", "text": "x" }));
        let paused = paused_event("GET", "https://example.test/missing", serde_json::json!({}));
        let req = request_from_paused(&paused);
        let outcome = har.outcome(&req);
        let action = replay_action(paused.request_id.clone(), &outcome);
        match action {
            ReplayAction::Fail(p) => assert_eq!(p.error_reason, ErrorReason::Failed),
            ReplayAction::Fulfill(_) => panic!("an unknown URL must fail, not fulfill"),
        }
    }

    #[test]
    fn fulfill_stats_default_is_all_zero() {
        let stats = FulfillStats::default();
        assert_eq!(stats.fulfilled, 0);
        assert_eq!(stats.failed, 0);
        assert_eq!(stats.errors, 0);
    }
}

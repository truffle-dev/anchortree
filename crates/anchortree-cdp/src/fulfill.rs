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
//! pure and deterministic, so it is fully unit-tested in CI; only the live
//! `Fetch.requestPaused` → dispatch wiring needs a browser and lives in an
//! example.
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

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chromiumoxide::cdp::browser_protocol::fetch::{
    FailRequestParams, FulfillRequestParams, HeaderEntry, RequestId,
};
use chromiumoxide::cdp::browser_protocol::network::ErrorReason;
use chromiumoxide::types::Binary;

use crate::replay::{MatchOutcome, ReplayBody, ReplayEntry};

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
}

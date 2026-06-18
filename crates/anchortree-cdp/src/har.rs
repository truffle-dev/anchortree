//! HAR 1.2 recorder for the benchmark harness (Phase 3.3a).
//!
//! The WebArena-Verified evaluator scores a task partly from a `network.har`
//! trace (`NetworkEventEvaluator`, no DOM selectors), so the harness must emit a
//! spec-shaped [HAR 1.2] log. This module is that recorder, and it is built the
//! same way the rest of this crate is: the part that matters — turning a stream
//! of typed CDP `Network.*` events into a HAR document — is a **pure state
//! machine** with no browser, no async, and no I/O, so every decision it makes
//! is unit-testable against synthetic events. The only live surface is
//! [`enable`], a one-liner that turns Network tracking on over a [`CdpChannel`];
//! the event subscription that feeds the recorder belongs with the task-runner
//! (Phase 3.3b), where there is a live page to record against.
//!
//! ## Why a state machine keyed by request id
//!
//! Chrome reports a request as four separate events spread over time, correlated
//! only by `requestId`:
//!
//! - [`EventRequestWillBeSent`] — the request leaves; carries the [`Request`] and
//!   both a wall-clock (`wallTime`) and a monotonic (`timestamp`) stamp.
//! - [`EventResponseReceived`] — headers are back; carries the [`Response`].
//! - [`EventLoadingFinished`] — the body finished; carries the encoded byte count.
//! - [`EventLoadingFailed`] — the request errored; carries the error text.
//!
//! So one HAR entry is assembled across up to three events. A redirect reuses the
//! same `requestId`: the next `requestWillBeSent` arrives carrying a
//! `redirectResponse`, which is the signal to close the previous hop as its own
//! entry and open a fresh one. The recorder holds in-flight requests in a
//! `pending` map keyed by id and moves each to `entries` the moment it finalizes,
//! so completion order is preserved and the live feeder can stay a thin pass-through.
//!
//! [HAR 1.2]: http://www.softwareishard.com/blog/har-12-spec/

use std::collections::HashMap;

use serde::Serialize;

use chromiumoxide::cdp::browser_protocol::network::{
    EnableParams, EventLoadingFailed, EventLoadingFinished, EventRequestWillBeSent,
    EventRequestWillBeSentExtraInfo, EventResponseReceived, Request as CdpRequest,
    Response as CdpResponse,
};

use crate::channel::CdpChannel;
use crate::error::CdpError;

/// Turn on Network tracking so `Network.*` events start arriving.
///
/// This is the only live call the recorder needs; once it returns, the channel's
/// event stream carries the events [`HarRecorder`] consumes. Routed through
/// [`CdpChannel::run_on`] so it works on the page session (`None`) or, for an
/// OOPIF, the owning child session.
pub async fn enable<C: CdpChannel>(chan: &C, session: Option<&str>) -> Result<(), CdpError> {
    chan.run_on(session, EnableParams::default()).await?;
    Ok(())
}

/// Accumulates typed CDP `Network.*` events into a [HAR 1.2] document.
///
/// Pure and synchronous: feed it the events with [`on_request_will_be_sent`],
/// [`on_request_will_be_sent_extra_info`], [`on_response_received`],
/// [`on_loading_finished`], and [`on_loading_failed`], then call [`into_har`] for
/// the finished log. A live feeder may also feed a captured response body via
/// [`on_response_body`] between the response and the loading-finished events;
/// without it the recorder still emits a valid, body-less HAR exactly as before.
/// It owns no browser handle, so a live feeder is a thin task that forwards
/// decoded events and the unit tests drive it with hand-built events.
///
/// [HAR 1.2]: http://www.softwareishard.com/blog/har-12-spec/
/// [`on_request_will_be_sent`]: HarRecorder::on_request_will_be_sent
/// [`on_request_will_be_sent_extra_info`]: HarRecorder::on_request_will_be_sent_extra_info
/// [`on_response_received`]: HarRecorder::on_response_received
/// [`on_response_body`]: HarRecorder::on_response_body
/// [`on_loading_finished`]: HarRecorder::on_loading_finished
/// [`on_loading_failed`]: HarRecorder::on_loading_failed
/// [`into_har`]: HarRecorder::into_har
#[derive(Debug, Clone)]
pub struct HarRecorder {
    creator_name: String,
    creator_version: String,
    entries: Vec<HarEntry>,
    pending: HashMap<String, Pending>,
    /// On-wire request headers from a `requestWillBeSentExtraInfo` that arrived
    /// before its matching `requestWillBeSent`, stashed by request id until the
    /// pending entry exists to receive them (see
    /// [`on_request_will_be_sent_extra_info`](HarRecorder::on_request_will_be_sent_extra_info)).
    extra_request_headers: HashMap<String, Vec<HarHeader>>,
}

/// A request that has started but not yet finalized into an entry.
#[derive(Debug, Clone)]
struct Pending {
    request: HarRequest,
    started_date_time: String,
    started_monotonic: f64,
    response: Option<HarResponse>,
    server_ip_address: Option<String>,
    body: Option<ResponseBody>,
}

/// A captured response body, the input half of [`HarRecorder::on_response_body`].
///
/// This is the transport-neutral shape a live feeder produces from a
/// `Network.getResponseBody` reply: `text` is the body and `base64` says whether
/// it is base64-encoded binary (Chrome sets that flag for non-text MIME types).
/// Keeping the recorder's input a plain value — rather than a CDP type — is what
/// lets the body-capture path stay a pure, unit-testable state transition while
/// the actual CDP call lives in the live feeder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseBody {
    /// The response body, base64-encoded when `base64` is set.
    pub text: String,
    /// `true` when `text` holds base64-encoded binary rather than UTF-8 text.
    pub base64: bool,
}

impl Default for HarRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl HarRecorder {
    /// A fresh recorder stamped with this crate as the HAR `creator`.
    pub fn new() -> Self {
        Self::with_creator("anchortree", env!("CARGO_PKG_VERSION"))
    }

    /// A fresh recorder with an explicit `creator` name and version.
    pub fn with_creator(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            creator_name: name.into(),
            creator_version: version.into(),
            entries: Vec::new(),
            pending: HashMap::new(),
            extra_request_headers: HashMap::new(),
        }
    }

    /// Record a `Network.requestWillBeSent`.
    ///
    /// If the event carries a `redirectResponse` for a request that is already
    /// in flight, that previous hop is closed as its own entry (its end time is
    /// the moment this redirected request begins) before the new hop opens — the
    /// HAR shape for a redirect chain is one entry per hop.
    pub fn on_request_will_be_sent(&mut self, ev: &EventRequestWillBeSent) {
        let id = ev.request_id.inner().clone();

        if let Some(redirect) = &ev.redirect_response
            && let Some(prev) = self.pending.remove(&id)
        {
            let prev = Pending {
                response: Some(har_response_from(redirect)),
                server_ip_address: redirect
                    .remote_ip_address
                    .clone()
                    .or(prev.server_ip_address),
                ..prev
            };
            let entry = finalize(prev, Some(*ev.timestamp.inner()), None, None);
            self.entries.push(entry);
        }

        self.pending.insert(
            id.clone(),
            Pending {
                request: har_request_from(&ev.request),
                started_date_time: iso8601_from_unix_secs(*ev.wall_time.inner()),
                started_monotonic: *ev.timestamp.inner(),
                response: None,
                server_ip_address: None,
                body: None,
            },
        );

        // If the wire headers landed first, apply them now that the entry exists.
        if let Some(extra) = self.extra_request_headers.remove(&id)
            && let Some(pending) = self.pending.get_mut(&id)
        {
            merge_extra_request_headers(&mut pending.request, &extra);
        }
    }

    /// Record a `Network.requestWillBeSentExtraInfo`.
    ///
    /// This event carries the request headers as they will actually be sent over
    /// the wire, which for a top-level navigation are far richer than the
    /// provisional set on `requestWillBeSent` (often just `User-Agent` and
    /// `Upgrade-Insecure-Requests`). The browser's network stack adds `Accept`,
    /// the `sec-fetch-*` triad, `Accept-Encoding`, cookies, and the rest here.
    /// CDP gives no ordering guarantee between the two events, so if the matching
    /// request is already pending its headers are upgraded in place; otherwise
    /// the wire headers are stashed by request id and applied the moment
    /// [`on_request_will_be_sent`](Self::on_request_will_be_sent) creates the
    /// pending entry.
    pub fn on_request_will_be_sent_extra_info(&mut self, ev: &EventRequestWillBeSentExtraInfo) {
        let id = ev.request_id.inner().clone();
        let headers = har_headers(ev.headers.inner());
        if let Some(pending) = self.pending.get_mut(&id) {
            merge_extra_request_headers(&mut pending.request, &headers);
        } else {
            self.extra_request_headers.insert(id, headers);
        }
    }

    /// Record a `Network.responseReceived` (response headers are back).
    pub fn on_response_received(&mut self, ev: &EventResponseReceived) {
        if let Some(pending) = self.pending.get_mut(ev.request_id.inner()) {
            pending.response = Some(har_response_from(&ev.response));
            pending.server_ip_address = ev.response.remote_ip_address.clone();
        }
    }

    /// Attach a captured response body to an in-flight request.
    ///
    /// A body comes from a `Network.getResponseBody` read, which the live feeder
    /// issues once the response has loaded but before it forwards the
    /// `loadingFinished` event — so the pending entry is still present here and
    /// gets its body before [`on_loading_finished`](Self::on_loading_finished)
    /// finalizes it. A call for an unknown id (already finalized, or never seen)
    /// is a no-op, keeping the feeder a tolerant pass-through.
    pub fn on_response_body(&mut self, request_id: &str, body: ResponseBody) {
        if let Some(pending) = self.pending.get_mut(request_id) {
            pending.body = Some(body);
        }
    }

    /// Record a `Network.loadingFinished` and finalize the entry.
    pub fn on_loading_finished(&mut self, ev: &EventLoadingFinished) {
        if let Some(pending) = self.pending.remove(ev.request_id.inner()) {
            let body_size = ev.encoded_data_length as i64;
            let entry = finalize(pending, Some(*ev.timestamp.inner()), Some(body_size), None);
            self.entries.push(entry);
        }
    }

    /// Record a `Network.loadingFailed` and finalize the entry as an error.
    pub fn on_loading_failed(&mut self, ev: &EventLoadingFailed) {
        if let Some(pending) = self.pending.remove(ev.request_id.inner()) {
            let entry = finalize(
                pending,
                Some(*ev.timestamp.inner()),
                Some(0),
                Some(ev.error_text.clone()),
            );
            self.entries.push(entry);
        }
    }

    /// Number of finalized entries recorded so far (excludes still-in-flight).
    pub fn finalized_len(&self) -> usize {
        self.entries.len()
    }

    /// Consume the recorder and produce the HAR document.
    ///
    /// Any requests still in flight (started, never finished or failed) are
    /// flushed as entries with an unknown duration (`time = -1`), sorted by their
    /// start time so the output is deterministic regardless of map iteration order.
    pub fn into_har(self) -> Har {
        let HarRecorder {
            creator_name,
            creator_version,
            mut entries,
            pending,
            extra_request_headers: _,
        } = self;

        let mut leftover: Vec<Pending> = pending.into_values().collect();
        leftover.sort_by(|a, b| {
            a.started_monotonic
                .partial_cmp(&b.started_monotonic)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for p in leftover {
            entries.push(finalize(p, None, None, None));
        }

        Har {
            log: HarLog {
                version: "1.2",
                creator: HarCreator {
                    name: creator_name,
                    version: creator_version,
                },
                entries,
            },
        }
    }
}

/// Build a finished [`HarEntry`] from a pending request.
///
/// `end_monotonic` is the finish/fail/redirect timestamp; `None` means the
/// request never completed, so the total time is reported as `-1`. `body_size`
/// overrides the response body byte count when the loading-finished event
/// supplies the authoritative encoded length. `error` marks the entry as failed.
fn finalize(
    pending: Pending,
    end_monotonic: Option<f64>,
    body_size: Option<i64>,
    error: Option<String>,
) -> HarEntry {
    let time = match end_monotonic {
        Some(end) => ((end - pending.started_monotonic) * 1000.0).max(0.0),
        None => -1.0,
    };

    let mut response = pending.response.unwrap_or_else(HarResponse::placeholder);
    if let Some(bs) = body_size {
        response.body_size = bs;
        response.content.size = bs;
    }
    if let Some(body) = pending.body {
        response.content.encoding = body.base64.then(|| "base64".to_string());
        response.content.text = Some(body.text);
    }
    if let Some(err) = error {
        response.status = 0;
        response.error = Some(err);
    }

    HarEntry {
        started_date_time: pending.started_date_time,
        time,
        request: pending.request,
        response,
        cache: HarCache::default(),
        timings: HarTimings::with_total(time),
        server_ip_address: pending.server_ip_address,
    }
}

fn har_request_from(req: &CdpRequest) -> HarRequest {
    HarRequest {
        method: req.method.clone(),
        url: req.url.clone(),
        // The request-line HTTP version is not in the CDP Request; HAR requires
        // the field, so we emit a well-formed placeholder. The negotiated version
        // is reported on the response, where CDP does carry `protocol`.
        http_version: "HTTP/1.1".to_string(),
        cookies: Vec::new(),
        headers: har_headers(req.headers.inner()),
        query_string: query_string_from_url(&req.url),
        headers_size: -1,
        body_size: if req.has_post_data == Some(true) {
            -1
        } else {
            0
        },
    }
}

fn har_response_from(resp: &CdpResponse) -> HarResponse {
    let body_size = resp.encoded_data_length as i64;
    HarResponse {
        status: resp.status,
        status_text: resp.status_text.clone(),
        http_version: resp
            .protocol
            .clone()
            .map(normalize_http_version)
            .unwrap_or_else(|| "HTTP/1.1".to_string()),
        cookies: Vec::new(),
        headers: har_headers(resp.headers.inner()),
        content: HarContent {
            size: body_size,
            mime_type: resp.mime_type.clone(),
            text: None,
            encoding: None,
        },
        redirect_url: header_value(resp.headers.inner(), "location").unwrap_or_default(),
        headers_size: -1,
        body_size,
        error: None,
    }
}

/// Decode a CDP `Headers` JSON object into the HAR name/value list.
///
/// CDP carries headers as a JSON object (`{name: value}`); HAR wants an ordered
/// list of `{name, value}` pairs. Values are usually strings; anything else is
/// stringified so no header is silently dropped.
fn har_headers(value: &serde_json::Value) -> Vec<HarHeader> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    map.iter()
        .map(|(name, v)| HarHeader {
            name: name.clone(),
            value: v
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string()),
        })
        .collect()
}

/// Overlay the authoritative on-wire request headers from a
/// `requestWillBeSentExtraInfo` onto a [`HarRequest`].
///
/// A header present in `extra` replaces the provisional value of the same name
/// case-insensitively; a header not yet on the request is appended. HTTP/2
/// pseudo-headers (names beginning with `:`, e.g. `:authority`/`:method`) are
/// dropped — they are connection metadata, not valid HAR request headers.
fn merge_extra_request_headers(req: &mut HarRequest, extra: &[HarHeader]) {
    for h in extra {
        if h.name.starts_with(':') {
            continue;
        }
        match req
            .headers
            .iter_mut()
            .find(|e| e.name.eq_ignore_ascii_case(&h.name))
        {
            Some(existing) => existing.value = h.value.clone(),
            None => req.headers.push(h.clone()),
        }
    }
}

/// Case-insensitive lookup of a single header value (used for `Location`).
fn header_value(value: &serde_json::Value, name: &str) -> Option<String> {
    let map = value.as_object()?;
    map.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| {
            v.as_str()
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string())
        })
}

/// Split the query string out of a URL into HAR `{name, value}` pairs.
///
/// Values are kept exactly as they appear in the URL (HAR permits percent-encoded
/// query values), the fragment is dropped, and a bare `?key` with no `=` becomes a
/// pair with an empty value.
fn query_string_from_url(url: &str) -> Vec<HarQuery> {
    let Some((_, after)) = url.split_once('?') else {
        return Vec::new();
    };
    let query = after.split('#').next().unwrap_or(after);
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => HarQuery {
                name: name.to_string(),
                value: value.to_string(),
            },
            None => HarQuery {
                name: pair.to_string(),
                value: String::new(),
            },
        })
        .collect()
}

/// Map a CDP `protocol` string onto the HAR HTTP-version convention.
///
/// CDP reports `http/1.1`, `h2`, `h3`; HAR uses `HTTP/1.1`, `HTTP/2`, `HTTP/3`.
fn normalize_http_version(protocol: String) -> String {
    match protocol.as_str() {
        "h2" => "HTTP/2".to_string(),
        "h3" => "HTTP/3".to_string(),
        other => match other.strip_prefix("http/") {
            Some(rest) => format!("HTTP/{rest}"),
            None => other.to_uppercase(),
        },
    }
}

/// Format Unix epoch seconds (with a fractional part) as an ISO-8601 UTC instant
/// with millisecond precision, e.g. `2023-11-14T22:13:20.000Z`.
///
/// Dependency-free: the date is computed with Howard Hinnant's `civil_from_days`
/// so the crate does not pull a calendar library just to stamp HAR entries.
fn iso8601_from_unix_secs(secs: f64) -> String {
    let whole = secs.floor() as i64;
    let mut millis = ((secs - whole as f64) * 1000.0).round() as i64;
    let mut whole = whole;
    if millis >= 1000 {
        whole += 1;
        millis -= 1000;
    }

    let days = whole.div_euclid(86_400);
    let secs_of_day = whole.rem_euclid(86_400);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    let (year, month, day) = civil_from_days(days);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Convert a count of days since 1970-01-01 into a `(year, month, day)` civil
/// date. Howard Hinnant's algorithm, valid across the full range HAR needs.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = year + if month <= 2 { 1 } else { 0 };
    (year, month as u32, day)
}

// ---------------------------------------------------------------------------
// HAR 1.2 output types. Field names follow the spec; `serde` renames map the
// idiomatic Rust snake_case onto the spec's camelCase (and the two odd-cased
// `serverIPAddress` / `redirectURL`).
// ---------------------------------------------------------------------------

/// A complete HAR 1.2 document: `{ "log": { ... } }`.
#[derive(Debug, Clone, Serialize)]
pub struct Har {
    /// The single top-level `log` object.
    pub log: HarLog,
}

impl Har {
    /// Serialize to a pretty-printed `network.har` JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("HAR serializes")
    }
}

/// The HAR `log`: version, creator, and the recorded entries.
#[derive(Debug, Clone, Serialize)]
pub struct HarLog {
    /// HAR spec version, always `"1.2"`.
    pub version: &'static str,
    /// The tool that produced the log.
    pub creator: HarCreator,
    /// One entry per request, in finalization order.
    pub entries: Vec<HarEntry>,
}

/// The `creator` block naming the tool that wrote the log.
#[derive(Debug, Clone, Serialize)]
pub struct HarCreator {
    /// Tool name.
    pub name: String,
    /// Tool version.
    pub version: String,
}

/// One request/response exchange.
#[derive(Debug, Clone, Serialize)]
pub struct HarEntry {
    /// Request start time, ISO-8601 UTC with millisecond precision.
    #[serde(rename = "startedDateTime")]
    pub started_date_time: String,
    /// Total elapsed time in milliseconds, or `-1` if the request never finished.
    pub time: f64,
    /// The request.
    pub request: HarRequest,
    /// The response (a placeholder with status `0` if none was observed).
    pub response: HarResponse,
    /// Cache info; always empty (the recorder does not model the disk cache).
    pub cache: HarCache,
    /// Phase breakdown; the recorder reports the total under `wait`.
    pub timings: HarTimings,
    /// Server IP, when CDP reported one.
    #[serde(rename = "serverIPAddress", skip_serializing_if = "Option::is_none")]
    pub server_ip_address: Option<String>,
}

/// The request half of an entry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarRequest {
    /// HTTP method.
    pub method: String,
    /// Full request URL.
    pub url: String,
    /// HTTP version (placeholder `HTTP/1.1`; CDP omits the request-line version).
    pub http_version: String,
    /// Cookies; always empty (the recorder does not parse `Cookie` headers).
    pub cookies: Vec<HarCookie>,
    /// Request headers.
    pub headers: Vec<HarHeader>,
    /// Parsed query-string parameters.
    pub query_string: Vec<HarQuery>,
    /// Header bytes, or `-1` when unknown.
    pub headers_size: i64,
    /// Body bytes: `0` with no post data, `-1` when post data exists but its size
    /// is unknown.
    pub body_size: i64,
}

/// The response half of an entry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarResponse {
    /// HTTP status code (`0` for a failed or unobserved response).
    pub status: i64,
    /// HTTP status text.
    pub status_text: String,
    /// Negotiated HTTP version.
    pub http_version: String,
    /// Cookies; always empty.
    pub cookies: Vec<HarCookie>,
    /// Response headers.
    pub headers: Vec<HarHeader>,
    /// Response body metadata (size + MIME type), plus the body itself when a
    /// `Network.getResponseBody` read was fed in via [`HarRecorder::on_response_body`].
    pub content: HarContent,
    /// `Location` for a redirect, else empty.
    #[serde(rename = "redirectURL")]
    pub redirect_url: String,
    /// Header bytes, or `-1` when unknown.
    pub headers_size: i64,
    /// Encoded body bytes.
    pub body_size: i64,
    /// Non-standard error text for a failed request (`_error`, per HAR custom-field
    /// convention).
    #[serde(rename = "_error", skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl HarResponse {
    /// A status-`0` placeholder for an entry that finished without a response
    /// (e.g. a failed request, or one still in flight at flush time).
    fn placeholder() -> Self {
        HarResponse {
            status: 0,
            status_text: String::new(),
            http_version: "HTTP/1.1".to_string(),
            cookies: Vec::new(),
            headers: Vec::new(),
            content: HarContent {
                size: 0,
                mime_type: String::new(),
                text: None,
                encoding: None,
            },
            redirect_url: String::new(),
            headers_size: -1,
            body_size: 0,
            error: None,
        }
    }
}

/// A single header name/value pair.
#[derive(Debug, Clone, Serialize)]
pub struct HarHeader {
    /// Header name.
    pub name: String,
    /// Header value.
    pub value: String,
}

/// A single query-string parameter.
#[derive(Debug, Clone, Serialize)]
pub struct HarQuery {
    /// Parameter name.
    pub name: String,
    /// Parameter value (kept as it appears in the URL).
    pub value: String,
}

/// A single cookie (always empty list in practice; present for HAR conformance).
#[derive(Debug, Clone, Serialize)]
pub struct HarCookie {
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
}

/// Response body metadata, plus the body itself when captured.
///
/// `text`/`encoding` follow the HAR 1.2 `content` shape: a captured body lives in
/// `text`, and when it is binary the bytes are base64-encoded and `encoding` is
/// `"base64"` (a text body leaves `encoding` absent). Both are omitted entirely
/// when no body was captured, so a body-less recording serializes exactly as
/// before — only `size` and `mimeType` appear. This is the field
/// [`replay`](crate::replay) reads back as `ReplayBody::Inline`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarContent {
    /// Body size in bytes.
    pub size: i64,
    /// MIME type.
    pub mime_type: String,
    /// Captured response body, when available. Base64-encoded if `encoding` is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// `"base64"` when `text` holds base64-encoded binary; absent for a text body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

/// Cache info. The recorder does not model the cache, so this is always empty,
/// which serializes to `{}` as the spec allows.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HarCache {}

/// Phase timing breakdown.
///
/// HAR requires `send`, `wait`, and `receive`; the others may be `-1` (not
/// applicable). The recorder reports the whole measured duration under `wait`
/// (with `send` and `receive` at `0`), so the invariant
/// `time == send + wait + receive` holds without inventing sub-phase numbers CDP
/// did not break out. When the total is unknown (`-1`), every phase is `-1`.
#[derive(Debug, Clone, Serialize)]
pub struct HarTimings {
    /// Time spent blocked, or `-1`.
    pub blocked: f64,
    /// DNS resolution time, or `-1`.
    pub dns: f64,
    /// Connect time, or `-1`.
    pub connect: f64,
    /// Time spent sending the request.
    pub send: f64,
    /// Time spent waiting for the response.
    pub wait: f64,
    /// Time spent receiving the response.
    pub receive: f64,
    /// SSL handshake time, or `-1`.
    pub ssl: f64,
}

impl HarTimings {
    /// Phase breakdown whose non-negative parts sum to `total` (all `wait`), or
    /// all-`-1` when `total` is negative (unknown).
    fn with_total(total: f64) -> Self {
        if total < 0.0 {
            HarTimings {
                blocked: -1.0,
                dns: -1.0,
                connect: -1.0,
                send: -1.0,
                wait: -1.0,
                receive: -1.0,
                ssl: -1.0,
            }
        } else {
            HarTimings {
                blocked: -1.0,
                dns: -1.0,
                connect: -1.0,
                send: 0.0,
                wait: total,
                receive: 0.0,
                ssl: -1.0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chromiumoxide::cdp::browser_protocol::network::{
        Headers, MonotonicTime, Request as CdpRequest, RequestId, ResourcePriority, ResourceType,
        Response as CdpResponse, TimeSinceEpoch,
    };
    use serde_json::json;

    fn request(url: &str, method: &str, headers: serde_json::Value) -> CdpRequest {
        CdpRequest::builder()
            .url(url)
            .method(method)
            .headers(Headers::new(headers))
            .initial_priority(ResourcePriority::Medium)
            .referrer_policy(
                chromiumoxide::cdp::browser_protocol::network::RequestReferrerPolicy::NoReferrer,
            )
            .build()
            .expect("request builds")
    }

    fn response(
        status: i64,
        mime: &str,
        headers: serde_json::Value,
        protocol: &str,
    ) -> CdpResponse {
        CdpResponse::builder()
            .url("http://example.test/")
            .status(status)
            .status_text("OK")
            .headers(Headers::new(headers))
            .mime_type(mime)
            .charset("utf-8")
            .connection_reused(false)
            .connection_id(0.0)
            .encoded_data_length(0.0)
            .security_state(chromiumoxide::cdp::browser_protocol::security::SecurityState::Secure)
            .protocol(protocol)
            .remote_ip_address("93.184.216.34")
            .build()
            .expect("response builds")
    }

    fn will_be_sent(id: &str, url: &str, wall_time: f64, ts: f64) -> EventRequestWillBeSent {
        EventRequestWillBeSent {
            request_id: RequestId::new(id),
            loader_id: chromiumoxide::cdp::browser_protocol::network::LoaderId::new("loader"),
            document_url: url.to_string(),
            request: request(url, "GET", json!({"Accept": "text/html"})),
            timestamp: MonotonicTime::new(ts),
            wall_time: TimeSinceEpoch::new(wall_time),
            initiator: chromiumoxide::cdp::browser_protocol::network::Initiator::builder()
                .r#type(chromiumoxide::cdp::browser_protocol::network::InitiatorType::Other)
                .build()
                .expect("initiator builds"),
            redirect_has_extra_info: false,
            redirect_response: None,
            r#type: Some(ResourceType::Document),
            frame_id: None,
            has_user_gesture: None,
            render_blocking_behavior: None,
        }
    }

    fn response_received(id: &str, resp: CdpResponse, ts: f64) -> EventResponseReceived {
        EventResponseReceived {
            request_id: RequestId::new(id),
            loader_id: chromiumoxide::cdp::browser_protocol::network::LoaderId::new("loader"),
            timestamp: MonotonicTime::new(ts),
            r#type: ResourceType::Document,
            response: resp,
            has_extra_info: false,
            frame_id: None,
        }
    }

    fn loading_finished(id: &str, ts: f64, bytes: f64) -> EventLoadingFinished {
        EventLoadingFinished {
            request_id: RequestId::new(id),
            timestamp: MonotonicTime::new(ts),
            encoded_data_length: bytes,
        }
    }

    fn loading_failed(id: &str, ts: f64, err: &str) -> EventLoadingFailed {
        EventLoadingFailed {
            request_id: RequestId::new(id),
            timestamp: MonotonicTime::new(ts),
            r#type: ResourceType::Fetch,
            error_text: err.to_string(),
            canceled: None,
            blocked_reason: None,
            cors_error_status: None,
        }
    }

    fn will_be_sent_extra_info(
        id: &str,
        headers: serde_json::Value,
    ) -> EventRequestWillBeSentExtraInfo {
        EventRequestWillBeSentExtraInfo {
            request_id: RequestId::new(id),
            associated_cookies: Vec::new(),
            headers: Headers::new(headers),
            connect_timing: chromiumoxide::cdp::browser_protocol::network::ConnectTiming::new(0.0),
            client_security_state: None,
            site_has_cookie_in_other_partition: None,
            applied_network_conditions_id: None,
        }
    }

    /// The wire headers (extra-info) arriving after `requestWillBeSent` upgrade
    /// a top-level navigation's sparse provisional header set in place.
    #[test]
    fn extra_info_upgrades_sparse_navigation_headers() {
        let mut rec = HarRecorder::new();
        let mut will = will_be_sent("1", "http://site.test/page", 1.0, 10.0);
        // A top-level navigation's `requestWillBeSent` carries only the
        // provisional headers the renderer knows — no Accept, no sec-fetch-*.
        will.request = request(
            "http://site.test/page",
            "GET",
            json!({"User-Agent": "x", "Upgrade-Insecure-Requests": "1"}),
        );
        rec.on_request_will_be_sent(&will);
        rec.on_request_will_be_sent_extra_info(&will_be_sent_extra_info(
            "1",
            json!({
                "accept": "text/html,application/xhtml+xml",
                "sec-fetch-mode": "navigate",
                ":authority": "site.test",
                "user-agent": "y"
            }),
        ));
        rec.on_response_received(&response_received(
            "1",
            response(200, "text/html", json!({}), "http/1.1"),
            11.0,
        ));
        rec.on_loading_finished(&loading_finished("1", 12.0, 100.0));

        let har = rec.into_har();
        let req = &har.log.entries[0].request;
        let get = |n: &str| {
            req.headers
                .iter()
                .find(|x| x.name.eq_ignore_ascii_case(n))
                .map(|x| x.value.as_str())
        };
        // The wire-only navigation headers are now present.
        assert_eq!(get("accept"), Some("text/html,application/xhtml+xml"));
        assert_eq!(get("sec-fetch-mode"), Some("navigate"));
        // A header in both sets is replaced case-insensitively, not duplicated.
        assert_eq!(get("user-agent"), Some("y"));
        assert_eq!(
            req.headers
                .iter()
                .filter(|x| x.name.eq_ignore_ascii_case("user-agent"))
                .count(),
            1
        );
        // HTTP/2 pseudo-headers are dropped.
        assert!(req.headers.iter().all(|x| !x.name.starts_with(':')));
        // A provisional-only header is preserved.
        assert_eq!(get("upgrade-insecure-requests"), Some("1"));
    }

    /// Extra-info has no ordering guarantee: when it lands before its
    /// `requestWillBeSent`, the wire headers are stashed and applied on insert.
    #[test]
    fn extra_info_before_will_be_sent_is_stashed_and_applied() {
        let mut rec = HarRecorder::new();
        rec.on_request_will_be_sent_extra_info(&will_be_sent_extra_info(
            "7",
            json!({"accept": "text/html", "sec-fetch-dest": "document"}),
        ));
        let mut will = will_be_sent("7", "http://site.test/", 1.0, 10.0);
        will.request = request("http://site.test/", "GET", json!({"user-agent": "x"}));
        rec.on_request_will_be_sent(&will);
        rec.on_response_received(&response_received(
            "7",
            response(200, "text/html", json!({}), "http/1.1"),
            11.0,
        ));
        rec.on_loading_finished(&loading_finished("7", 12.0, 50.0));

        let har = rec.into_har();
        let req = &har.log.entries[0].request;
        let get = |n: &str| {
            req.headers
                .iter()
                .find(|x| x.name.eq_ignore_ascii_case(n))
                .map(|x| x.value.as_str())
        };
        assert_eq!(get("accept"), Some("text/html"));
        assert_eq!(get("sec-fetch-dest"), Some("document"));
        assert_eq!(get("user-agent"), Some("x"));
    }

    #[test]
    fn epoch_zero_formats_as_unix_epoch() {
        assert_eq!(iso8601_from_unix_secs(0.0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn known_epoch_formats_correctly() {
        // 1_700_000_000 is 2023-11-14T22:13:20 UTC.
        assert_eq!(
            iso8601_from_unix_secs(1_700_000_000.0),
            "2023-11-14T22:13:20.000Z"
        );
    }

    #[test]
    fn fractional_seconds_become_milliseconds() {
        assert_eq!(
            iso8601_from_unix_secs(1_700_000_000.250),
            "2023-11-14T22:13:20.250Z"
        );
    }

    #[test]
    fn millisecond_rounding_carries_into_the_next_second() {
        // 0.9996s rounds to 1000ms, which must carry into the seconds field.
        assert_eq!(iso8601_from_unix_secs(0.9996), "1970-01-01T00:00:01.000Z");
    }

    #[test]
    fn leap_year_boundary_is_correct() {
        // 1_582_934_400 is 2020-02-29T00:00:00 UTC (a leap day).
        assert_eq!(
            iso8601_from_unix_secs(1_582_934_400.0),
            "2020-02-29T00:00:00.000Z"
        );
    }

    #[test]
    fn query_string_is_parsed_and_fragment_dropped() {
        let q = query_string_from_url("http://h/p?a=1&b=2&flag#frag");
        assert_eq!(q.len(), 3);
        assert_eq!(q[0].name, "a");
        assert_eq!(q[0].value, "1");
        assert_eq!(q[2].name, "flag");
        assert_eq!(q[2].value, "");
        assert!(query_string_from_url("http://h/p").is_empty());
    }

    #[test]
    fn http_version_normalizes_from_cdp_protocol() {
        assert_eq!(normalize_http_version("h2".into()), "HTTP/2");
        assert_eq!(normalize_http_version("h3".into()), "HTTP/3");
        assert_eq!(normalize_http_version("http/1.1".into()), "HTTP/1.1");
    }

    #[test]
    fn headers_decode_into_ordered_pairs() {
        let headers = har_headers(&json!({"Content-Type": "text/html"}));
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].name, "Content-Type");
        assert_eq!(headers[0].value, "text/html");
        assert!(har_headers(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn full_request_response_finish_makes_one_entry() {
        let mut rec = HarRecorder::new();
        rec.on_request_will_be_sent(&will_be_sent(
            "1",
            "http://example.test/page?q=hi",
            1_700_000_000.0,
            100.0,
        ));
        rec.on_response_received(&response_received(
            "1",
            response(
                200,
                "text/html",
                json!({"Content-Type": "text/html"}),
                "http/1.1",
            ),
            100.2,
        ));
        rec.on_loading_finished(&loading_finished("1", 100.5, 2048.0));

        let har = rec.into_har();
        assert_eq!(har.log.entries.len(), 1);
        let entry = &har.log.entries[0];
        assert_eq!(entry.request.method, "GET");
        assert_eq!(entry.request.query_string[0].name, "q");
        assert_eq!(entry.response.status, 200);
        assert_eq!(entry.response.http_version, "HTTP/1.1");
        assert_eq!(entry.response.body_size, 2048);
        assert_eq!(entry.response.content.size, 2048);
        assert_eq!(entry.server_ip_address.as_deref(), Some("93.184.216.34"));
        // time = (100.5 - 100.0) * 1000 = 500ms, all under `wait`.
        assert!((entry.time - 500.0).abs() < 1e-6);
        assert!((entry.timings.wait - 500.0).abs() < 1e-6);
        assert_eq!(entry.timings.send, 0.0);
        assert_eq!(entry.started_date_time, "2023-11-14T22:13:20.000Z");
    }

    #[test]
    fn redirect_chain_yields_one_entry_per_hop() {
        let mut rec = HarRecorder::new();
        // Hop 1 starts.
        rec.on_request_will_be_sent(&will_be_sent(
            "1",
            "http://example.test/old",
            1_700_000_000.0,
            10.0,
        ));
        // Hop 2 reuses the id and carries the 301 redirectResponse for hop 1.
        let mut redirect = will_be_sent("1", "http://example.test/new", 1_700_000_000.5, 10.5);
        redirect.redirect_response = Some(response(
            301,
            "text/html",
            json!({"Location": "http://example.test/new"}),
            "http/1.1",
        ));
        rec.on_request_will_be_sent(&redirect);
        // Hop 2 completes.
        rec.on_response_received(&response_received(
            "1",
            response(200, "text/html", json!({}), "h2"),
            10.7,
        ));
        rec.on_loading_finished(&loading_finished("1", 11.0, 4096.0));

        let har = rec.into_har();
        assert_eq!(har.log.entries.len(), 2);
        // Hop 1: the redirect, closed when hop 2 began.
        assert_eq!(har.log.entries[0].response.status, 301);
        assert_eq!(
            har.log.entries[0].response.redirect_url,
            "http://example.test/new"
        );
        assert!((har.log.entries[0].time - 500.0).abs() < 1e-6);
        // Hop 2: the final 200 over h2.
        assert_eq!(har.log.entries[1].response.status, 200);
        assert_eq!(har.log.entries[1].response.http_version, "HTTP/2");
        assert_eq!(har.log.entries[1].request.url, "http://example.test/new");
    }

    #[test]
    fn failed_request_records_error_entry() {
        let mut rec = HarRecorder::new();
        rec.on_request_will_be_sent(&will_be_sent(
            "9",
            "http://example.test/gone",
            1_700_000_000.0,
            5.0,
        ));
        rec.on_loading_failed(&loading_failed("9", 5.25, "net::ERR_NAME_NOT_RESOLVED"));

        let har = rec.into_har();
        assert_eq!(har.log.entries.len(), 1);
        let entry = &har.log.entries[0];
        assert_eq!(entry.response.status, 0);
        assert_eq!(
            entry.response.error.as_deref(),
            Some("net::ERR_NAME_NOT_RESOLVED")
        );
        assert!((entry.time - 250.0).abs() < 1e-6);
    }

    #[test]
    fn in_flight_requests_flush_in_start_order_with_unknown_time() {
        let mut rec = HarRecorder::new();
        // Two requests start, neither finishes. Insert out of start order to
        // prove the flush sorts by start time, not map iteration order.
        rec.on_request_will_be_sent(&will_be_sent("b", "http://h/b", 1_700_000_010.0, 20.0));
        rec.on_request_will_be_sent(&will_be_sent("a", "http://h/a", 1_700_000_000.0, 10.0));

        assert_eq!(rec.finalized_len(), 0);
        let har = rec.into_har();
        assert_eq!(har.log.entries.len(), 2);
        assert_eq!(har.log.entries[0].request.url, "http://h/a");
        assert_eq!(har.log.entries[1].request.url, "http://h/b");
        assert_eq!(har.log.entries[0].time, -1.0);
        assert_eq!(har.log.entries[0].timings.wait, -1.0);
    }

    #[test]
    fn emitted_har_is_valid_round_trippable_json() {
        let mut rec = HarRecorder::with_creator("anchortree-test", "9.9.9");
        rec.on_request_will_be_sent(&will_be_sent("1", "http://h/p?x=1", 1_700_000_000.0, 1.0));
        rec.on_response_received(&response_received(
            "1",
            response(200, "application/json", json!({"X-Test": "1"}), "h2"),
            1.1,
        ));
        rec.on_loading_finished(&loading_finished("1", 1.2, 16.0));

        let json = rec.into_har().to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["log"]["version"], "1.2");
        assert_eq!(parsed["log"]["creator"]["name"], "anchortree-test");
        assert_eq!(parsed["log"]["creator"]["version"], "9.9.9");
        let entry = &parsed["log"]["entries"][0];
        assert_eq!(entry["request"]["method"], "GET");
        assert_eq!(entry["request"]["queryString"][0]["name"], "x");
        assert_eq!(entry["response"]["status"], 200);
        assert_eq!(entry["response"]["httpVersion"], "HTTP/2");
        assert_eq!(entry["response"]["content"]["mimeType"], "application/json");
        // Spec-cased odd fields survive serialization.
        assert!(entry.get("serverIPAddress").is_some());
        assert!(entry["response"].get("redirectURL").is_some());
        // The timing invariant the evaluator relies on.
        let t = &entry["timings"];
        let sum = t["send"].as_f64().unwrap()
            + t["wait"].as_f64().unwrap()
            + t["receive"].as_f64().unwrap();
        assert!((entry["time"].as_f64().unwrap() - sum).abs() < 1e-6);
    }

    #[test]
    fn text_body_is_captured_into_content_text() {
        let mut rec = HarRecorder::new();
        rec.on_request_will_be_sent(&will_be_sent("1", "http://h/page", 1_700_000_000.0, 1.0));
        rec.on_response_received(&response_received(
            "1",
            response(200, "text/html", json!({}), "h2"),
            1.1,
        ));
        // A getResponseBody read lands between the response and loadingFinished.
        rec.on_response_body(
            "1",
            ResponseBody {
                text: "<!doctype html><title>hi</title>".to_string(),
                base64: false,
            },
        );
        rec.on_loading_finished(&loading_finished("1", 1.2, 32.0));

        let har = rec.into_har();
        let content = &har.log.entries[0].response.content;
        assert_eq!(
            content.text.as_deref(),
            Some("<!doctype html><title>hi</title>")
        );
        // A text body leaves `encoding` absent.
        assert_eq!(content.encoding, None);
        // Body capture does not disturb the encoded byte count.
        assert_eq!(content.size, 32);
    }

    #[test]
    fn base64_body_sets_encoding_marker() {
        let mut rec = HarRecorder::new();
        rec.on_request_will_be_sent(&will_be_sent("1", "http://h/img.png", 1_700_000_000.0, 1.0));
        rec.on_response_received(&response_received(
            "1",
            response(200, "image/png", json!({}), "h2"),
            1.1,
        ));
        rec.on_response_body(
            "1",
            ResponseBody {
                text: "iVBORw0KGgo=".to_string(),
                base64: true,
            },
        );
        rec.on_loading_finished(&loading_finished("1", 1.2, 8.0));

        let content = &rec.into_har().log.entries[0].response.content;
        assert_eq!(content.text.as_deref(), Some("iVBORw0KGgo="));
        assert_eq!(content.encoding.as_deref(), Some("base64"));
    }

    #[test]
    fn no_body_capture_leaves_content_fields_absent_in_json() {
        let mut rec = HarRecorder::new();
        rec.on_request_will_be_sent(&will_be_sent("1", "http://h/p", 1_700_000_000.0, 1.0));
        rec.on_response_received(&response_received(
            "1",
            response(200, "text/html", json!({}), "h2"),
            1.1,
        ));
        rec.on_loading_finished(&loading_finished("1", 1.2, 16.0));

        // Without a body, `text`/`encoding` must serialize away entirely so a
        // body-less recording is byte-identical to the pre-capture output.
        let json: serde_json::Value =
            serde_json::from_str(&rec.into_har().to_json()).expect("valid JSON");
        let content = &json["log"]["entries"][0]["response"]["content"];
        assert!(content.get("text").is_none());
        assert!(content.get("encoding").is_none());
        assert_eq!(content["size"], 16);
    }

    #[test]
    fn captured_text_body_serializes_under_content_text() {
        let mut rec = HarRecorder::new();
        rec.on_request_will_be_sent(&will_be_sent("1", "http://h/api", 1_700_000_000.0, 1.0));
        rec.on_response_received(&response_received(
            "1",
            response(200, "application/json", json!({}), "h2"),
            1.1,
        ));
        rec.on_response_body(
            "1",
            ResponseBody {
                text: r#"{"ok":true}"#.to_string(),
                base64: false,
            },
        );
        rec.on_loading_finished(&loading_finished("1", 1.2, 11.0));

        let json: serde_json::Value =
            serde_json::from_str(&rec.into_har().to_json()).expect("valid JSON");
        let content = &json["log"]["entries"][0]["response"]["content"];
        assert_eq!(content["text"], r#"{"ok":true}"#);
        assert!(content.get("encoding").is_none());
    }

    #[test]
    fn body_for_unknown_request_is_ignored() {
        let mut rec = HarRecorder::new();
        // No request with id "99" is in flight: the call is a tolerant no-op.
        rec.on_response_body(
            "99",
            ResponseBody {
                text: "orphan".to_string(),
                base64: false,
            },
        );
        rec.on_request_will_be_sent(&will_be_sent("1", "http://h/p", 1_700_000_000.0, 1.0));
        rec.on_loading_finished(&loading_finished("1", 1.2, 4.0));

        let entry = &rec.into_har().log.entries[0];
        assert_eq!(entry.response.content.text, None);
    }
}

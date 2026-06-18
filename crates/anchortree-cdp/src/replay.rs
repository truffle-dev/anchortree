//! Phase 3.5b Tier 1: the hermetic HAR replay **matcher** — the browser-free
//! heart of the `HAR -> chromium` fulfill layer (DECISIONS D33).
//!
//! ## Why this exists
//!
//! The baseline axis of the benchmark report (M = per-turn accessibility + DOM +
//! layout the engine diffs) needs the engine to *observe a real page*, and the
//! run-25 D32 correction proved a `network.har` cannot produce that on its own: a
//! HAR is a network trace, and [`har`](crate::har) is record-only (it emits a
//! [`Har`](crate::har::Har), it never replays one). D33 pins the fix as a two-tier
//! mechanism; this module is the hermetic, CI-runnable core of **Tier 1**: given a
//! corpus task's recorded HAR, decide for each live request the browser issues
//! which recorded response to serve back — so the engine can render the captured
//! page offline and the observe loop can run against it.
//!
//! ## What this module is, and is not
//!
//! It is the **matcher**: pure logic over a parsed HAR. It answers exactly one
//! question — *for this outgoing request, which recorded entry (if any) is the
//! response?* — mirroring Playwright's `routeFromHAR` selection rule:
//!
//! - **URL and method are strict.** A candidate entry must have the same method
//!   (case-insensitive, per HTTP) and the byte-identical URL.
//! - **POST payload is strict when the live request carries one.** If the request
//!   has a body, a candidate must have recorded the identical body; a request with
//!   no body does not constrain on payload.
//! - **Ties break by most-matching headers.** Among equally-qualified candidates,
//!   the one sharing the most request headers (name case-insensitive, value exact)
//!   with the live request wins; a remaining tie takes the earlier recording.
//! - **No match is an abort, not a guess** ([`MatchOutcome::Abort`]). This is the
//!   D30 honesty guard carried to the byte: an off-trajectory request fails loudly
//!   rather than silently rendering a wrong page and contaminating M. It mirrors
//!   Playwright's `notFound: "abort"`.
//!
//! It is **not** the CDP wiring and **not** the body fulfiller. Decoding a live
//! `Fetch.requestPaused` into a [`ReplayRequest`] and calling `Fetch.fulfillRequest`
//! with the matched [`ReplayEntry`]'s response is the live follow-up (it needs a
//! browser, so it is proven by an example, not in CI — the project's pattern for
//! transport-touching code). This module stays browser-free and CDP-free so the
//! whole selection rule is unit-tested without a Chrome, and it sits behind the
//! transport seam (D9): it is named in the transport-neutrality guard's fusion
//! path, never in its CDP-adapter set.
//!
//! ## The read model is its own thing
//!
//! [`har`](crate::har)'s `Har`/`HarEntry`/... are a *write* contract: `Serialize`
//! only, and deliberately body-less (the recorder does not capture response
//! bodies). Replay *reads* a HAR produced by some other tool (the vendored corpus
//! HARs are browser-use / Playwright captures), which carries bodies — sometimes
//! inline as `content.text`, sometimes as an external `content._file` reference.
//! So replay models the read side independently ([`ReplayHar`] and friends,
//! `Deserialize`, tolerant of unknown fields), exactly as run-25 split the
//! read-side `AgentAnswer` from the write-only `runner::AgentResponse`. Body
//! resolution (inline vs external file) is surfaced as [`ReplayBody`] for the
//! fulfiller; this module does not read the external files.

use serde::Deserialize;

/// A live request the replayed browser issues, reduced to the fields the matcher
/// keys on. The CDP layer decodes a `Fetch.requestPaused` event into this at the
/// transport seam; the matcher never sees a CDP type.
#[derive(Debug, Clone)]
pub struct ReplayRequest {
    /// HTTP method (compared case-insensitively).
    pub method: String,
    /// Full request URL (compared byte-for-byte).
    pub url: String,
    /// Request body, when the request carries one (compared byte-for-byte).
    pub post_data: Option<String>,
    /// Request headers as `(name, value)`; names match case-insensitively.
    pub headers: Vec<(String, String)>,
}

impl ReplayRequest {
    /// A bodyless GET-style request with no headers — the common case and a terse
    /// test constructor.
    pub fn get(url: impl Into<String>) -> Self {
        ReplayRequest {
            method: "GET".to_string(),
            url: url.into(),
            post_data: None,
            headers: Vec::new(),
        }
    }
}

/// A parsed HAR, read side. Tolerant of unknown fields so real third-party
/// captures (browser-use, Playwright) parse without modeling every key.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReplayHar {
    log: ReplayLog,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ReplayLog {
    #[serde(default)]
    entries: Vec<ReplayEntry>,
}

/// One recorded request/response exchange, read side.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReplayEntry {
    request: ReqRecord,
    response: RespRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ReqRecord {
    method: String,
    url: String,
    #[serde(rename = "postData", default)]
    post_data: Option<PostData>,
    #[serde(default)]
    headers: Vec<NameValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct PostData {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RespRecord {
    status: i64,
    #[serde(default)]
    headers: Vec<NameValue>,
    #[serde(default)]
    content: RespContent,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct RespContent {
    #[serde(rename = "mimeType", default)]
    mime_type: String,
    #[serde(default)]
    text: Option<String>,
    /// `base64` when `text` is base64-encoded, else absent.
    #[serde(default)]
    encoding: Option<String>,
    /// External body file reference (`content._file`), used by captures that store
    /// bodies outside the HAR. The matcher does not read it; the fulfiller does.
    #[serde(rename = "_file", default)]
    file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct NameValue {
    name: String,
    value: String,
}

/// Where a matched entry's response body lives. The fulfiller resolves this; the
/// matcher only surfaces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayBody<'h> {
    /// No body was recorded.
    Empty,
    /// The body is inline in the HAR. `base64` is true when it was stored
    /// base64-encoded (`content.encoding == "base64"`).
    Inline {
        /// The recorded body text (raw or base64).
        text: &'h str,
        /// Whether `text` is base64-encoded.
        base64: bool,
    },
    /// The body is in an external file referenced by `content._file`, relative to
    /// the HAR. The matcher does not open it.
    External(&'h str),
}

impl ReplayEntry {
    /// The recorded HTTP status.
    pub fn status(&self) -> i64 {
        self.response.status
    }

    /// The recorded response MIME type (`""` if none).
    pub fn mime_type(&self) -> &str {
        &self.response.content.mime_type
    }

    /// The recorded response headers as `(name, value)` pairs.
    pub fn response_headers(&self) -> impl Iterator<Item = (&str, &str)> {
        self.response
            .headers
            .iter()
            .map(|h| (h.name.as_str(), h.value.as_str()))
    }

    /// Where this entry's response body lives, for the fulfiller to resolve. Inline
    /// `content.text` wins over an external `_file` when both are present (the
    /// inline copy is authoritative).
    pub fn body(&self) -> ReplayBody<'_> {
        let c = &self.response.content;
        if let Some(text) = &c.text {
            ReplayBody::Inline {
                text,
                base64: c.encoding.as_deref() == Some("base64"),
            }
        } else if let Some(file) = &c.file {
            ReplayBody::External(file)
        } else {
            ReplayBody::Empty
        }
    }

    /// How many of `req`'s headers this entry's *request* also recorded (name
    /// case-insensitive, value exact). The tie-breaker score.
    fn header_match_score(&self, req: &ReplayRequest) -> usize {
        req.headers
            .iter()
            .filter(|(name, value)| {
                self.request
                    .headers
                    .iter()
                    .any(|h| h.name.eq_ignore_ascii_case(name) && &h.value == value)
            })
            .count()
    }

    /// Whether this entry is a candidate for `req`: same method (case-insensitive)
    /// and identical URL, and — when `req` carries a body — the identical body.
    fn is_candidate(&self, req: &ReplayRequest) -> bool {
        if !self.request.method.eq_ignore_ascii_case(&req.method) || self.request.url != req.url {
            return false;
        }
        match &req.post_data {
            Some(body) => {
                self.request
                    .post_data
                    .as_ref()
                    .and_then(|p| p.text.as_ref())
                    == Some(body)
            }
            None => true,
        }
    }
}

impl ReplayHar {
    /// Parse a `network.har` document. Unknown fields are ignored, so real
    /// third-party captures parse without a full HAR model.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Every recorded entry, in recording order.
    pub fn entries(&self) -> &[ReplayEntry] {
        &self.log.entries
    }

    /// Select the recorded entry that answers `req`, applying the `routeFromHAR`
    /// rule: strict URL + method, strict body when present, ties broken by the most
    /// matching request headers (then earliest recording). `None` means no entry
    /// qualifies — the caller should abort the request (see [`Self::outcome`]).
    pub fn match_entry(&self, req: &ReplayRequest) -> Option<&ReplayEntry> {
        self.log
            .entries
            .iter()
            .filter(|e| e.is_candidate(req))
            .max_by_key(|e| e.header_match_score(req))
    }

    /// The replay decision for `req`: serve the matched entry, or abort. Aborting
    /// on no-match (rather than serving a fallback) is the honesty guard — an
    /// off-trajectory request must not silently render a wrong page and pollute the
    /// baseline.
    pub fn outcome(&self, req: &ReplayRequest) -> MatchOutcome<'_> {
        match self.match_entry(req) {
            Some(entry) => MatchOutcome::Fulfill(entry),
            None => MatchOutcome::Abort,
        }
    }
}

/// What the replay layer should do with a live request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchOutcome<'h> {
    /// Serve this recorded entry's response back to the browser.
    Fulfill(&'h ReplayEntry),
    /// No recorded entry matches; fail the request loudly (`Fetch.failRequest`).
    Abort,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-entry HAR JSON with the given request method/url and a 200
    /// response, optionally with a request body and extra request headers.
    fn har_json(entries: &str) -> String {
        format!(r#"{{"log":{{"version":"1.2","entries":[{entries}]}}}}"#)
    }

    /// A GET entry for `url` returning status 200 with the given inline body.
    fn get_entry(url: &str, body: &str) -> String {
        format!(
            r#"{{"request":{{"method":"GET","url":"{url}","headers":[]}},
               "response":{{"status":200,"headers":[],
                 "content":{{"mimeType":"text/html","text":"{body}"}}}}}}"#
        )
    }

    #[test]
    fn matches_a_single_get_by_url_and_method() {
        let har =
            ReplayHar::from_json(&har_json(&get_entry("https://x.test/a", "PAGE A"))).unwrap();
        let entry = har
            .match_entry(&ReplayRequest::get("https://x.test/a"))
            .unwrap();
        assert_eq!(entry.status(), 200);
        assert_eq!(
            entry.body(),
            ReplayBody::Inline {
                text: "PAGE A",
                base64: false
            }
        );
    }

    #[test]
    fn url_mismatch_is_an_abort() {
        let har = ReplayHar::from_json(&har_json(&get_entry("https://x.test/a", "A"))).unwrap();
        assert_eq!(
            har.match_entry(&ReplayRequest::get("https://x.test/b")),
            None
        );
        assert_eq!(
            har.outcome(&ReplayRequest::get("https://x.test/b")),
            MatchOutcome::Abort
        );
    }

    #[test]
    fn method_mismatch_is_an_abort() {
        let har = ReplayHar::from_json(&har_json(&get_entry("https://x.test/a", "A"))).unwrap();
        let post = ReplayRequest {
            method: "POST".to_string(),
            url: "https://x.test/a".to_string(),
            post_data: Some("{}".to_string()),
            headers: Vec::new(),
        };
        assert_eq!(har.outcome(&post), MatchOutcome::Abort);
    }

    #[test]
    fn method_compare_is_case_insensitive() {
        let har = ReplayHar::from_json(&har_json(&get_entry("https://x.test/a", "A"))).unwrap();
        let lower = ReplayRequest {
            method: "get".to_string(),
            url: "https://x.test/a".to_string(),
            post_data: None,
            headers: Vec::new(),
        };
        assert!(har.match_entry(&lower).is_some(), "get == GET");
    }

    #[test]
    fn post_payload_is_strict() {
        // Two POSTs to the same URL with different bodies; the matcher must pick the
        // one whose recorded body is identical to the request's.
        let entries = format!(
            "{},{}",
            r#"{"request":{"method":"POST","url":"https://x.test/cart","headers":[],
                 "postData":{"text":"qty=1"}},
               "response":{"status":200,"headers":[],"content":{"mimeType":"","text":"ONE"}}}"#,
            r#"{"request":{"method":"POST","url":"https://x.test/cart","headers":[],
                 "postData":{"text":"qty=2"}},
               "response":{"status":200,"headers":[],"content":{"mimeType":"","text":"TWO"}}}"#
        );
        let har = ReplayHar::from_json(&har_json(&entries)).unwrap();
        let req = ReplayRequest {
            method: "POST".to_string(),
            url: "https://x.test/cart".to_string(),
            post_data: Some("qty=2".to_string()),
            headers: Vec::new(),
        };
        let entry = har.match_entry(&req).unwrap();
        assert_eq!(
            entry.body(),
            ReplayBody::Inline {
                text: "TWO",
                base64: false
            }
        );

        // A body that matches neither recording aborts.
        let miss = ReplayRequest {
            method: "POST".to_string(),
            url: "https://x.test/cart".to_string(),
            post_data: Some("qty=9".to_string()),
            headers: Vec::new(),
        };
        assert_eq!(har.outcome(&miss), MatchOutcome::Abort);
    }

    #[test]
    fn ties_break_by_most_matching_headers() {
        // Two entries, same method+url; the request shares two headers with the
        // second and one with the first, so the second must win.
        let entries = format!(
            "{},{}",
            r#"{"request":{"method":"GET","url":"https://x.test/p",
                 "headers":[{"name":"Accept","value":"text/html"}]},
               "response":{"status":200,"headers":[],"content":{"mimeType":"","text":"FIRST"}}}"#,
            r#"{"request":{"method":"GET","url":"https://x.test/p",
                 "headers":[{"name":"Accept","value":"text/html"},
                            {"name":"X-Tab","value":"7"}]},
               "response":{"status":200,"headers":[],"content":{"mimeType":"","text":"SECOND"}}}"#
        );
        let har = ReplayHar::from_json(&har_json(&entries)).unwrap();
        let req = ReplayRequest {
            method: "GET".to_string(),
            url: "https://x.test/p".to_string(),
            post_data: None,
            headers: vec![
                ("accept".to_string(), "text/html".to_string()), // name case-insensitive
                ("X-Tab".to_string(), "7".to_string()),
            ],
        };
        let entry = har.match_entry(&req).unwrap();
        assert_eq!(
            entry.body(),
            ReplayBody::Inline {
                text: "SECOND",
                base64: false
            }
        );
    }

    #[test]
    fn body_accessor_covers_inline_base64_external_and_empty() {
        let entries = format!(
            "{},{},{}",
            r#"{"request":{"method":"GET","url":"https://x.test/b64","headers":[]},
               "response":{"status":200,"headers":[],
                 "content":{"mimeType":"image/png","text":"aGk=","encoding":"base64"}}}"#,
            r#"{"request":{"method":"GET","url":"https://x.test/ext","headers":[]},
               "response":{"status":200,"headers":[],
                 "content":{"mimeType":"text/html","_file":"resources/42.html"}}}"#,
            r#"{"request":{"method":"GET","url":"https://x.test/empty","headers":[]},
               "response":{"status":204,"headers":[],"content":{"mimeType":""}}}"#
        );
        let har = ReplayHar::from_json(&har_json(&entries)).unwrap();
        assert_eq!(
            har.match_entry(&ReplayRequest::get("https://x.test/b64"))
                .unwrap()
                .body(),
            ReplayBody::Inline {
                text: "aGk=",
                base64: true
            }
        );
        assert_eq!(
            har.match_entry(&ReplayRequest::get("https://x.test/ext"))
                .unwrap()
                .body(),
            ReplayBody::External("resources/42.html")
        );
        let empty = har
            .match_entry(&ReplayRequest::get("https://x.test/empty"))
            .unwrap();
        assert_eq!(empty.body(), ReplayBody::Empty);
        assert_eq!(empty.status(), 204);
    }

    #[test]
    fn response_headers_and_mime_are_surfaced_for_the_fulfiller() {
        let entry_json = r#"{"request":{"method":"GET","url":"https://x.test/h","headers":[]},
               "response":{"status":200,
                 "headers":[{"name":"Content-Type","value":"application/json"}],
                 "content":{"mimeType":"application/json","text":"{}"}}}"#;
        let har = ReplayHar::from_json(&har_json(entry_json)).unwrap();
        let entry = har
            .match_entry(&ReplayRequest::get("https://x.test/h"))
            .unwrap();
        assert_eq!(entry.mime_type(), "application/json");
        let headers: Vec<(&str, &str)> = entry.response_headers().collect();
        assert_eq!(headers, vec![("Content-Type", "application/json")]);
    }

    #[test]
    fn unknown_har_fields_are_tolerated() {
        // A realistic capture carries many fields the read model does not name
        // (httpVersion, cookies, queryString, timings, _file, serverIPAddress).
        let entry_json = r#"{"startedDateTime":"2026-01-01T00:00:00.000Z","time":12.5,
               "request":{"method":"GET","url":"https://x.test/real","httpVersion":"HTTP/2",
                 "cookies":[],"headers":[{"name":":authority","value":"x.test"}],
                 "queryString":[],"headersSize":-1,"bodySize":0},
               "response":{"status":200,"statusText":"OK","httpVersion":"HTTP/2","cookies":[],
                 "headers":[],"content":{"size":-1,"mimeType":"text/html","_file":"r/1.html"},
                 "redirectURL":"","headersSize":-1,"bodySize":-1},
               "cache":{},"timings":{"send":0,"wait":12,"receive":0},
               "serverIPAddress":"1.2.3.4"}"#;
        let har = ReplayHar::from_json(&har_json(entry_json)).unwrap();
        assert_eq!(har.entries().len(), 1);
        let entry = har
            .match_entry(&ReplayRequest::get("https://x.test/real"))
            .unwrap();
        assert_eq!(entry.body(), ReplayBody::External("r/1.html"));
    }

    #[test]
    fn empty_har_aborts_everything() {
        let har = ReplayHar::from_json(&har_json("")).unwrap();
        assert!(har.entries().is_empty());
        assert_eq!(
            har.outcome(&ReplayRequest::get("https://x.test/a")),
            MatchOutcome::Abort
        );
    }
}

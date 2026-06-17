//! Session acquisition for hosted browser gateways.
//!
//! [`connect`](crate::connect) speaks CDP over `wss://` after Phase 1.5b made
//! the WebSocket leg TLS-capable. But a hosted gateway does not hand you a bare
//! `wss://` URL up front — you first authenticate to its HTTP control plane and
//! it returns a *self-authenticating* WebSocket URL, with the credential carried
//! in the URL (a query string or a session-scoped path), never in a header. That
//! is not a stylistic choice: chromiumoxide 0.9.1 offers no hook to set an
//! `Authorization` header on the WS handshake, so header auth is structurally
//! impossible through [`connect`](crate::connect) (DECISIONS D18). The
//! URL-borne-credential model is the one both supported gateways already use.
//!
//! Two providers fit that model with slightly different shapes:
//!
//! - **Cloudflare Browser Run** exposes the CDP endpoint at a stable per-account
//!   path and accepts the API token as a `?token=` query parameter. No HTTP
//!   round-trip is needed: the URL is built directly and is itself the
//!   credential. See [`cloudflare::devtools_ws_url`].
//! - **Browserbase** mints a session with a `POST` to its REST API and returns a
//!   `connectUrl` of the form
//!   `wss://connect.browserbase.com/v1/sessions/<id>?apiKey=<key>`. This one
//!   needs the round-trip. See [`browserbase::acquire`].
//!
//! Both paths end at the same call site:
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use anchortree_cdp::{connect, gateway};
//!
//! // Cloudflare: pure URL build, no network.
//! let url = gateway::cloudflare::devtools_ws_url("ACCOUNT_ID", "API_TOKEN");
//! let _session = connect(url).await?;
//!
//! // Browserbase: REST mint, then the same connect.
//! let acquired = gateway::browserbase::acquire("PROJECT_ID", "API_KEY").await?;
//! let _session = connect(acquired.connect_url).await?;
//! # Ok(())
//! # }
//! ```
//!
//! Everything in this module is provider *plumbing*, deliberately kept out of
//! `anchortree-core`: the engine never learns where a browser came from.

use serde::Deserialize;

use crate::error::GatewayError;
use crate::observer::ensure_ring_provider;

/// A session minted on a hosted browser gateway, ready to drive over CDP.
#[derive(Debug, Clone)]
pub struct AcquiredSession {
    /// The self-authenticating CDP WebSocket URL (`wss://...`), ready to hand
    /// straight to [`connect`](crate::connect). The credential rides in the URL,
    /// never an HTTP header, because the WS handshake carries none (D18).
    pub connect_url: String,
    /// The provider's session id, when one is surfaced. Useful for teardown,
    /// a session-replay link, or correlating logs. `None` for providers (like
    /// the Cloudflare query-param path) that do not return a separate id.
    pub session_id: Option<String>,
}

/// Largest response-body prefix kept in a [`GatewayError::Status`]. Enough to
/// carry a gateway's JSON error envelope without unbounded logs.
const BODY_SNIPPET_MAX: usize = 512;

/// Truncate a response body for inclusion in an error, never splitting a UTF-8
/// scalar and annotating the original length when it is cut.
fn snippet(body: &str) -> String {
    if body.len() <= BODY_SNIPPET_MAX {
        return body.to_string();
    }
    let mut end = BODY_SNIPPET_MAX;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… ({} bytes total)", &body[..end], body.len())
}

/// Build the shared HTTP client used for control-plane calls.
///
/// reqwest is compiled with `rustls-no-provider`, so it negotiates TLS through
/// whatever process-default crypto provider is installed. We install `ring`
/// here for the same reason the WS path does (aws-lc-rs needs a C toolchain we
/// lack — D10); this keeps the HTTP and WebSocket legs on one provider.
fn http_client() -> Result<reqwest::Client, GatewayError> {
    ensure_ring_provider();
    Ok(reqwest::Client::builder()
        .user_agent(concat!("anchortree-cdp/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// Cloudflare Browser Run.
///
/// As of the 2026-04-10 GA, Browser Run serves raw CDP over a WebSocket at a
/// stable per-account path, authenticated by an API token (Browser Rendering -
/// Edit permission) carried as a `?token=` query parameter. Because the
/// credential lives in the URL, no HTTP round-trip is required: the URL is built
/// directly and handed to [`connect`](crate::connect).
pub mod cloudflare {
    /// The Cloudflare API host the Browser Run CDP endpoint lives under.
    const API_HOST: &str = "https://api.cloudflare.com";

    /// Build the per-account CDP base URL (no credential), as documented for
    /// Browser Run: `.../accounts/<id>/browser-rendering/devtools/browser`. The
    /// scheme is `https` here; [`devtools_ws_url`] swaps it to `wss` for the
    /// WebSocket upgrade. Exposed for callers that want to inspect or log the
    /// endpoint without the token in it.
    pub fn devtools_base_url(account_id: &str) -> String {
        format!("{API_HOST}/client/v4/accounts/{account_id}/browser-rendering/devtools/browser")
    }

    /// Build the self-authenticating `wss://` CDP URL for an account, with the
    /// API token as a `?token=` query parameter. Hand the result straight to
    /// [`connect`](crate::connect).
    ///
    /// The token is percent-encoded for query-string safety. The host
    /// (`api.cloudflare.com`) is fixed and the account id is path-positional, so
    /// only the token can contain reserved characters.
    pub fn devtools_ws_url(account_id: &str, token: &str) -> String {
        let base = devtools_base_url(account_id);
        // api.cloudflare.com is always https; rewrite only the scheme to wss so
        // the path/host stay byte-identical to the documented endpoint.
        let wss_base = base.replacen("https://", "wss://", 1);
        format!("{wss_base}?token={}", encode_query_component(token))
    }

    /// Minimal percent-encoding for a query-string value: escape every byte that
    /// is not an RFC 3986 unreserved character. Small and dependency-free; the
    /// only values passed here are opaque API tokens.
    fn encode_query_component(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        for &b in value.as_bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char);
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn base_url_is_the_documented_per_account_path() {
            assert_eq!(
                devtools_base_url("abc123"),
                "https://api.cloudflare.com/client/v4/accounts/abc123/browser-rendering/devtools/browser"
            );
        }

        #[test]
        fn ws_url_is_wss_with_the_token_in_the_query() {
            let url = devtools_ws_url("acc", "tok-plain");
            assert_eq!(
                url,
                "wss://api.cloudflare.com/client/v4/accounts/acc/browser-rendering/devtools/browser?token=tok-plain"
            );
            assert!(url.starts_with("wss://"));
        }

        #[test]
        fn ws_url_percent_encodes_reserved_token_bytes() {
            // A token carrying reserved characters must not break the query.
            let url = devtools_ws_url("acc", "a/b c+d?e&f");
            assert!(url.ends_with("?token=a%2Fb%20c%2Bd%3Fe%26f"), "got: {url}");
            assert!(!url[url.find("?token=").unwrap()..].contains('&'));
        }

        #[test]
        fn unreserved_token_bytes_pass_through_unescaped() {
            assert_eq!(encode_query_component("Aa0-_.~"), "Aa0-_.~");
        }
    }
}

/// Browserbase.
///
/// A session is minted with `POST https://api.browserbase.com/v1/sessions`
/// (`X-BB-API-Key` header, `{"projectId": ...}` body). The reply carries a
/// `connectUrl` — `wss://connect.browserbase.com/v1/sessions/<id>?apiKey=<key>`
/// — already self-authenticating, plus the session `id`.
pub mod browserbase {
    use super::{AcquiredSession, Deserialize, GatewayError, http_client, snippet};

    /// The create-session REST endpoint.
    const SESSIONS_ENDPOINT: &str = "https://api.browserbase.com/v1/sessions";

    /// Build the create-session request body. Pure so its shape is pinned by a
    /// test rather than discovered in production.
    pub(crate) fn create_session_body(project_id: &str) -> serde_json::Value {
        serde_json::json!({ "projectId": project_id })
    }

    /// The subset of the create-session reply we depend on. Extra fields
    /// (`seleniumRemoteUrl`, `signingKey`, `status`, …) are ignored.
    #[derive(Deserialize)]
    struct CreateSessionReply {
        id: Option<String>,
        #[serde(rename = "connectUrl")]
        connect_url: Option<String>,
    }

    /// Parse a 2xx create-session body into an [`AcquiredSession`]. Pure and
    /// tested: malformed JSON and a missing `connectUrl` both become
    /// [`GatewayError::Malformed`].
    pub(crate) fn parse_session_reply(json: &str) -> Result<AcquiredSession, GatewayError> {
        let reply: CreateSessionReply = serde_json::from_str(json).map_err(|e| {
            GatewayError::Malformed(format!("create-session reply was not valid JSON: {e}"))
        })?;
        let connect_url = reply.connect_url.ok_or_else(|| {
            GatewayError::Malformed("create-session reply carried no `connectUrl`".to_string())
        })?;
        Ok(AcquiredSession {
            connect_url,
            session_id: reply.id,
        })
    }

    /// Mint a Browserbase session and return its self-authenticating
    /// `connectUrl` (plus session id), ready for [`connect`](crate::connect).
    ///
    /// `api_key` travels in the `X-BB-API-Key` header on this HTTP call only;
    /// the returned `connectUrl` embeds its own `?apiKey=` for the WS upgrade.
    pub async fn acquire(project_id: &str, api_key: &str) -> Result<AcquiredSession, GatewayError> {
        let client = http_client()?;
        let resp = client
            .post(SESSIONS_ENDPOINT)
            .header("X-BB-API-Key", api_key)
            .json(&create_session_body(project_id))
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(GatewayError::Status {
                status: status.as_u16(),
                body: snippet(&body),
            });
        }
        parse_session_reply(&body)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn create_body_carries_the_project_id() {
            assert_eq!(
                create_session_body("proj-7"),
                serde_json::json!({ "projectId": "proj-7" })
            );
        }

        #[test]
        fn parse_extracts_connect_url_and_id() {
            // Shape per docs.browserbase.com/reference/api/create-a-session;
            // extra fields are present and must be ignored.
            let json = r#"{
                "id": "sess-abc",
                "connectUrl": "wss://connect.browserbase.com/v1/sessions/sess-abc?apiKey=k",
                "seleniumRemoteUrl": "https://…",
                "signingKey": "…",
                "status": "RUNNING"
            }"#;
            let acquired = parse_session_reply(json).expect("parses");
            assert_eq!(
                acquired.connect_url,
                "wss://connect.browserbase.com/v1/sessions/sess-abc?apiKey=k"
            );
            assert_eq!(acquired.session_id.as_deref(), Some("sess-abc"));
        }

        #[test]
        fn parse_session_id_is_optional() {
            let json =
                r#"{ "connectUrl": "wss://connect.browserbase.com/v1/sessions/x?apiKey=k" }"#;
            let acquired = parse_session_reply(json).expect("parses without id");
            assert!(acquired.session_id.is_none());
        }

        #[test]
        fn parse_missing_connect_url_is_malformed() {
            let json = r#"{ "id": "sess-abc", "status": "RUNNING" }"#;
            let err = parse_session_reply(json).expect_err("no connectUrl");
            assert!(matches!(err, GatewayError::Malformed(_)));
        }

        #[test]
        fn parse_non_json_is_malformed() {
            let err = parse_session_reply("<html>502 Bad Gateway</html>").expect_err("not json");
            assert!(matches!(err, GatewayError::Malformed(_)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_passes_short_bodies_through() {
        assert_eq!(snippet("short"), "short");
    }

    #[test]
    fn snippet_truncates_long_bodies_on_a_char_boundary() {
        let body = "x".repeat(BODY_SNIPPET_MAX + 50);
        let s = snippet(&body);
        assert!(s.starts_with(&"x".repeat(BODY_SNIPPET_MAX)));
        assert!(s.contains(&format!("({} bytes total)", BODY_SNIPPET_MAX + 50)));
    }

    #[test]
    fn snippet_never_splits_a_multibyte_scalar() {
        // A body of multibyte scalars longer than the cap must still produce
        // valid UTF-8 (the truncation walks back to a boundary).
        let body = "é".repeat(BODY_SNIPPET_MAX); // 2 bytes each => well over the cap
        let s = snippet(&body);
        // The very fact that this does not panic and yields a &str proves the
        // boundary walk; assert it carried the length annotation too.
        assert!(s.contains("bytes total)"));
    }
}

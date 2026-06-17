//! Errors surfaced by the CDP observer.

/// A failure while connecting to or observing a CDP browser.
#[derive(Debug, thiserror::Error)]
pub enum CdpError {
    /// The underlying chromiumoxide / CDP transport failed.
    #[error("cdp transport error: {0}")]
    Cdp(#[from] chromiumoxide::error::CdpError),

    /// A CDP reply was missing a field the fusion needs, or was otherwise
    /// shaped in a way we could not decode.
    #[error("malformed cdp reply: {0}")]
    Malformed(String),
}

/// A failure while acquiring a session from a hosted browser gateway
/// (Cloudflare Browser Run, Browserbase) over its HTTP control-plane API,
/// before any CDP WebSocket is opened.
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    /// The HTTP request to the gateway's control-plane API failed at the
    /// transport level (DNS, TLS, connection, timeout).
    #[error("gateway http transport error: {0}")]
    Http(#[from] reqwest::Error),

    /// The gateway answered, but with a non-success status. Carries the status
    /// code and a bounded snippet of the response body for diagnosis. Auth
    /// failures (401/403) and quota/plan errors (429) surface here.
    #[error("gateway returned {status}: {body}")]
    Status {
        /// The HTTP status code the gateway returned.
        status: u16,
        /// A truncated snippet of the response body, for diagnosis.
        body: String,
    },

    /// The gateway answered 2xx, but the JSON did not carry the field we need
    /// to open a CDP connection (e.g. Browserbase omitted `connectUrl`).
    #[error("gateway reply missing required field: {0}")]
    Malformed(String),
}

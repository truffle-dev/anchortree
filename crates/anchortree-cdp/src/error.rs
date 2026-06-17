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

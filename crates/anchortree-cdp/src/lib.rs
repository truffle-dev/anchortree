//! `anchortree-cdp`: the live browser adapter for anchortree.
//!
//! This crate is the bridge between the pure, browser-free identity engine in
//! `anchortree-core` and a real Chrome over the Chrome DevTools Protocol. It
//! drives one fused accessibility + DOM + layout pass and produces the flat
//! `Vec<ObservedNode>` the engine consumes, implementing
//! [`ObservationSource`](anchortree_core::ObservationSource) so a consumer can
//! write the whole agent loop against the trait and swap a canned source in
//! tests.
//!
//! The crate is split so that almost all of the interesting logic stays
//! testable without a browser:
//!
//! - [`fuse`] is the browser-free heart: it decides which roles survive, how
//!   element state is read off accessibility properties, and how a structural
//!   path is built. Every one of those decisions has a unit test.
//! - [`observer`] is the thin mechanical adapter: it issues CDP requests,
//!   decodes the replies into [`fuse`]'s plain inputs, and calls [`fuse::fuse`].
//!
//! ## Transport
//!
//! Connections are made over either a plain CDP WebSocket (`ws://`, e.g. a
//! locally launched Chrome) or a TLS one (`wss://`, e.g. a hosted gateway like
//! Cloudflare Browser Run or Browserbase). [`connect`] accepts both and routes
//! `wss://` through rustls on the `ring` crypto provider, trusting the bundled
//! Mozilla `webpki-roots` so no system certificate store is required. The
//! provider choice and the reason it is `ring` rather than aws-lc-rs are
//! recorded in `DECISIONS.md` (D8/D10).

pub mod actions;
pub mod error;
pub mod fuse;
pub mod observer;

pub use actions::{ActError, Action, act, act_mark};
pub use error::CdpError;
pub use observer::{CdpObserver, Session, connect, is_tls_endpoint};

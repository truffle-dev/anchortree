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
//! Connections are made over a non-TLS CDP WebSocket (`ws://`). This covers a
//! locally launched Chrome and CDP gateways that expose a plain-WebSocket
//! endpoint. TLS endpoints (`wss://`, e.g. hosted browser providers) are not
//! yet supported; the rationale and the path to lifting that limit are recorded
//! in `DECISIONS.md`.

pub mod actions;
pub mod error;
pub mod fuse;
pub mod observer;

pub use actions::{ActError, Action, act};
pub use error::CdpError;
pub use observer::{CdpObserver, Session, connect};

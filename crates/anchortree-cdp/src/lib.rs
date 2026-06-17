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
//!
//! ## Hosted gateways
//!
//! A hosted gateway does not hand you a bare `wss://` URL: you authenticate to
//! its HTTP control plane first and it returns a self-authenticating WebSocket
//! URL (the credential rides in the URL, never a header — D18). [`gateway`]
//! turns provider credentials into that URL: [`gateway::cloudflare`] builds the
//! Cloudflare Browser Run `?token=` URL with no round-trip, and
//! [`gateway::browserbase`] mints a session over REST and returns its
//! `connectUrl`. Both feed the same [`connect`].
//!
//! ## The hosted connect leg
//!
//! A `wss://` URL from a hosted gateway points at a browser that *already has a
//! page open*, which chromiumoxide 0.9.1 cannot cleanly attach to (D19).
//! [`channel`] solves that with a self-contained flat-attach CDP transport:
//! [`connect_hosted`] flat-attaches to the existing page and returns a
//! [`HostedSession`] whose `observer` runs the exact same fusion pipeline as the
//! local [`connect`]. See `DECISIONS.md` D20 for why a thin channel rather than
//! a dependency bump or a `Page` wrap.

pub mod actions;
pub mod channel;
pub mod error;
pub mod eval;
pub mod frames;
pub mod fuse;
pub mod gateway;
pub mod har;
pub mod observer;
pub mod runner;

pub use actions::{ActError, Action, act, act_mark};
pub use channel::{ChildSession, HostedSession, RawCdpSession, connect_hosted};
pub use error::{CdpError, GatewayError};
pub use eval::{
    EvalError, EvalResult, EvaluatorResult, eval_tasks_args, eval_tasks_command, run_eval_tasks,
    task_output_dir,
};
pub use frames::{
    DomNode, FrameNode, child_frame_keys, dom_frame_keys, frame_keys, map_backends_to_frames,
    same_origin_frame_ids,
};
pub use gateway::{AcquiredSession, browserbase, cloudflare};
pub use har::{
    Har, HarCache, HarContent, HarCookie, HarCreator, HarEntry, HarHeader, HarLog, HarQuery,
    HarRecorder, HarRequest, HarResponse, HarTimings,
};
pub use observer::{CdpObserver, Session, connect, is_tls_endpoint};
pub use runner::{AgentResponse, NetworkCapture, TaskStatus, TaskType, write_task_output};

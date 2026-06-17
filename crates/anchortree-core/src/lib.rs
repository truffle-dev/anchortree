//! `anchortree-core`: the durable-identity and diff engine at the heart of
//! anchortree, an agent-first browser interface.
//!
//! This crate is deliberately browser-free. It operates on [`ObservedNode`]s,
//! which a CDP accessibility/DOM pass produces, so the identity logic is fully
//! unit-testable without driving Chrome. The CDP plumbing lives in a sibling
//! crate (`anchortree-cdp`, added in a later phase).
//!
//! The thesis: an agent's non-determinism in a browser is an *identity*
//! problem, not a rendering problem. A logical element (the "Sign in" button)
//! should keep one durable handle across the agent's own clicks and a
//! framework re-render that swaps the underlying DOM node. [`IdentityMap`]
//! delivers exactly that.

pub mod budget;
pub mod diff;
pub mod fingerprint;
pub mod identity;
pub mod observation;
pub mod role;
pub mod source;

pub use budget::{BASELINE_BUDGET, DIFF_BUDGET, estimated_tokens};
pub use diff::{Diff, ElementChange};
pub use fingerprint::{Bbox, Fingerprint, REBIND_THRESHOLD};
pub use identity::{BackendNodeId, Binding, Eid, ElementState, IdentityMap, ObservedNode};
pub use observation::{Mark, Observation};
pub use role::Role;
pub use source::ObservationSource;

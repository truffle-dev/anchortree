//! Event-sourced observation diffs.
//!
//! An agent driving a page does not need the whole accessibility tree on every
//! turn. It needs the *delta*: what appeared, what vanished, what changed text
//! or state, and which logical elements were re-bound to fresh DOM nodes. A
//! [`Diff`] is what [`IdentityMap::observe`](crate::IdentityMap::observe)
//! returns after each CDP pass, and it is the token-cheap payload an agent
//! reads turn to turn.

use crate::identity::Eid;

/// A single element whose observable content or state changed between two
/// observations, while keeping the same logical identity.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementChange {
    /// The durable logical id that did not change.
    pub eid: Eid,
    /// The new text / accessible-name of the element.
    pub text: String,
}

/// The delta between two consecutive observations of a page.
///
/// `rebound` is the primitive that distinguishes anchortree from a naive
/// snapshot differ: an element that got a brand-new DOM node (new
/// `backendNodeId`) but the same fingerprint is reported here, *not* as a
/// `removed` + `added` pair. The agent's handle survives the re-render.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diff {
    /// Logical elements observed for the first time.
    pub added: Vec<Eid>,
    /// Logical elements that are gone from the page.
    pub removed: Vec<Eid>,
    /// Elements whose text/state changed but identity held.
    pub changed: Vec<ElementChange>,
    /// Elements re-bound to a new underlying DOM node, identity preserved.
    pub rebound: Vec<Eid>,
}

impl Diff {
    /// Whether nothing observable changed since the previous observation.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.changed.is_empty()
            && self.rebound.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_diff_is_empty() {
        assert!(Diff::default().is_empty());
    }

    #[test]
    fn any_change_is_non_empty() {
        let d = Diff {
            rebound: vec![Eid("btn-1".into())],
            ..Default::default()
        };
        assert!(!d.is_empty());
    }
}

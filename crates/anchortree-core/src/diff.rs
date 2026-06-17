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

    /// Render the diff as the compact, line-oriented text an agent actually
    /// reads each turn. One element per line, prefixed by a single sigil so the
    /// kind is obvious at a glance:
    ///
    /// ```text
    /// + btn-sign-in        (added: a logical element seen for the first time)
    /// - banner-promo       (removed: gone from the page)
    /// * btn-submit         (rebound: same handle, fresh DOM node after a re-render)
    /// ~ st-clock: 12:42     (changed: same handle, new text/state)
    /// ```
    ///
    /// This is deliberately lean — an eid like `btn-sign-in` already encodes
    /// role and name, so the inventory needs no second column. Richer state
    /// (value, checked, disabled) stays queryable on demand via
    /// [`IdentityMap::binding`](crate::IdentityMap::binding); paying for it on
    /// every line would defeat the token-cheap point. This is the exact string
    /// the [`budget`](crate::budget) module measures.
    ///
    /// Sections render in a fixed order (added, removed, rebound, changed) so
    /// the output is deterministic. `changed` text is whitespace-collapsed to a
    /// single line; it is not truncated, so an oversized payload is visible to
    /// the budget guardrail rather than hidden by it.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for eid in &self.added {
            out.push_str("+ ");
            out.push_str(&eid.0);
            out.push('\n');
        }
        for eid in &self.removed {
            out.push_str("- ");
            out.push_str(&eid.0);
            out.push('\n');
        }
        for eid in &self.rebound {
            out.push_str("* ");
            out.push_str(&eid.0);
            out.push('\n');
        }
        for ch in &self.changed {
            out.push_str("~ ");
            out.push_str(&ch.eid.0);
            out.push_str(": ");
            out.push_str(&ch.text.split_whitespace().collect::<Vec<_>>().join(" "));
            out.push('\n');
        }
        out
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

    #[test]
    fn render_uses_sigils_in_fixed_section_order() {
        let d = Diff {
            added: vec![Eid("btn-sign-in".into())],
            removed: vec![Eid("banner-promo".into())],
            rebound: vec![Eid("btn-submit".into())],
            changed: vec![ElementChange {
                eid: Eid("st-clock".into()),
                text: "  12:42\n  PM ".into(),
            }],
        };
        assert_eq!(
            d.render(),
            "+ btn-sign-in\n- banner-promo\n* btn-submit\n~ st-clock: 12:42 PM\n"
        );
    }

    #[test]
    fn empty_diff_renders_empty() {
        assert_eq!(Diff::default().render(), "");
    }
}

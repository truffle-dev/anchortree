//! Transient marks and the bundled observation result.
//!
//! Most interactive elements get a durable [`Eid`](crate::Eid): a handle that
//! survives clicks and re-renders. But some elements `fuse` decides are worth
//! surfacing have no anchor the engine can promise to keep — an unlabeled icon
//! button with no stable attribute, a generic clickable with no accessible
//! name. Minting an eid for one of those is a lie: the next observation would
//! churn it into a *different* eid, because there is nothing stable to rebind
//! to.
//!
//! For those, the engine emits a [`Mark`]: a **transient, single-turn** handle
//! the agent can act on *this turn* and must not remember. This is the textual
//! form of "set-of-marks" prompting — a numbered handle list — but text, not a
//! screenshot overlay, which is an order of magnitude cheaper in tokens (see
//! decision D13). The visual/VLM form is a separate, opt-in escalation reserved
//! for the genuinely DOM-less case.
//!
//! [`IdentityMap::observe`](crate::IdentityMap::observe) returns both at once,
//! bundled in an [`Observation`]: the durable [`Diff`] and the turn's marks.

use crate::diff::Diff;
use crate::fingerprint::Bbox;
use crate::identity::BackendNodeId;
use crate::role::Role;

/// The maximum length of a mark's [`label_snippet`](Mark::label_snippet),
/// before an ellipsis. Long enough to disambiguate, short enough to stay cheap.
const SNIPPET_MAX: usize = 40;

/// A transient, single-turn handle to an interactive element the engine could
/// not give a durable [`Eid`](crate::Eid).
///
/// A mark is valid only for the [`Observation`] that produced it. Its
/// [`index`](Mark::index) is **positional and recomputed every observation** —
/// that is the whole contract that separates a mark from an eid. An agent may
/// act on a mark in the same turn it was observed; it must not store a mark and
/// reuse it next turn. If the page re-renders in between, the captured
/// [`backend_node_id`](Mark::backend_node_id) goes stale and an action against
/// it fails loudly, which is the correct single-turn behaviour, not a bug.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    /// Positional index within this observation's mark list, recomputed every
    /// pass. Rendered as `m{index}` (see [`id`](Mark::id)) so it is never
    /// confused with a durable eid in a log or an agent prompt.
    pub index: usize,
    /// The CDP `backendNodeId` the mark resolves to. Document-lifetime stable
    /// while the node lives — but a mark is not tracked, so this is captured
    /// only for the current turn.
    pub backend_node_id: BackendNodeId,
    /// The element's accessibility role, so the agent knows what kind of thing
    /// it is even without a name.
    pub role: Role,
    /// A short human-readable hint (trimmed, truncated text or a role fallback)
    /// to help the agent pick the right mark when several are present.
    pub label_snippet: String,
    /// The element's bounding box, the spatial cue an agent uses to relate the
    /// mark to the rest of the page.
    pub geometry: Bbox,
}

impl Mark {
    /// The namespaced display id, e.g. `m3`. Distinct from the `eid` namespace
    /// so a one-turn mark is never mistaken for a durable handle.
    pub fn id(&self) -> String {
        format!("m{}", self.index)
    }

    /// Build a mark for `index` from the raw observation fields, deriving a
    /// compact label snippet from `text` (falling back to the role name when the
    /// element has no text — which is the usual reason it became a mark).
    pub(crate) fn from_parts(
        index: usize,
        backend_node_id: BackendNodeId,
        role: Role,
        text: &str,
        geometry: Bbox,
    ) -> Self {
        Mark {
            index,
            backend_node_id,
            label_snippet: snippet(text, &role),
            role,
            geometry,
        }
    }
}

/// Compact a node's text into a single-line, length-capped snippet. Falls back
/// to the role's display name when the element has no usable text.
fn snippet(text: &str, role: &Role) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return format!("<{}>", role.prefix());
    }
    if collapsed.chars().count() <= SNIPPET_MAX {
        return collapsed;
    }
    let head: String = collapsed.chars().take(SNIPPET_MAX).collect();
    format!("{}…", head.trim_end())
}

/// The full result of one [`IdentityMap::observe`](crate::IdentityMap::observe)
/// pass: the durable [`Diff`] and this turn's transient [`Mark`]s.
///
/// An agent reads `diff` for everything it can name and remember, and `marks`
/// for the handful of unanchorable elements it may act on this turn only.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observation {
    /// The durable delta: added / removed / changed / rebound logical elements.
    pub diff: Diff,
    /// This turn's transient marks, in document order. Empty on most pages.
    pub marks: Vec<Mark>,
}

impl Observation {
    /// Look up a mark by its positional index within this observation.
    pub fn mark(&self, index: usize) -> Option<&Mark> {
        self.marks.iter().find(|m| m.index == index)
    }

    /// Whether nothing observable changed and there are no marks this turn.
    pub fn is_empty(&self) -> bool {
        self.diff.is_empty() && self.marks.is_empty()
    }

    /// Render the whole observation as the compact text an agent reads this
    /// turn: the durable [`Diff`] lines (see [`Diff::render`]) followed by one
    /// line per transient [`Mark`]:
    ///
    /// ```text
    /// m0 btn "Add to cart" @312,48
    /// m1 btn <btn> @344,48
    /// ```
    ///
    /// A mark line carries the spatial cue (`@x,y`, rounded to whole pixels)
    /// that is the whole reason a mark exists — an unanchorable element the
    /// agent locates by position this turn only. This is the exact string the
    /// [`budget`](crate::budget) module measures for the baseline cap.
    pub fn render(&self) -> String {
        let mut out = self.diff.render();
        for mark in &self.marks {
            out.push_str(&mark.id());
            out.push(' ');
            out.push_str(mark.role.prefix());
            out.push_str(" \"");
            out.push_str(&mark.label_snippet);
            out.push_str("\" @");
            out.push_str(&(mark.geometry.x.round() as i64).to_string());
            out.push(',');
            out.push_str(&(mark.geometry.y.round() as i64).to_string());
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox() -> Bbox {
        Bbox {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
        }
    }

    #[test]
    fn id_is_m_namespaced() {
        let m = Mark::from_parts(3, 99, Role::Button, "Go", bbox());
        assert_eq!(m.id(), "m3");
    }

    #[test]
    fn snippet_collapses_whitespace() {
        let m = Mark::from_parts(0, 1, Role::Button, "  Add\n  to   cart ", bbox());
        assert_eq!(m.label_snippet, "Add to cart");
    }

    #[test]
    fn snippet_falls_back_to_role_when_textless() {
        // The usual mark case: an icon button with no accessible text.
        let m = Mark::from_parts(0, 1, Role::Button, "   ", bbox());
        assert_eq!(m.label_snippet, "<btn>");
    }

    #[test]
    fn snippet_truncates_long_text_with_ellipsis() {
        let long = "x".repeat(80);
        let m = Mark::from_parts(0, 1, Role::Button, &long, bbox());
        assert!(m.label_snippet.ends_with('…'));
        // 40 chars of content + the ellipsis.
        assert_eq!(m.label_snippet.chars().count(), SNIPPET_MAX + 1);
    }

    #[test]
    fn lookup_by_index_finds_the_mark() {
        let obs = Observation {
            diff: Diff::default(),
            marks: vec![
                Mark::from_parts(0, 10, Role::Button, "a", bbox()),
                Mark::from_parts(1, 11, Role::Link, "b", bbox()),
            ],
        };
        assert_eq!(obs.mark(1).unwrap().backend_node_id, 11);
        assert!(obs.mark(2).is_none());
    }

    #[test]
    fn empty_observation_is_empty() {
        assert!(Observation::default().is_empty());
        let with_mark = Observation {
            diff: Diff::default(),
            marks: vec![Mark::from_parts(0, 1, Role::Button, "x", bbox())],
        };
        assert!(!with_mark.is_empty());
    }

    #[test]
    fn render_appends_mark_lines_after_the_diff() {
        let geom = Bbox {
            x: 311.6,
            y: 48.2,
            w: 16.0,
            h: 16.0,
        };
        let obs = Observation {
            diff: Diff {
                added: vec![crate::Eid("btn-sign-in".into())],
                ..Default::default()
            },
            marks: vec![
                Mark::from_parts(0, 10, Role::Button, "Add to cart", geom),
                Mark::from_parts(1, 11, Role::Button, "   ", geom),
            ],
        };
        assert_eq!(
            obs.render(),
            "+ btn-sign-in\nm0 btn \"Add to cart\" @312,48\nm1 btn \"<btn>\" @312,48\n"
        );
    }
}

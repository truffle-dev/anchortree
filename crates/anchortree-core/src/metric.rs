//! Phase 3.3c: the re-grounding-calls metric — the thesis headline.
//!
//! anchortree's whole claim is that durable element identity lets an agent skip
//! the LLM call a naive agent pays to *re-ground* an element after a re-render.
//! This module turns that claim into one defensible number: a per-task tally of
//! the durable rebinds the engine delivered (every one an LLM re-ground avoided)
//! and the LLM re-ground calls the engine itself made (zero, by construction).
//!
//! ## Where the signal comes from — and what does NOT count
//!
//! [`IdentityMap::observe`](crate::IdentityMap::observe) returns a [`Diff`] whose
//! three element buckets map to the engine's three identity paths
//! (`identity.rs`):
//!
//! - [`Diff::rebound`] — Path 2: a known `eid` re-bound onto a *fresh* DOM node
//!   (its `backendNodeId` changed) after a re-render. **This is the win, and the
//!   only bucket this metric counts.** Each entry is one element that survived a
//!   re-render with the same logical handle and zero model call.
//! - [`Diff::added`] — Path 3 (`mint`): a genuinely new element. A naive agent
//!   grounds it once too, so it is a *first*-ground, not a re-ground-avoided.
//!   Counting it would inflate the headline. **Not counted.**
//! - [`Diff::changed`] — Path 1: same `backendNodeId`, a cheap attribute/text
//!   update with no re-render on either side. **Not counted.**
//!
//! These guardrails are the difference between an honest number and a vanity
//! one; they are pinned by DECISIONS D28 and enforced by the tests below.
//!
//! ## "Zero LLM, by construction" is structural, not a runtime accident
//!
//! [`RegroundLedger`] has no API that could ever record a model call: the only
//! mutator is [`record`](RegroundLedger::record), which takes a [`Diff`] and
//! reads only its bucket lengths. The engine's observe path takes no model
//! client. So [`llm_reground_calls`](RegroundLedger::llm_reground_calls) is `0`
//! because the type *cannot represent* a re-ground — not because a counter
//! happened to stay at zero. The tests fold diffs full of `added`/`changed`
//! churn and assert the LLM count never moves.

use crate::diff::Diff;

/// A per-task tally of durable rebinds delivered vs. LLM re-ground calls made.
///
/// Fold each observation's [`Diff`] in with [`record`](Self::record) over the
/// course of a task; read the headline out at the end with
/// [`rebinds_zero_llm`](Self::rebinds_zero_llm). Browser-free and cheap — it
/// only ever reads three lengths off the diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegroundLedger {
    rebinds_zero_llm: usize,
    observes: usize,
}

impl RegroundLedger {
    /// A fresh ledger with everything at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one observation's diff into the tally.
    ///
    /// Adds `diff.rebound.len()` to the rebind headline and counts the observe
    /// pass. Deliberately ignores `diff.added` (Path 3 mint = a first-ground)
    /// and `diff.changed` (Path 1 = a cheap attr update) — see the module docs
    /// and DECISIONS D28. There is no path here that records a model call, which
    /// is what keeps [`llm_reground_calls`](Self::llm_reground_calls) zero.
    pub fn record(&mut self, diff: &Diff) {
        self.rebinds_zero_llm += diff.rebound.len();
        self.observes += 1;
    }

    /// The headline: total durable rebinds across the task. Each one is an
    /// element that survived a re-render with its `eid` intact and cost **zero**
    /// LLM re-ground calls — the count of model calls a re-grounding peer pays
    /// that anchortree does not.
    pub fn rebinds_zero_llm(&self) -> usize {
        self.rebinds_zero_llm
    }

    /// LLM re-ground calls the engine made: `0`, by construction. The observe
    /// path takes no model client and this ledger has no mutator that could
    /// record one, so the value is structurally zero rather than a runtime
    /// accident (see the module docs).
    pub fn llm_reground_calls(&self) -> usize {
        0
    }

    /// How many observation passes were folded in.
    pub fn observes(&self) -> usize {
        self.observes
    }

    /// Whether any durable rebind was recorded. A task with no re-render churns
    /// no DOM nodes and so delivers no rebinds — a true `false` here is not a
    /// failure, just a task that never re-rendered.
    pub fn has_rebinds(&self) -> bool {
        self.rebinds_zero_llm > 0
    }

    /// Render the standalone headline line for a log or report, e.g.
    /// `3 durable rebinds at 0 LLM re-grounds (over 2 observes)`. The LLM count
    /// is always `0` and is stated explicitly so the claim is on the record, not
    /// implied.
    pub fn render(&self) -> String {
        format!(
            "{} durable rebinds at {} LLM re-grounds (over {} observes)",
            self.rebinds_zero_llm,
            self.llm_reground_calls(),
            self.observes,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::ElementChange;
    use crate::identity::Eid;

    fn eid(s: &str) -> Eid {
        Eid(s.into())
    }

    #[test]
    fn fresh_ledger_is_all_zero() {
        let l = RegroundLedger::new();
        assert_eq!(l.rebinds_zero_llm(), 0);
        assert_eq!(l.llm_reground_calls(), 0);
        assert_eq!(l.observes(), 0);
        assert!(!l.has_rebinds());
    }

    #[test]
    fn counts_only_rebound_across_passes() {
        let mut l = RegroundLedger::new();
        // First paint: three first-grounds, nothing rebound.
        l.record(&Diff {
            added: vec![eid("a"), eid("b"), eid("c")],
            ..Default::default()
        });
        // Re-render: all three rebind onto fresh DOM nodes.
        l.record(&Diff {
            rebound: vec![eid("a"), eid("b"), eid("c")],
            ..Default::default()
        });
        assert_eq!(l.rebinds_zero_llm(), 3);
        assert_eq!(l.observes(), 2);
        assert!(l.has_rebinds());
    }

    #[test]
    fn added_and_changed_never_inflate_the_headline() {
        // The honesty guardrail (D28): a diff packed with adds, changes, and
        // removals but ZERO rebounds must contribute nothing to the headline.
        let mut l = RegroundLedger::new();
        l.record(&Diff {
            added: vec![eid("new-1"), eid("new-2")],
            removed: vec![eid("gone-1")],
            changed: vec![
                ElementChange {
                    eid: eid("clock"),
                    text: "12:42".into(),
                },
                ElementChange {
                    eid: eid("badge"),
                    text: "3".into(),
                },
            ],
            rebound: vec![],
        });
        assert_eq!(
            l.rebinds_zero_llm(),
            0,
            "added/changed/removed must not count as re-grounds avoided"
        );
        assert_eq!(l.observes(), 1);
    }

    #[test]
    fn llm_reground_count_is_zero_under_any_diff_churn() {
        // The structural assertion: no sequence of diffs, however busy, can move
        // the LLM re-ground count off zero — the ledger has no API to record one.
        let mut l = RegroundLedger::new();
        for _ in 0..50 {
            l.record(&Diff {
                added: vec![eid("x")],
                removed: vec![eid("y")],
                changed: vec![ElementChange {
                    eid: eid("z"),
                    text: "t".into(),
                }],
                rebound: vec![eid("r")],
            });
        }
        assert_eq!(l.rebinds_zero_llm(), 50);
        assert_eq!(l.llm_reground_calls(), 0);
        assert_eq!(l.observes(), 50);
    }

    #[test]
    fn render_states_the_zero_llm_claim_explicitly() {
        let mut l = RegroundLedger::new();
        l.record(&Diff {
            added: vec![eid("a")],
            ..Default::default()
        });
        l.record(&Diff {
            rebound: vec![eid("a"), eid("b"), eid("c")],
            ..Default::default()
        });
        assert_eq!(
            l.render(),
            "3 durable rebinds at 0 LLM re-grounds (over 2 observes)"
        );
    }
}

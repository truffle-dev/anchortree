//! Token-budget guardrails.
//!
//! The thesis has two halves. The first is durable identity: an agent keeps one
//! handle to a logical element across re-renders. The second — the half this
//! module enforces — is that the payload carrying those handles must be *cheap
//! enough to send every turn*. A durable id is worthless if observing the page
//! costs 30K tokens, because then the agent cannot afford to look. Peers hit
//! exactly this wall: uncompressed accessibility dumps run 15K–35K tokens and
//! drive real 25K–200K context-window failures (Skyvern#1712,
//! playwright-mcp#1216).
//!
//! So anchortree holds two caps:
//!
//! - a **baseline** [`Observation`] (the first turn, where every element is
//!   `added`) must fit [`BASELINE_BUDGET`] (5,000 tokens), and
//! - a **per-turn** [`Diff`] must fit [`DIFF_BUDGET`] (800 tokens).
//!
//! The estimate is tokenizer-free on purpose (decision **D14**). Pulling a BPE
//! tokenizer in for a guardrail would be a heavy dependency for an
//! order-of-magnitude question. A fixed divisor over the serialized string is
//! reliable here: byte-size and token-size correlate at r = 0.9994 on DOM
//! content (arXiv 2508.04412), and chars/N is established practice (LangChain's
//! `count_tokens_approximately`).
//!
//! The divisor is **3.5, not the usual 4**. chars/4 is calibrated to English
//! prose; the anchortree payload is markup-dense (`role` prefixes, short
//! hyphenated refs, sigils, coordinates), where BPE fragments brackets and
//! attribute-like tokens and the real ratio runs 2.5–3.8 chars/token. A
//! guardrail must fail safe by *over*-estimating, so 3.5 sits conservatively
//! inside the measured band.

use crate::diff::Diff;
use crate::observation::Observation;

/// The cap, in estimated tokens, for a baseline [`Observation`] — the first
/// turn on a page, where every tracked element is reported as `added`. Roomy
/// versus a compact snapshot (peers land ~200–1,000 tokens) yet well under the
/// 15K–35K of an uncompressed accessibility dump.
pub const BASELINE_BUDGET: usize = 5_000;

/// The cap, in estimated tokens, for a per-turn [`Diff`]. Tight on purpose: the
/// whole point of an event-sourced diff is that polling the page every turn
/// stays nearly free.
pub const DIFF_BUDGET: usize = 800;

/// Estimate the token cost of a serialized string without a tokenizer.
///
/// `ceil(chars / 3.5)`, written in integer math as `ceil(chars * 2 / 7)` so it
/// is exact and branch-free. Deliberately an *over*-estimate for markup-dense
/// payloads (decision D14): a guardrail that under-counts is worse than useless.
///
/// ```
/// use anchortree_core::budget::estimated_tokens;
/// assert_eq!(estimated_tokens(""), 0);
/// assert_eq!(estimated_tokens("1234567"), 2); // 7 chars / 3.5 = 2
/// ```
pub fn estimated_tokens(s: &str) -> usize {
    (s.chars().count() * 2).div_ceil(7)
}

/// Estimated token cost of a rendered [`Diff`] (see [`Diff::render`]).
pub fn diff_tokens(diff: &Diff) -> usize {
    estimated_tokens(&diff.render())
}

/// Estimated token cost of a rendered [`Observation`] (see
/// [`Observation::render`]): the durable diff plus this turn's marks.
pub fn observation_tokens(obs: &Observation) -> usize {
    estimated_tokens(&obs.render())
}

/// Whether a baseline observation fits [`BASELINE_BUDGET`].
pub fn observation_within_budget(obs: &Observation) -> bool {
    observation_tokens(obs) <= BASELINE_BUDGET
}

/// Whether a per-turn diff fits [`DIFF_BUDGET`].
pub fn diff_within_budget(diff: &Diff) -> bool {
    diff_tokens(diff) <= DIFF_BUDGET
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::ElementChange;
    use crate::fingerprint::Bbox;
    use crate::identity::Eid;
    use crate::observation::Mark;
    use crate::role::Role;

    #[test]
    fn estimator_is_ceil_of_chars_over_3_5() {
        assert_eq!(estimated_tokens(""), 0);
        // Exact multiples of 3.5 chars -> exact token counts.
        assert_eq!(estimated_tokens("1234567"), 2); // 7 / 3.5
        assert_eq!(estimated_tokens(&"x".repeat(350)), 100); // 350 / 3.5
        // Always rounds up: 1 char is a (partial) token, never zero.
        assert_eq!(estimated_tokens("x"), 1);
        assert_eq!(estimated_tokens("xxxx"), 2); // ceil(4 / 3.5) = ceil(1.14)
        assert_eq!(estimated_tokens("xxxxxxxx"), 3); // ceil(8 / 3.5) = ceil(2.28)
    }

    #[test]
    fn estimator_counts_unicode_scalars_not_bytes() {
        // A 4-byte emoji is one char here; using byte length would over-charge
        // by 4x and make the guardrail jumpy on non-ASCII labels.
        assert_eq!("🎯".len(), 4);
        assert_eq!(estimated_tokens("🎯"), 1);
    }

    /// A plausible inventory for a real app page: a nav rail, a header, a
    /// project-creation form, a small table with duplicate-disambiguated row
    /// actions, some status/headings, and a footer. ~40 logical elements — the
    /// kind of page an agent actually drives.
    fn realistic_added() -> Vec<Eid> {
        [
            // nav rail
            "lnk-home",
            "lnk-dashboard",
            "lnk-projects",
            "lnk-team",
            "lnk-settings",
            "lnk-billing",
            "lnk-docs",
            "lnk-support",
            // header
            "srch-search-projects",
            "btn-notifications",
            "btn-account-menu",
            "btn-new-project",
            // form
            "inp-project-name",
            "inp-description",
            "sel-visibility",
            "chk-enable-ci",
            "chk-auto-deploy",
            "rdo-plan-free",
            "rdo-plan-pro",
            "rdo-plan-team",
            "inp-repository-url",
            "sel-default-branch",
            "btn-create-project",
            "btn-cancel",
            // table with row actions (engine disambiguates duplicates)
            "btn-edit",
            "btn-edit-1",
            "btn-edit-2",
            "btn-delete",
            "btn-delete-1",
            "btn-delete-2",
            "lnk-row-detail",
            "lnk-row-detail-1",
            // status + structure
            "st-build-status",
            "st-deploy-count",
            "hd-overview",
            "hd-recent-activity",
            "rg-activity-feed",
            // footer
            "lnk-privacy",
            "lnk-terms",
            "btn-cookie-prefs",
        ]
        .iter()
        .map(|s| Eid((*s).to_string()))
        .collect()
    }

    #[test]
    fn baseline_observation_lands_well_under_budget() {
        let added = realistic_added();
        let n = added.len();
        // Two unanchorable icon buttons surface as marks alongside the durable
        // inventory — a toolbar with no labels.
        let icon = Bbox {
            x: 1180.0,
            y: 64.0,
            w: 16.0,
            h: 16.0,
        };
        let obs = Observation {
            diff: Diff {
                added,
                ..Default::default()
            },
            marks: vec![
                Mark::from_parts(0, 901, Role::Button, "", icon),
                Mark::from_parts(1, 902, Role::Button, "", icon),
            ],
        };

        let tokens = observation_tokens(&obs);
        println!("baseline: {n} elements + 2 marks -> {tokens} est. tokens");

        // The guardrail.
        assert!(
            observation_within_budget(&obs),
            "baseline {tokens} tokens exceeds {BASELINE_BUDGET}"
        );
        // The thesis margin: a full ~40-element page baseline lands in the same
        // ~200–400 token band as peers' *compact* snapshots, an order of
        // magnitude under the 5K cap and two under a raw AX dump. If this ever
        // creeps past 600 the render grew too chatty — investigate before
        // raising the number.
        assert!(
            tokens < 600,
            "baseline {tokens} tokens is heavier than the design target (<600)"
        );
    }

    #[test]
    fn steady_turn_diff_is_nearly_free() {
        // A typical later turn: two status lines tick, one button rebinds onto a
        // fresh DOM node after a partial re-render, one toast appears.
        let diff = Diff {
            added: vec![Eid("st-toast".into())],
            removed: vec![],
            rebound: vec![Eid("btn-create-project".into())],
            changed: vec![
                ElementChange {
                    eid: Eid("st-build-status".into()),
                    text: "Build passing".into(),
                },
                ElementChange {
                    eid: Eid("st-deploy-count".into()),
                    text: "1,284 deploys".into(),
                },
            ],
        };

        let tokens = diff_tokens(&diff);
        println!("steady-turn diff -> {tokens} est. tokens");

        assert!(
            diff_within_budget(&diff),
            "per-turn diff {tokens} tokens exceeds {DIFF_BUDGET}"
        );
        // A handful of changed lines is tens of tokens, not hundreds.
        assert!(
            tokens < 100,
            "steady-turn diff unexpectedly heavy: {tokens}"
        );
    }

    #[test]
    fn empty_observation_costs_nothing() {
        assert_eq!(observation_tokens(&Observation::default()), 0);
        assert!(observation_within_budget(&Observation::default()));
        assert!(diff_within_budget(&Diff::default()));
    }
}

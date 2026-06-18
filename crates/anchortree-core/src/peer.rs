//! Phase 3.3d: two hermetic peer models — the baseline anchortree is measured
//! against.
//!
//! Phase 3.3c gave the thesis headline in anchortree's own terms: durable
//! rebinds delivered at zero LLM re-grounds ([`RegroundLedger`](crate::RegroundLedger)).
//! A headline needs a *comparison* to mean anything, and the honest comparison
//! is against the two interfaces a real agent would otherwise reach for. This
//! module is those two peers, modelled offline so the benchmark stays
//! HERMETIC (decision **D29**): no live Stagehand, no Node, no OpenAI, no
//! Playwright-MCP server. We replay the *same* captured observe/mutation
//! sequence through both peer models and score them with the engine's own
//! tokenizer ([`budget`](crate::budget)), so every number is apples-to-apples.
//!
//! There are two independent axes, because the two peers fail in two different
//! ways:
//!
//! ## Axis 1 — token volume (the Playwright-MCP model)
//!
//! A Playwright-MCP-style agent re-sends the **full accessibility snapshot**
//! every turn; it has no event-sourced delta. [`playwright_snapshot`] renders
//! that snapshot in the tool's own line shape (`- button "Sign in" [ref=e13]`)
//! and [`snapshot_tokens`] prices it with the *same* `ceil(chars / 3.5)` ruler
//! anchortree prices its diff with. So the per-turn comparison is
//! [`snapshot_tokens`] (peer resends everything) versus
//! [`diff_tokens`](crate::budget::diff_tokens) (anchortree sends only the
//! delta). On turn one they are close — both carry the whole inventory once —
//! and the gap compounds every turn after, which is exactly the cost the diff
//! is designed to erase.
//!
//! ## Axis 2 — LLM re-grounds (the Stagehand model)
//!
//! A Stagehand-style agent caches an **absolute selector** (an XPath) for each
//! element it acts on, and re-uses it on later turns; when the cached selector
//! no longer resolves to the same element it pays an LLM `page.act` call to
//! re-find it (a *self-heal*). [`StagehandCache`] models exactly this over a
//! [`DomPositions`] map.
//!
//! The load-bearing subtlety, pinned by D29, is that **this self-heal count is
//! not anchortree's rebind count** — neither an upper nor a lower bound:
//!
//! - An absolute XPath can *survive* a `backendNodeId` change. A framework that
//!   replaces a node in place leaves the element at the same DOM position, so
//!   the XPath still resolves — zero self-heal — while anchortree's engine
//!   counts a Path-2 rebind. (Rebind without self-heal.)
//! - An absolute XPath can *break* with no `backendNodeId` change. Insert a
//!   sibling above the element and every positional index below shifts, so the
//!   cached XPath now points at the wrong node — one self-heal — while
//!   anchortree's engine took the cheap Path-1 soft match and counted no
//!   rebind. (Self-heal without rebind.)
//!
//! So [`rebinds_zero_llm`](crate::RegroundLedger::rebinds_zero_llm) is genuinely
//! a different measurement from [`StagehandCache::self_heals`], and modelling
//! the XPath resolver directly — rather than reusing the rebind tally as a
//! proxy — is the only honest way to report the LLM-call axis. The integration
//! test drives both directions of the divergence against the real engine.
//!
//! [`BaselineReport`] folds a whole task's turns together and renders the two
//! axes side by side.

use std::collections::BTreeMap;

use crate::budget::{diff_tokens, estimated_tokens};
use crate::diff::Diff;
use crate::identity::ObservedNode;
use crate::role::Role;

/// The canonical ARIA role string for a [`Role`] — the inverse of
/// [`Role::from_aria`] for the one-to-one cases, choosing the canonical spelling
/// for the many-to-one ones (`menuitem`, `status`). [`Role::Other`] round-trips
/// its preserved string. Used to render the Playwright-MCP snapshot in the
/// tool's own vocabulary so the token comparison is fair.
fn aria_role(role: &Role) -> &str {
    match role {
        Role::Button => "button",
        Role::Link => "link",
        Role::Textbox => "textbox",
        Role::Searchbox => "searchbox",
        Role::Combobox => "combobox",
        Role::Checkbox => "checkbox",
        Role::Radio => "radio",
        Role::Switch => "switch",
        Role::Slider => "slider",
        Role::Menuitem => "menuitem",
        Role::Tab => "tab",
        Role::Option => "option",
        Role::Heading => "heading",
        Role::Region => "region",
        Role::Status => "status",
        Role::Other(s) => s,
    }
}

/// Render an observation as a Playwright-MCP-style accessibility snapshot: one
/// line per node, in document order, shaped like the real tool's output so the
/// token cost is a fair comparison.
///
/// ```text
/// - textbox "Email" [ref=e11]
/// - textbox "Password" [ref=e12]
/// - button "Sign in" [ref=e13]
/// ```
///
/// The `ref` is the node's `backendNodeId` (the handle a Playwright-MCP agent
/// would address), so a re-render that swaps DOM nodes churns every `ref` on the
/// line — the very thing anchortree's durable eid hides. The string carries a
/// trailing newline per line, matching [`Diff::render`](crate::diff::Diff::render)
/// so the two are priced on identical terms.
pub fn playwright_snapshot(nodes: &[ObservedNode]) -> String {
    let mut out = String::new();
    for node in nodes {
        out.push_str("- ");
        out.push_str(aria_role(&node.fingerprint.role));
        out.push_str(" \"");
        out.push_str(&node.fingerprint.accessible_name);
        out.push_str("\" [ref=e");
        out.push_str(&node.backend_node_id.to_string());
        out.push_str("]\n");
    }
    out
}

/// Estimated token cost of a full Playwright-MCP snapshot, priced with the same
/// `ceil(chars / 3.5)` ruler anchortree uses for its diff
/// ([`estimated_tokens`](crate::budget::estimated_tokens)). This is the peer's
/// per-turn payload; anchortree's is [`diff_tokens`](crate::budget::diff_tokens).
pub fn snapshot_tokens(nodes: &[ObservedNode]) -> usize {
    estimated_tokens(&playwright_snapshot(nodes))
}

/// The ground-truth bijection between logical elements and their absolute
/// XPaths at one point in time — the page state a Stagehand-style resolver sees
/// when it tries a cached selector.
///
/// Bidirectional on purpose: a resolver needs both "where is element X now"
/// ([`xpath_of`](Self::xpath_of), to re-cache after a heal) and "what element is
/// at this XPath now" ([`logical_at`](Self::logical_at), to decide whether the
/// cached selector still points at the right thing). Keyed with [`BTreeMap`] so
/// iteration and rendering are deterministic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomPositions {
    xpath_of: BTreeMap<String, String>,
    logical_at: BTreeMap<String, String>,
}

impl DomPositions {
    /// An empty position map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that logical element `logical` currently sits at absolute path
    /// `xpath`. Fills both directions of the bijection.
    pub fn place(&mut self, logical: &str, xpath: &str) {
        self.xpath_of.insert(logical.to_string(), xpath.to_string());
        self.logical_at
            .insert(xpath.to_string(), logical.to_string());
    }

    /// The absolute XPath of a logical element in this page state, if present.
    pub fn xpath_of(&self, logical: &str) -> Option<&str> {
        self.xpath_of.get(logical).map(String::as_str)
    }

    /// The logical element currently resolved by an absolute XPath, if any. This
    /// is what a cached Stagehand selector actually hits when re-tried.
    pub fn logical_at(&self, xpath: &str) -> Option<&str> {
        self.logical_at.get(xpath).map(String::as_str)
    }

    /// The absolute-positional view a raw-XPath resolver would cache over one
    /// observation: each *named* element keyed by its accessible name, placed at
    /// its 1-based **document-order** position `/*[k]`.
    ///
    /// This is deliberately the resolver's view, not anchortree's. anchortree's
    /// own [`structural_path`](crate::fingerprint::Fingerprint::structural_path)
    /// is role-scoped and landmark-anchored — it survives a sibling shift on
    /// purpose. A Stagehand-style absolute selector counts *every* preceding
    /// sibling, named or not, so the index `k` runs over the full `nodes` slice
    /// (unnamed nodes still consume a position) while only named elements are
    /// placed (a resolver caches the selector for the element the agent named).
    /// That is exactly why a reorder that anchortree rebinds for free breaks the
    /// cached XPath: the moved element's document index changes.
    ///
    /// Two named elements that share an accessible name collapse to one entry
    /// (the [`DomPositions`] bijection); a real resolver disambiguates with more
    /// of the path, but the fixtures this models use distinct names.
    pub fn from_document_order(nodes: &[ObservedNode]) -> Self {
        let mut positions = Self::new();
        for (i, node) in nodes.iter().enumerate() {
            let name = &node.fingerprint.accessible_name;
            if name.is_empty() {
                continue;
            }
            positions.place(name, &format!("/*[{}]", i + 1));
        }
        positions
    }
}

/// A Stagehand-style cache of absolute selectors, and the count of LLM
/// `page.act` self-heals it was forced to pay.
///
/// The model: when the agent acts on a logical element, [`bind`](Self::bind)
/// caches the element's current XPath (free — the agent already located it this
/// turn). On every later page state, [`reresolve`](Self::reresolve) checks each
/// cached selector; any that no longer resolves to its logical element is a
/// stale selector that costs one LLM call to re-ground, after which the cache is
/// repaired to the element's new XPath. [`self_heals`](Self::self_heals) is the
/// running total of those calls — the peer's analogue of the re-grounds
/// anchortree pays zero of.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StagehandCache {
    cached: BTreeMap<String, String>,
    self_heals: usize,
}

impl StagehandCache {
    /// An empty cache, no elements bound, no self-heals paid.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cache the current absolute XPath of a logical element the agent just
    /// acted on. Free: the agent already located the element this turn, so no
    /// LLM call is charged here. A no-op (silently) if the element is absent
    /// from `positions`, which a real resolver would never hit for an element it
    /// just acted on.
    pub fn bind(&mut self, logical: &str, positions: &DomPositions) {
        if let Some(xpath) = positions.xpath_of(logical) {
            self.cached.insert(logical.to_string(), xpath.to_string());
        }
    }

    /// Re-try every cached selector against a new page state, charging one
    /// self-heal per selector that no longer resolves to its logical element and
    /// repairing the cache to the element's current XPath.
    ///
    /// A selector is stale when the cached XPath now resolves to a *different*
    /// logical element (a sibling shifted into its slot) or to *nothing* (the
    /// path no longer exists). Both are one LLM `page.act` call for a Stagehand
    /// agent. Returns the number of self-heals charged *this* call, so a caller
    /// can assert the per-turn delta.
    pub fn reresolve(&mut self, positions: &DomPositions) -> usize {
        // Collect repairs first; we cannot mutate `cached` while iterating it.
        let mut repairs: Vec<(String, String)> = Vec::new();
        let mut healed = 0;
        for (logical, cached_xpath) in &self.cached {
            if positions.logical_at(cached_xpath) == Some(logical.as_str()) {
                continue; // selector still good — no LLM call.
            }
            healed += 1;
            if let Some(fresh) = positions.xpath_of(logical) {
                repairs.push((logical.clone(), fresh.to_string()));
            }
        }
        for (logical, fresh) in repairs {
            self.cached.insert(logical, fresh);
        }
        self.self_heals += healed;
        healed
    }

    /// Total LLM `page.act` self-heals paid across the task so far.
    pub fn self_heals(&self) -> usize {
        self.self_heals
    }

    /// Number of distinct logical elements currently cached.
    pub fn cached_len(&self) -> usize {
        self.cached.len()
    }
}

/// The frame-tree positional view a Stagehand-style cross-frame handle keys on:
/// each frame-owner's document-order ordinal mapped to its durable discriminator
/// (the `src`-origin / `name` / `title` / `id` label anchortree's
/// [`FrameKey`](crate::FrameKey) carries). This is the frame-tier analogue of
/// [`DomPositions`] — the page state a positional frame resolver sees when it
/// re-tries a cached frame handle one level up the tree.
///
/// Stagehand v3's cross-frame composite is `frame ordinal + backendNodeId`
/// (browserbase.com/blog/taming-iframes-a-stagehand-update): the FRAME half is
/// positional, so inserting a sibling frame-owner ahead of the target shifts its
/// ordinal and the cached handle resolves into the wrong frame. anchortree keys
/// the frame by its discriminator instead, so the same insert leaves its key
/// unchanged (decision D40). Both views are built from the same ordered owner
/// list here so the head-to-head is apples-to-apples.
///
/// Two frame-owners that share a discriminator (identical-`src` ad slots)
/// collapse in the discriminator→ordinal direction to the FIRST occurrence — the
/// same document-order fallback anchortree degrades to for that case (decision
/// D41), and the same place Playwright's [`FrameLocator`] lands with `.nth()`.
///
/// [`FrameLocator`]: https://playwright.dev/docs/api/class-framelocator
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameOrder {
    discriminator_at: BTreeMap<usize, String>,
    first_ordinal_of: BTreeMap<String, usize>,
}

impl FrameOrder {
    /// Build the positional view from the document-order list of frame-owner
    /// discriminators (ordinal `i` → `owners[i]`). The discriminator→ordinal
    /// direction keeps the first occurrence of each label.
    pub fn from_owner_order<S: AsRef<str>>(owners: &[S]) -> Self {
        let mut order = Self::default();
        for (i, owner) in owners.iter().enumerate() {
            let label = owner.as_ref().to_string();
            order.discriminator_at.insert(i, label.clone());
            order.first_ordinal_of.entry(label).or_insert(i);
        }
        order
    }

    /// The discriminator of the frame-owner currently at `ordinal`, if any. This
    /// is what a cached positional frame handle actually hits when re-tried.
    pub fn discriminator_at(&self, ordinal: usize) -> Option<&str> {
        self.discriminator_at.get(&ordinal).map(String::as_str)
    }

    /// The (first) document-order ordinal a discriminator sits at, if present.
    /// This is the durable lookup anchortree's content-addressed `FrameKey`
    /// performs: it finds the frame by label regardless of how many siblings
    /// were inserted ahead of it.
    pub fn ordinal_of(&self, discriminator: &str) -> Option<usize> {
        self.first_ordinal_of.get(discriminator).copied()
    }

    /// Number of frame-owners in this layout.
    pub fn len(&self) -> usize {
        self.discriminator_at.len()
    }

    /// Whether the layout has no frame-owners.
    pub fn is_empty(&self) -> bool {
        self.discriminator_at.is_empty()
    }
}

/// A Stagehand-style cache of cross-frame handles keyed by frame ORDINAL, and
/// the count of LLM re-grounds it was forced to pay when a cached frame handle
/// went stale. The frame-tier twin of [`StagehandCache`].
///
/// The model: when the agent acts inside a frame, [`bind`](Self::bind) caches
/// that frame's current ordinal (free — the agent already located it this turn).
/// On every later layout, [`reresolve`](Self::reresolve) checks each cached
/// handle; any whose ordinal now holds a DIFFERENT frame discriminator (a
/// sibling owner shifted into its slot) is stale and costs one LLM re-ground,
/// after which the cache is repaired to the frame's new ordinal.
/// [`regrounds`](Self::regrounds) is the running total — the frame-tier analogue
/// of the re-grounds anchortree's discriminator key pays zero of.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameOrdinalCache {
    cached: BTreeMap<String, usize>,
    regrounds: usize,
}

impl FrameOrdinalCache {
    /// An empty cache, no frames bound, no re-grounds paid.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cache the current ordinal of the frame identified by `discriminator` (the
    /// frame the agent just acted inside). Free: the agent already located it
    /// this turn. A silent no-op if the frame is absent from `order`.
    pub fn bind(&mut self, discriminator: &str, order: &FrameOrder) {
        if let Some(ordinal) = order.ordinal_of(discriminator) {
            self.cached.insert(discriminator.to_string(), ordinal);
        }
    }

    /// Re-try every cached frame handle against a new layout, charging one
    /// re-ground per handle whose cached ordinal no longer holds its frame's
    /// discriminator and repairing the cache to the frame's current ordinal.
    /// Returns the re-grounds charged *this* call.
    pub fn reresolve(&mut self, order: &FrameOrder) -> usize {
        let mut repairs: Vec<(String, usize)> = Vec::new();
        let mut reground = 0;
        for (discriminator, cached_ordinal) in &self.cached {
            if order.discriminator_at(*cached_ordinal) == Some(discriminator.as_str()) {
                continue; // handle still good — no LLM call.
            }
            reground += 1;
            if let Some(fresh) = order.ordinal_of(discriminator) {
                repairs.push((discriminator.clone(), fresh));
            }
        }
        for (discriminator, fresh) in repairs {
            self.cached.insert(discriminator, fresh);
        }
        self.regrounds += reground;
        reground
    }

    /// Total LLM re-grounds paid across the task so far.
    pub fn regrounds(&self) -> usize {
        self.regrounds
    }

    /// Number of distinct frames currently cached.
    pub fn cached_len(&self) -> usize {
        self.cached.len()
    }
}

/// A whole task's two-axis comparison: anchortree versus the two peer models,
/// folded turn by turn.
///
/// Fold each turn in with [`record_turn`](Self::record_turn) (the token axis,
/// computed from the snapshot and the diff) and, once the Stagehand replay is
/// done, set the LLM axis with [`set_peer_self_heals`](Self::set_peer_self_heals).
/// Read the headline out with [`render`](Self::render). Both axes are reported
/// explicitly; anchortree's re-ground count is `0` by construction
/// ([`anchortree_regrounds`](Self::anchortree_regrounds)), the same structural
/// zero [`RegroundLedger`](crate::RegroundLedger) carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BaselineReport {
    peer_snapshot_tokens_per_turn: Vec<usize>,
    anchortree_diff_tokens_per_turn: Vec<usize>,
    peer_self_heals: usize,
}

impl BaselineReport {
    /// A fresh report with no turns folded in.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one turn's token cost into both columns: the peer's full snapshot
    /// (`nodes`) and anchortree's delta (`diff`), priced with the same ruler.
    pub fn record_turn(&mut self, nodes: &[ObservedNode], diff: &Diff) {
        self.peer_snapshot_tokens_per_turn
            .push(snapshot_tokens(nodes));
        self.anchortree_diff_tokens_per_turn.push(diff_tokens(diff));
    }

    /// Set the LLM-re-ground axis: the total Stagehand self-heals from the
    /// replay (see [`StagehandCache::self_heals`]).
    pub fn set_peer_self_heals(&mut self, n: usize) {
        self.peer_self_heals = n;
    }

    /// Number of turns folded in.
    pub fn turns(&self) -> usize {
        self.anchortree_diff_tokens_per_turn.len()
    }

    /// Total tokens the Playwright-MCP peer spent re-sending full snapshots.
    pub fn peer_snapshot_tokens(&self) -> usize {
        self.peer_snapshot_tokens_per_turn.iter().sum()
    }

    /// Total tokens anchortree spent sending diffs.
    pub fn anchortree_diff_tokens(&self) -> usize {
        self.anchortree_diff_tokens_per_turn.iter().sum()
    }

    /// Per-turn peer snapshot token costs, in order.
    pub fn peer_snapshot_tokens_per_turn(&self) -> &[usize] {
        &self.peer_snapshot_tokens_per_turn
    }

    /// Per-turn anchortree diff token costs, in order.
    pub fn anchortree_diff_tokens_per_turn(&self) -> &[usize] {
        &self.anchortree_diff_tokens_per_turn
    }

    /// Total Stagehand self-heal `page.act` LLM calls.
    pub fn peer_self_heals(&self) -> usize {
        self.peer_self_heals
    }

    /// anchortree's LLM re-grounds: `0`, by construction — the engine's observe
    /// path takes no model client (see [`RegroundLedger`](crate::RegroundLedger)).
    pub fn anchortree_regrounds(&self) -> usize {
        0
    }

    /// How many times lighter anchortree's token payload is than the peer's,
    /// peer over anchortree. Returns `None` when anchortree spent nothing (an
    /// empty task), to avoid dividing by zero.
    pub fn token_ratio(&self) -> Option<f64> {
        let at = self.anchortree_diff_tokens();
        if at == 0 {
            None
        } else {
            Some(self.peer_snapshot_tokens() as f64 / at as f64)
        }
    }

    /// Render the two-axis headline for a log or report, e.g.
    /// `anchortree: 81 diff tokens, 0 re-grounds | peer: 213 snapshot tokens, 3 self-heals (over 4 turns)`.
    /// Both peer costs are stated explicitly so the comparison is on the record,
    /// not implied.
    pub fn render(&self) -> String {
        format!(
            "anchortree: {} diff tokens, {} re-grounds | peer: {} snapshot tokens, {} self-heals (over {} turns)",
            self.anchortree_diff_tokens(),
            self.anchortree_regrounds(),
            self.peer_snapshot_tokens(),
            self.peer_self_heals,
            self.turns(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::ElementChange;
    use crate::fingerprint::{Bbox, Fingerprint};
    use crate::identity::{Eid, ElementState, FrameKey};

    fn node(backend: i64, role: Role, name: &str) -> ObservedNode {
        ObservedNode {
            backend_node_id: backend,
            frame_key: FrameKey::root(),
            fingerprint: Fingerprint {
                stable_attr: None,
                role,
                accessible_name: name.to_string(),
                structural_path: String::new(),
                centroid: (0.0, 0.0),
            },
            bbox: Bbox {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            state: ElementState {
                enabled: true,
                visible: true,
                ..Default::default()
            },
            text: name.to_string(),
        }
    }

    #[test]
    fn snapshot_renders_in_playwright_line_shape() {
        let nodes = vec![
            node(11, Role::Textbox, "Email"),
            node(13, Role::Button, "Sign in"),
        ];
        assert_eq!(
            playwright_snapshot(&nodes),
            "- textbox \"Email\" [ref=e11]\n- button \"Sign in\" [ref=e13]\n"
        );
    }

    #[test]
    fn other_role_round_trips_its_aria_string() {
        let nodes = vec![node(1, Role::Other("gridcell".into()), "A1")];
        assert_eq!(playwright_snapshot(&nodes), "- gridcell \"A1\" [ref=e1]\n");
    }

    #[test]
    fn snapshot_tokens_price_the_full_inventory() {
        // The peer pays for every node, every turn — there is no delta.
        let nodes = vec![
            node(11, Role::Textbox, "Email"),
            node(12, Role::Textbox, "Password"),
            node(13, Role::Button, "Sign in"),
        ];
        let snap = playwright_snapshot(&nodes);
        assert_eq!(snapshot_tokens(&nodes), estimated_tokens(&snap));
        assert!(snapshot_tokens(&nodes) > 0);
    }

    #[test]
    fn diff_payload_is_lighter_than_a_full_resnapshot_on_a_steady_turn() {
        // The token thesis in miniature: on a turn where only one element
        // rebinds, anchortree sends one line; the peer re-sends the whole page.
        let nodes = vec![
            node(11, Role::Textbox, "Email"),
            node(12, Role::Textbox, "Password"),
            node(13, Role::Button, "Sign in"),
        ];
        let diff = Diff {
            rebound: vec![Eid("btn-sign-in".into())],
            ..Default::default()
        };
        assert!(
            diff_tokens(&diff) < snapshot_tokens(&nodes),
            "a one-line diff must be cheaper than a full re-snapshot"
        );
    }

    #[test]
    fn positions_are_a_bijection_both_directions() {
        let mut p = DomPositions::new();
        p.place("signin", "/form/*[3]");
        assert_eq!(p.xpath_of("signin"), Some("/form/*[3]"));
        assert_eq!(p.logical_at("/form/*[3]"), Some("signin"));
        assert_eq!(p.xpath_of("missing"), None);
        assert_eq!(p.logical_at("/nope"), None);
    }

    #[test]
    fn from_document_order_places_named_nodes_at_their_doc_index() {
        // Index counts every node; only named nodes are placed.
        let nodes = vec![
            node(1, Role::Heading, "Checkout"),
            node(2, Role::Other("paragraph".into()), ""), // unnamed: consumes index 2
            node(3, Role::Button, "Buy now"),
        ];
        let p = DomPositions::from_document_order(&nodes);
        assert_eq!(p.xpath_of("Checkout"), Some("/*[1]"));
        assert_eq!(p.xpath_of("Buy now"), Some("/*[3]"));
        // The unnamed paragraph occupies index 2 but is not cached by name.
        assert_eq!(p.logical_at("/*[2]"), None);
    }

    #[test]
    fn reorder_moves_a_named_element_to_a_new_absolute_index() {
        // The transition the head-to-head measures: a reorder that anchortree
        // rebinds for free (the name is unchanged) shifts the Stagehand
        // resolver's absolute selector. Before: button is the 3rd node.
        let before = DomPositions::from_document_order(&[
            node(1, Role::Heading, "Checkout"),
            node(2, Role::Other("paragraph".into()), ""),
            node(3, Role::Button, "Buy now"),
        ]);
        // After: the button moved ahead of the paragraph — now the 2nd node.
        let after = DomPositions::from_document_order(&[
            node(1, Role::Heading, "Checkout"),
            node(2, Role::Button, "Buy now"),
            node(3, Role::Other("paragraph".into()), ""),
        ]);
        // A Stagehand cache bound at `before` self-heals when re-tried at `after`:
        // the cached `/*[3]` no longer resolves to "Buy now".
        let mut cache = StagehandCache::new();
        cache.bind("Buy now", &before);
        assert_eq!(cache.reresolve(&before), 0, "same state — no heal");
        assert_eq!(
            cache.reresolve(&after),
            1,
            "the reorder shifted the button's absolute index, breaking the cached selector"
        );
    }

    fn layout_a() -> DomPositions {
        let mut p = DomPositions::new();
        p.place("email", "/form/*[1]");
        p.place("password", "/form/*[2]");
        p.place("signin", "/form/*[3]");
        p
    }

    fn layout_b() -> DomPositions {
        // A "Skip" link inserted at the top shifts every positional index down.
        let mut p = DomPositions::new();
        p.place("skip", "/form/*[1]");
        p.place("email", "/form/*[2]");
        p.place("password", "/form/*[3]");
        p.place("signin", "/form/*[4]");
        p
    }

    #[test]
    fn bind_costs_no_self_heal() {
        let mut cache = StagehandCache::new();
        let a = layout_a();
        cache.bind("signin", &a);
        assert_eq!(cache.self_heals(), 0);
        assert_eq!(cache.cached_len(), 1);
    }

    #[test]
    fn in_place_replace_costs_zero_self_heals() {
        // A framework re-render that keeps positions: the cached XPath still
        // resolves, so the Stagehand peer pays nothing — even though
        // anchortree's engine would count a rebind here. (Rebind != self-heal.)
        let mut cache = StagehandCache::new();
        let a = layout_a();
        cache.bind("signin", &a);
        // Same positions, different DOM nodes underneath: XPath unaffected.
        let healed = cache.reresolve(&layout_a());
        assert_eq!(healed, 0);
        assert_eq!(cache.self_heals(), 0);
    }

    #[test]
    fn sibling_insert_costs_one_self_heal_per_shifted_selector() {
        // No DOM node was swapped, so anchortree's engine counts zero rebinds —
        // but the inserted sibling shifts every index, breaking the cached
        // XPath. (Self-heal without rebind.)
        let mut cache = StagehandCache::new();
        cache.bind("signin", &layout_a());
        let healed = cache.reresolve(&layout_b());
        assert_eq!(healed, 1, "the shifted selector costs exactly one heal");
        assert_eq!(cache.self_heals(), 1);
        // And the cache is repaired, so re-trying the same state is free.
        assert_eq!(cache.reresolve(&layout_b()), 0);
    }

    #[test]
    fn rebind_without_position_change_is_zero_self_heals() {
        // The over-claim guard, stated as its own test: bind all three acted
        // elements, then replay a pure in-place re-render. anchortree would
        // rebind all three; the Stagehand peer heals none.
        let mut cache = StagehandCache::new();
        let a = layout_a();
        for logical in ["email", "password", "signin"] {
            cache.bind(logical, &a);
        }
        assert_eq!(cache.reresolve(&layout_a()), 0);
        assert_eq!(cache.self_heals(), 0);
    }

    #[test]
    fn report_renders_both_axes_explicitly() {
        let mut report = BaselineReport::new();
        let nodes = vec![
            node(11, Role::Textbox, "Email"),
            node(13, Role::Button, "Sign in"),
        ];
        // Turn 1: both carry the inventory once (added vs snapshot).
        report.record_turn(
            &nodes,
            &Diff {
                added: vec![Eid("inp-email".into()), Eid("btn-sign-in".into())],
                ..Default::default()
            },
        );
        // Turn 2: anchortree sends one changed line; peer re-sends everything.
        report.record_turn(
            &nodes,
            &Diff {
                changed: vec![ElementChange {
                    eid: Eid("btn-sign-in".into()),
                    text: "Signing in...".into(),
                }],
                ..Default::default()
            },
        );
        report.set_peer_self_heals(2);

        assert_eq!(report.turns(), 2);
        assert_eq!(report.anchortree_regrounds(), 0);
        assert_eq!(report.peer_self_heals(), 2);
        assert!(report.peer_snapshot_tokens() > report.anchortree_diff_tokens());
        assert!(report.token_ratio().unwrap() > 1.0);

        let at = report.anchortree_diff_tokens();
        let peer = report.peer_snapshot_tokens();
        assert_eq!(
            report.render(),
            format!(
                "anchortree: {at} diff tokens, 0 re-grounds | peer: {peer} snapshot tokens, 2 self-heals (over 2 turns)"
            )
        );
    }

    #[test]
    fn empty_report_has_no_token_ratio() {
        let report = BaselineReport::new();
        assert_eq!(report.turns(), 0);
        assert_eq!(report.token_ratio(), None);
        assert_eq!(report.anchortree_regrounds(), 0);
    }

    // --- Frame tier (D40/D41): the positional frame handle vs the discriminator. ---

    #[test]
    fn frame_order_is_a_positional_to_discriminator_view() {
        let order = FrameOrder::from_owner_order(&["checkout", "ads"]);
        assert_eq!(order.len(), 2);
        assert!(!order.is_empty());
        assert_eq!(order.discriminator_at(0), Some("checkout"));
        assert_eq!(order.discriminator_at(1), Some("ads"));
        assert_eq!(order.discriminator_at(2), None);
        assert_eq!(order.ordinal_of("checkout"), Some(0));
        assert_eq!(order.ordinal_of("ads"), Some(1));
        assert_eq!(order.ordinal_of("missing"), None);
    }

    #[test]
    fn binding_a_frame_handle_costs_no_reground() {
        let order = FrameOrder::from_owner_order(&["checkout"]);
        let mut cache = FrameOrdinalCache::new();
        cache.bind("checkout", &order);
        assert_eq!(cache.cached_len(), 1);
        assert_eq!(cache.regrounds(), 0);
    }

    #[test]
    fn sibling_frame_inserted_ahead_costs_the_positional_handle_one_reground() {
        // Leg B of the frame-tier head-to-head, measured: the agent acted inside
        // the distinctly-identified `checkout` frame at ordinal 0. A sibling
        // `ads` frame-owner is then inserted AHEAD of it, so `checkout` shifts to
        // ordinal 1. A Stagehand-style `frame ordinal + backendNodeId` handle
        // cached at ordinal 0 now resolves into `ads` — one LLM re-ground.
        let before = FrameOrder::from_owner_order(&["checkout"]);
        let mut peer = FrameOrdinalCache::new();
        peer.bind("checkout", &before);

        let after = FrameOrder::from_owner_order(&["ads", "checkout"]);
        assert_eq!(peer.reresolve(&after), 1, "the shifted ordinal is stale");
        assert_eq!(peer.regrounds(), 1);
        // Repaired: a second re-try against the same layout is free.
        assert_eq!(peer.reresolve(&after), 0);
    }

    #[test]
    fn the_discriminator_key_pays_zero_regrounds_on_the_same_reorder() {
        // The anchortree side of the SAME transition: its `FrameKey` is the
        // content-addressed discriminator, not the ordinal, so the durable lookup
        // finds `checkout` regardless of how many siblings were inserted ahead of
        // it. Zero re-grounds where the positional handle paid one — the
        // frame-tier head-to-head as a CI-gated number (D40 proven, D41 bounded).
        let after = FrameOrder::from_owner_order(&["ads", "checkout"]);
        assert_eq!(
            after.ordinal_of("checkout"),
            Some(1),
            "the durable key resolves the frame at its new position, no re-ground"
        );
        // Folded as a head-to-head: positional handle 1, discriminator 0.
        let before = FrameOrder::from_owner_order(&["checkout"]);
        let mut peer = FrameOrdinalCache::new();
        peer.bind("checkout", &before);
        let positional = peer.reresolve(&after);
        let discriminator_regrounds = usize::from(after.ordinal_of("checkout").is_none());
        assert_eq!((positional, discriminator_regrounds), (1, 0));
    }

    #[test]
    fn in_frame_churn_alone_does_not_move_the_frame_ordinal() {
        // Leg A: the inner card re-renders but no frame-owner is added or
        // reordered, so the frame layout is unchanged and the positional handle
        // pays nothing either. The frame tier's "rebind without self-heal" case,
        // matching the node tier's in-place leg.
        let order = FrameOrder::from_owner_order(&["checkout"]);
        let mut peer = FrameOrdinalCache::new();
        peer.bind("checkout", &order);
        let unchanged = FrameOrder::from_owner_order(&["checkout"]);
        assert_eq!(peer.reresolve(&unchanged), 0);
        assert_eq!(peer.regrounds(), 0);
    }

    #[test]
    fn identical_discriminator_siblings_collapse_to_first_ordinal() {
        // The D41 bound, at the FrameOrder level: two `src`-identical `ads` slots
        // are indistinguishable from author metadata, so the discriminator→ordinal
        // direction collapses to the first occurrence — document-order parity with
        // Playwright's `.nth()`, not a durable handle.
        let order = FrameOrder::from_owner_order(&["ads", "ads", "checkout"]);
        assert_eq!(order.ordinal_of("ads"), Some(0));
        assert_eq!(order.discriminator_at(0), Some("ads"));
        assert_eq!(order.discriminator_at(1), Some("ads"));
        assert_eq!(order.ordinal_of("checkout"), Some(2));
    }
}

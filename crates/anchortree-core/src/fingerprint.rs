//! Element fingerprints and the re-bind scoring ladder.
//!
//! A [`Fingerprint`] is a content-derived description of an element that
//! survives a DOM mutation. When a framework re-renders and swaps the
//! underlying DOM node (so the CDP `backendNodeId` changes), anchortree
//! re-binds the *logical* element to its new node by scoring the old
//! fingerprint against every candidate and keeping the eid if the best score
//! clears [`REBIND_THRESHOLD`]. This is what turns non-determinism into a
//! stable identity: the "Sign in" button keeps one handle even though its DOM
//! node was destroyed and recreated.

use crate::role::Role;

/// The minimum [`Fingerprint::match_score`] required to treat a candidate node
/// as the same logical element after a hard DOM mutation. Below this we mint a
/// fresh identity rather than risk a wrong rebind.
pub const REBIND_THRESHOLD: f32 = 0.6;

/// An axis-aligned bounding box in CSS pixels, as reported by CDP layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Bbox {
    /// The centre point of the box. Used as the weakest (geometry) rung of the
    /// re-bind ladder.
    pub fn centroid(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// A content-derived signature of an element, ordered from strongest to
/// weakest discriminator. The scoring in [`Fingerprint::match_score`] walks
/// this from the top: a stable attribute is near-certain identity, geometry is
/// only a tie-breaker.
#[derive(Debug, Clone, PartialEq)]
pub struct Fingerprint {
    /// A developer-stable attribute if present: `id`, `name`, `data-testid`,
    /// `aria-label`, or a `for` target. The single strongest signal.
    pub stable_attr: Option<String>,
    /// The accessibility role. A role mismatch vetoes any match.
    pub role: Role,
    /// The computed accessible name (visible label / aria-label / text).
    pub accessible_name: String,
    /// A structural path through the interactive ancestry, e.g.
    /// `form>button:2`. Survives cosmetic wrapper churn better than a full
    /// CSS selector.
    pub structural_path: String,
    /// The element centroid in CSS pixels, used only to break ties.
    pub centroid: (f32, f32),
}

impl Fingerprint {
    /// Score how strongly `self` (a remembered element) matches `other` (a
    /// freshly observed candidate). Returns `0.0` for an impossible match
    /// (role mismatch) up to `1.0` for a stable-attribute equality.
    ///
    /// The ladder, highest rung first:
    /// 1. role mismatch  -> `0.0` (hard veto)
    /// 2. stable_attr equal -> `1.0`
    /// 3. accessible-name equal -> `0.6`, similar -> `0.4`
    /// 4. structural_path equal -> `+0.3`
    /// 5. geometry close -> `+0.1`, near -> `+0.05`
    ///
    /// Rungs 3-5 accumulate so a name-and-structure agreement (0.6 + 0.3)
    /// comfortably clears [`REBIND_THRESHOLD`] without a stable attribute.
    pub fn match_score(&self, other: &Fingerprint) -> f32 {
        if self.role != other.role {
            return 0.0;
        }

        // Strongest rung: a developer-stable attribute that agrees.
        if let (Some(a), Some(b)) = (&self.stable_attr, &other.stable_attr) {
            if a == b {
                return 1.0;
            }
            // Two elements that both carry a stable attribute but disagree are
            // deliberately *different* elements. Don't rebind across them.
            return 0.0;
        }

        let mut score: f32 = 0.0;

        if self.accessible_name == other.accessible_name && !self.accessible_name.is_empty() {
            score += 0.6;
        } else if name_similar(&self.accessible_name, &other.accessible_name) {
            score += 0.4;
        }

        if self.structural_path == other.structural_path && !self.structural_path.is_empty() {
            score += 0.3;
        }

        let d = dist(self.centroid, other.centroid);
        if d < 8.0 {
            score += 0.1;
        } else if d < 40.0 {
            score += 0.05;
        }

        score.min(1.0)
    }
}

/// Euclidean distance between two points.
fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

/// Token-overlap (Jaccard) similarity of two accessible names. Returns `true`
/// when the labels share at least half their distinct lowercase word tokens,
/// which tolerates trailing count badges and minor copy edits.
fn name_similar(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let ta: std::collections::HashSet<String> = a
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    let tb: std::collections::HashSet<String> = b
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    if ta.is_empty() || tb.is_empty() {
        return false;
    }
    let inter = ta.intersection(&tb).count() as f32;
    let union = ta.union(&tb).count() as f32;
    inter / union >= 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(stable: Option<&str>, name: &str, path: &str, c: (f32, f32)) -> Fingerprint {
        Fingerprint {
            stable_attr: stable.map(String::from),
            role: Role::Button,
            accessible_name: name.to_string(),
            structural_path: path.to_string(),
            centroid: c,
        }
    }

    #[test]
    fn stable_attr_equality_is_certain() {
        let a = fp(Some("submit"), "Sign in", "form>button:1", (10.0, 10.0));
        let b = fp(Some("submit"), "Log in", "div>button:3", (900.0, 50.0));
        assert_eq!(a.match_score(&b), 1.0);
    }

    #[test]
    fn disagreeing_stable_attrs_never_match() {
        let a = fp(Some("submit"), "Sign in", "form>button:1", (10.0, 10.0));
        let b = fp(Some("cancel"), "Sign in", "form>button:1", (10.0, 10.0));
        assert_eq!(a.match_score(&b), 0.0);
    }

    #[test]
    fn role_mismatch_vetoes() {
        let mut a = fp(None, "Go", "form>button:1", (10.0, 10.0));
        let mut b = a.clone();
        a.role = Role::Button;
        b.role = Role::Link;
        assert_eq!(a.match_score(&b), 0.0);
    }

    #[test]
    fn name_plus_structure_clears_threshold_without_stable_attr() {
        // The headline scenario: same logical button, brand-new DOM node, no
        // stable attribute. Name (0.6) + structure (0.3) must rebind.
        let a = fp(None, "Sign in", "form>button:1", (10.0, 10.0));
        let b = fp(None, "Sign in", "form>button:1", (12.0, 11.0));
        let s = a.match_score(&b);
        assert!(
            s >= REBIND_THRESHOLD,
            "expected >= {REBIND_THRESHOLD}, got {s}"
        );
    }

    #[test]
    fn unrelated_elements_stay_below_threshold() {
        let a = fp(None, "Sign in", "form>button:1", (10.0, 10.0));
        let b = fp(None, "Delete account", "footer>button:9", (900.0, 999.0));
        assert!(a.match_score(&b) < REBIND_THRESHOLD);
    }
}

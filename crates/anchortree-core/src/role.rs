//! Semantic roles for observed elements.
//!
//! anchortree models a page as a set of *interactive* logical elements, each
//! carrying an accessibility role. The role drives two things: the human- and
//! agent-readable [`Eid`](crate::Eid) prefix (so `btn-submit` reads as a
//! button), and the [`Role::is_interactive`] filter that keeps the observation
//! small (we never mint identities for decorative containers).

/// An accessibility role, derived from the ARIA role of a CDP accessibility
/// node. Unknown roles are preserved verbatim in [`Role::Other`] so we never
/// lose information from the source tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Button,
    Link,
    Textbox,
    Searchbox,
    Combobox,
    Checkbox,
    Radio,
    Switch,
    Slider,
    Menuitem,
    Tab,
    Option,
    Heading,
    Region,
    Status,
    Other(String),
}

impl Role {
    /// The short, stable prefix used when minting an [`Eid`](crate::Eid).
    ///
    /// Prefixes are deliberately terse: an agent reading `btn-submit` or
    /// `inp-email` can infer the action space without a second lookup.
    pub fn prefix(&self) -> &str {
        match self {
            Role::Button => "btn",
            Role::Link => "lnk",
            Role::Textbox => "inp",
            Role::Searchbox => "srch",
            Role::Combobox => "sel",
            Role::Checkbox => "chk",
            Role::Radio => "rdo",
            Role::Switch => "sw",
            Role::Slider => "sld",
            Role::Menuitem => "mi",
            Role::Tab => "tab",
            Role::Option => "opt",
            Role::Heading => "hd",
            Role::Region => "rg",
            Role::Status => "st",
            Role::Other(_) => "el",
        }
    }

    /// Whether an element of this role is worth a durable identity.
    ///
    /// Interactive elements are the agent's action surface. Non-interactive
    /// roles (headings, regions, status) are observable for context but are
    /// not part of the click/type action space; callers can use this to keep
    /// the identity map small.
    pub fn is_interactive(&self) -> bool {
        matches!(
            self,
            Role::Button
                | Role::Link
                | Role::Textbox
                | Role::Searchbox
                | Role::Combobox
                | Role::Checkbox
                | Role::Radio
                | Role::Switch
                | Role::Slider
                | Role::Menuitem
                | Role::Tab
                | Role::Option
        )
    }

    /// Map an ARIA role string (as reported by CDP `Accessibility.getFullAXTree`)
    /// onto a [`Role`]. Unrecognised roles round-trip through [`Role::Other`].
    pub fn from_aria(role: &str) -> Role {
        match role {
            "button" => Role::Button,
            "link" => Role::Link,
            "textbox" => Role::Textbox,
            "searchbox" => Role::Searchbox,
            "combobox" => Role::Combobox,
            "checkbox" => Role::Checkbox,
            "radio" => Role::Radio,
            "switch" => Role::Switch,
            "slider" => Role::Slider,
            "menuitem" | "menuitemcheckbox" | "menuitemradio" => Role::Menuitem,
            "tab" => Role::Tab,
            "option" => Role::Option,
            "heading" => Role::Heading,
            "region" => Role::Region,
            "status" | "alert" => Role::Status,
            other => Role::Other(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_roles_round_trip() {
        assert_eq!(Role::from_aria("button"), Role::Button);
        assert_eq!(Role::from_aria("textbox"), Role::Textbox);
        assert_eq!(Role::Button.prefix(), "btn");
    }

    #[test]
    fn unknown_role_is_preserved() {
        assert_eq!(
            Role::from_aria("gridcell"),
            Role::Other("gridcell".to_string())
        );
        assert_eq!(Role::Other("gridcell".into()).prefix(), "el");
    }

    #[test]
    fn interactivity_partition() {
        assert!(Role::Button.is_interactive());
        assert!(!Role::Heading.is_interactive());
        assert!(!Role::Other("gridcell".into()).is_interactive());
    }
}

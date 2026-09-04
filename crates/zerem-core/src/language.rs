//! Which languages the app ships, and how a system tag maps onto one.
//!
//! The list is here rather than in the UI for the same reason the column titles
//! are: two places naming the same set is two places that drift, and the one
//! that drifts is always the one nobody looks at.
//!
//! Adding a language is a folder under `lang/` and a line in [`SHIPPED`]. There
//! is no third step, and forgetting the line is what a test here catches.

/// Every language the binary carries, source first.
///
/// The tag is the folder name under `lang/`; the name is what the preferences
/// panel shows, written in that language. A language named in English in a list
/// of languages is a list somebody has to translate to read.
pub const SHIPPED: [(&str, &str); 2] = [("en", "English"), ("pt-BR", "Português (Brasil)")];

/// What the preferences panel shows for "whatever the machine is set to".
///
/// Stored as an empty string rather than as a tag, so a machine that changes
/// its language is followed rather than pinned to whatever it was on the day
/// the app was first opened.
pub const SYSTEM: &str = "";

/// Which shipped language a system tag asks for.
///
/// Matched on the primary subtag as well as the whole thing, because a machine
/// set to `pt-PT` is asking for Portuguese and getting English would be the
/// wrong answer to a question that was answered nearly right. The region is
/// preferred when it is there — `pt-BR` beats `pt` for a Brazilian machine.
#[must_use]
pub fn resolve(tag: &str) -> &'static str {
    let tag = tag.trim();
    if let Some((exact, _)) = SHIPPED.iter().find(|(t, _)| t.eq_ignore_ascii_case(tag)) {
        return exact;
    }
    let primary = tag.split(['-', '_']).next().unwrap_or_default();
    if primary.is_empty() {
        return SHIPPED[0].0;
    }
    SHIPPED
        .iter()
        .find(|(t, _)| t.split('-').next().is_some_and(|p| p.eq_ignore_ascii_case(primary)))
        .map_or(SHIPPED[0].0, |(t, _)| *t)
}

/// What to show for a stored preference, and the tag to hand the renderer.
///
/// `SYSTEM` keeps its own name here rather than resolving: the panel has to say
/// "System" when that is what was chosen, not the language it happens to
/// produce today.
#[must_use]
pub fn label(stored: &str) -> &'static str {
    if stored == SYSTEM {
        return "System";
    }
    SHIPPED.iter().find(|(t, _)| *t == stored).map_or("System", |(_, name)| *name)
}

/// The next choice in the list, wrapping through "System".
///
/// The same shape the transfer limits use: a short list is faster to step
/// through than a menu is to open, and impossible to get wrong.
#[must_use]
pub fn next(stored: &str) -> &'static str {
    let order: Vec<&str> = std::iter::once(SYSTEM).chain(SHIPPED.iter().map(|(t, _)| *t)).collect();
    let at = order.iter().position(|t| *t == stored).unwrap_or(0);
    order[(at + 1) % order.len()]
}

#[cfg(test)]
mod tests {
    use super::{label, next, resolve, SHIPPED, SYSTEM};

    #[test]
    fn the_source_language_is_first_and_is_the_fallback() {
        // Everything unrecognised lands on it, so it had better be the one the
        // `.slint` is actually written in.
        assert_eq!(SHIPPED[0].0, "en");
        assert_eq!(resolve("kl-GL"), "en");
        assert_eq!(resolve(""), "en");
    }

    #[test]
    fn an_exact_tag_wins() {
        assert_eq!(resolve("pt-BR"), "pt-BR");
        assert_eq!(resolve("pt-br"), "pt-BR", "tags are not case-sensitive");
    }

    #[test]
    fn a_near_miss_lands_on_the_language_rather_than_on_english() {
        // A machine set to Portuguese of Portugal is asking for Portuguese.
        // Handing it English would be the wrong answer to a question that was
        // answered nearly right.
        assert_eq!(resolve("pt"), "pt-BR");
        assert_eq!(resolve("pt-PT"), "pt-BR");
        assert_eq!(resolve("pt_BR.UTF-8".split('.').next().unwrap()), "pt-BR");
    }

    #[test]
    fn every_language_names_itself_in_its_own_language() {
        // A list of languages named in English is a list somebody has to
        // translate in order to read.
        assert!(SHIPPED.iter().any(|(t, n)| *t == "pt-BR" && *n == "Português (Brasil)"));
    }

    #[test]
    fn following_the_machine_keeps_its_own_name() {
        // The panel has to say "System" when that is what was chosen, not the
        // language it happens to produce today.
        assert_eq!(label(SYSTEM), "System");
        assert_eq!(label("pt-BR"), "Português (Brasil)");
        assert_eq!(label("xx"), "System", "a hand-edited tag we do not ship");
    }

    #[test]
    fn stepping_wraps_through_the_whole_list_and_back() {
        let mut seen = vec![SYSTEM];
        let mut at = SYSTEM;
        for _ in 0..SHIPPED.len() {
            at = next(at);
            seen.push(at);
        }
        assert_eq!(next(at), SYSTEM, "the last step comes home");
        assert_eq!(seen.len(), SHIPPED.len() + 1, "every choice is reachable");
    }
}

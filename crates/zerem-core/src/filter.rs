//! Which rows the view shows.
//!
//! Separate from the ordering because it answers a different question — *which*
//! rows, not *in what order* — and because the answer only changes when someone
//! types, while the order can change every tick.

use crate::torrent::TorrentRow;

/// A parsed query.
///
/// Split on whitespace, and every token has to appear somewhere in the name.
/// A plain substring would be the wrong tool: torrent names are written
/// `The.Longest.Winter.2160p.WEB-DL`, so "longest winter" — which is how a
/// person types it — would match nothing. Tokens make the separators
/// irrelevant without reaching for fuzzy matching, which trades a predictable
/// answer for a clever one.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Filter {
    /// Folded to lower case once, here, for the same reason
    /// [`TorrentRow::name_key`] is folded once at construction: the alternative
    /// is re-walking both strings through the Unicode tables per row per
    /// keystroke.
    tokens: Vec<String>,
}

impl Filter {
    #[must_use]
    pub fn new(query: &str) -> Self {
        Self { tokens: query.split_whitespace().map(str::to_lowercase).collect() }
    }

    /// Whether this filter lets everything through. Whitespace alone does.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Whether this row belongs in the view.
    #[must_use]
    pub fn matches(&self, row: &TorrentRow) -> bool {
        self.tokens.iter().all(|token| row.name_key.contains(token.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::Filter;
    use crate::torrent::{TorrentId, TorrentRow};

    fn row(name: &str) -> TorrentRow {
        TorrentRow::new(TorrentId(1), name, 1000)
    }

    #[test]
    fn words_match_across_the_separators_torrents_are_named_with() {
        // The case the whole design is for: nobody types the dots.
        let t = row("The.Longest.Winter.2160p.WEB-DL.x265");
        assert!(Filter::new("longest winter").matches(&t));
        assert!(Filter::new("winter 2160p").matches(&t), "order does not matter");
        assert!(!Filter::new("longest summer").matches(&t), "every token has to land");
    }

    #[test]
    fn matching_ignores_case_on_both_sides() {
        let t = row("Debian-13.6.0-AMD64.iso");
        assert!(Filter::new("DEBIAN").matches(&t));
        assert!(Filter::new("amd64").matches(&t));
    }

    #[test]
    fn an_empty_query_is_not_a_filter() {
        let t = row("anything at all");
        assert!(Filter::new("").is_empty());
        assert!(Filter::new("   ").is_empty(), "whitespace alone is not a query");
        assert!(Filter::new("   ").matches(&t), "and it hides nothing");
    }

    #[test]
    fn a_partial_word_still_matches() {
        // Filtering has to answer on every keystroke, so "deb" must already
        // find what "debian" will.
        let t = row("debian-13.6.0-amd64.iso");
        for typed in ["d", "de", "deb", "debia", "debian"] {
            assert!(Filter::new(typed).matches(&t), "{typed} should already match");
        }
    }

    #[test]
    fn a_magnet_without_metadata_is_found_by_its_infohash() {
        // Until the swarm answers, the name *is* the infohash — so this works
        // without the filter knowing anything about hashes.
        let t = row("2c6b6858d61da9543d4231a71db4b1c9264b0685");
        assert!(Filter::new("2c6b6858").matches(&t));
    }
}

//! Which rows the view shows.
//!
//! Separate from the ordering because it answers a different question — *which*
//! rows, not *in what order* — and because the answer only changes when someone
//! types, while the order can change every tick.

use crate::torrent::{State, TorrentRow};

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

/// Which states the view is narrowed to.
///
/// Separate from the text [`Filter`] and combined with it rather than folded
/// into it: they answer different questions and are set by different gestures —
/// one by typing, one by a click that has to survive the typing.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Shown {
    #[default]
    All,
    Downloading,
    Seeding,
    Queued,
    Paused,
    Failed,
}

impl Shown {
    /// The discriminant the rail draws by, which is also what crosses into
    /// Slint — it has no enums.
    #[must_use]
    pub const fn index(self) -> i32 {
        match self {
            Self::All => 0,
            Self::Downloading => 1,
            Self::Seeding => 2,
            Self::Queued => 3,
            Self::Paused => 4,
            Self::Failed => 5,
        }
    }

    /// Back from that discriminant. Anything unrecognised is everything, which
    /// is the answer that cannot hide a torrent from somebody.
    #[must_use]
    pub const fn from_index(index: i32) -> Self {
        match index {
            1 => Self::Downloading,
            2 => Self::Seeding,
            3 => Self::Queued,
            4 => Self::Paused,
            5 => Self::Failed,
            _ => Self::All,
        }
    }

    /// Whether this row belongs in the view.
    ///
    /// Checking counts as downloading: it is the same torrent doing the same
    /// job, and a row that vanishes from the list while it hashes and comes
    /// back afterwards is a list that cannot be trusted to hold still.
    #[must_use]
    pub const fn matches(self, state: State) -> bool {
        match self {
            Self::All => true,
            Self::Downloading => matches!(state, State::Downloading | State::Checking),
            Self::Seeding => matches!(state, State::Seeding),
            Self::Queued => matches!(state, State::Queued),
            Self::Paused => matches!(state, State::Paused),
            Self::Failed => matches!(state, State::Error),
        }
    }
}

#[cfg(test)]
mod shown_tests {
    use super::Shown;
    use crate::State;

    #[test]
    fn everything_is_the_default_and_hides_nothing() {
        for state in
            [State::Paused, State::Checking, State::Downloading, State::Seeding, State::Error, State::Queued]
        {
            assert!(Shown::default().matches(state), "{state:?}");
        }
    }

    #[test]
    fn hashing_still_counts_as_downloading() {
        // A row that vanishes while it hashes and comes back afterwards is a
        // list that cannot be trusted to hold still.
        assert!(Shown::Downloading.matches(State::Checking));
        assert!(Shown::Downloading.matches(State::Downloading));
        assert!(!Shown::Paused.matches(State::Checking));
    }

    #[test]
    fn queued_is_told_apart_from_paused() {
        // The whole reason `Queued` is its own state: one is somebody stopping
        // it, the other is something being ahead of it.
        assert!(Shown::Queued.matches(State::Queued));
        assert!(!Shown::Queued.matches(State::Paused));
        assert!(!Shown::Paused.matches(State::Queued));
    }

    #[test]
    fn the_discriminant_survives_the_round_trip_into_slint() {
        for shown in
            [Shown::All, Shown::Downloading, Shown::Seeding, Shown::Queued, Shown::Paused, Shown::Failed]
        {
            assert_eq!(Shown::from_index(shown.index()), shown);
        }
    }

    #[test]
    fn an_index_from_nowhere_shows_everything() {
        // The answer that cannot hide a torrent from somebody.
        assert_eq!(Shown::from_index(99), Shown::All);
        assert_eq!(Shown::from_index(-1), Shown::All);
    }
}

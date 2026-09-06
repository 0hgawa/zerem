//! The one structure every piece of drawn state travels in.

use std::sync::Arc;

use zerem_core::{History, SessionStats, TorrentId, TorrentRow};

/// Everything the UI draws, as of one tick. Immutable — the UI never mutates a
/// snapshot, it replaces the one it holds.
///
/// Details deliberately absent: peers, files and trackers are asked for on
/// demand, only for the selected torrent and only while its panel is open.
/// Polling the peer list of 500 torrents once a second is how a client burns a
/// core without moving a byte.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Where a file can be streamed from, once there is somewhere.
    ///
    /// `None` on a machine that would not give a loopback port, where the
    /// window simply does not offer to play anything. It carries the secret
    /// with the port because a port on its own cannot build a URL any more.
    pub stream: Option<zerem_core::Stream>,
    /// Monotonic. An optimistic UI edit records the sequence it was made
    /// against, and the first snapshot past it is the truth that supersedes it.
    pub seq: u64,
    /// The inside of the one torrent whose panel is open, when one is.
    ///
    /// Absent by default and by design: it is built only for a torrent the UI
    /// asked to watch, so a closed panel costs nothing at all.
    pub details: Option<zerem_core::Details>,
    /// A torrent read but not yet accepted — what the add dialog is asking
    /// about. Present only between `Inspect` and the answer to it.
    pub pending: Option<zerem_core::Pending>,
    /// Bumped only when the row *set* changes — a torrent added or removed.
    ///
    /// It is what lets a consumer know its cached ordering is stale. Comparing
    /// lengths is not enough: one torrent removed and another added in the same
    /// tick leaves the count identical and every index wrong.
    pub generation: u64,
    pub stats: SessionStats,
    pub torrents: Vec<TorrentRow>,
    /// Something the user needs to be told that belongs to no single row — a
    /// magnet that would not parse, a port already in use. Clears itself after
    /// a few ticks.
    pub notice: Option<Arc<str>>,
    /// Torrents that finished on this tick, by name.
    ///
    /// The event and not the state: a consumer that compared completeness
    /// between snapshots would announce every torrent again after any hiccup,
    /// and one that read it from the row could not tell "finished just now"
    /// from "finished last Tuesday".
    pub finished: Vec<Arc<str>>,
    /// The last minute of session throughput, which the footer draws.
    ///
    /// Carried as samples rather than as a drawing: the contract here is what
    /// happened, and turning that into a shape is the consumer's business.
    pub history: History,
}

impl Snapshot {
    #[must_use]
    pub fn new(seq: u64, generation: u64, torrents: Vec<TorrentRow>) -> Self {
        Self {
            seq,
            generation,
            stats: SessionStats::of(&torrents),
            torrents,
            notice: None,
            details: None,
            pending: None,
            history: History::default(),
            finished: Vec::new(),
            stream: None,
        }
    }

    /// Say where a file can be streamed from.
    #[must_use]
    pub fn with_stream(mut self, at: Option<zerem_core::Stream>) -> Self {
        self.stream = at;
        self
    }

    #[must_use]
    pub fn with_notice(mut self, notice: Option<Arc<str>>) -> Self {
        self.notice = notice;
        self
    }

    #[must_use]
    pub fn with_details(mut self, details: Option<zerem_core::Details>) -> Self {
        self.details = details;
        self
    }

    #[must_use]
    pub fn with_pending(mut self, pending: Option<zerem_core::Pending>) -> Self {
        self.pending = pending;
        self
    }

    #[must_use]
    pub fn with_finished(mut self, finished: Vec<Arc<str>>) -> Self {
        self.finished = finished;
        self
    }

    #[must_use]
    pub const fn with_history(mut self, history: History) -> Self {
        self.history = history;
        self
    }

    #[must_use]
    pub fn position_of(&self, id: TorrentId) -> Option<usize> {
        self.torrents.iter().position(|t| t.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::Snapshot;
    use zerem_core::{State, TorrentId, TorrentRow};

    #[test]
    fn totals_are_derived_not_passed_in() {
        // One source of truth: a caller cannot hand over stats that disagree
        // with the rows they came from.
        let mut a = TorrentRow::new(TorrentId(1), "a", 100);
        a.state = State::Downloading;
        a.down_bps = 700;
        let snap = Snapshot::new(3, 1, vec![a, TorrentRow::new(TorrentId(2), "b", 100)]);
        assert_eq!(snap.stats.down_bps, 700);
        assert_eq!(snap.stats.active, 1);
        assert_eq!(snap.stats.paused, 1);
    }

    #[test]
    fn an_empty_snapshot_is_valid() {
        let snap = Snapshot::new(0, 0, Vec::new());
        assert!(snap.torrents.is_empty());
        assert_eq!(snap.stats.active, 0);
        assert!(snap.notice.is_none());
    }

    #[test]
    fn a_notice_rides_along_with_an_otherwise_empty_session() {
        // The case that matters: the session failed to start, so there are no
        // rows — and the explanation is the only thing the window has to show.
        let snap = Snapshot::new(1, 1, Vec::new())
            .with_notice(Some(std::sync::Arc::from("port 6881 is already in use")));
        assert_eq!(snap.notice.as_deref(), Some("port 6881 is already in use"));
    }

    #[test]
    fn a_torrent_is_found_by_id_not_by_position() {
        let snap = Snapshot::new(
            1,
            1,
            vec![TorrentRow::new(TorrentId(9), "a", 1), TorrentRow::new(TorrentId(4), "b", 1)],
        );
        assert_eq!(snap.position_of(TorrentId(4)), Some(1));
        assert_eq!(snap.position_of(TorrentId(99)), None);
    }
}

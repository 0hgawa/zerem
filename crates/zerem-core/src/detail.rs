//! What one torrent looks like from the inside.
//!
//! Deliberately outside [`crate::TorrentRow`], and outside the snapshot's row
//! list: this is only ever built for the torrent whose panel is open. Polling
//! the file list and peer table of five hundred torrents once a second is how a
//! client burns a core without moving a byte.

use std::sync::Arc;

use crate::TorrentId;

/// One file inside a torrent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileRow {
    /// Path relative to the torrent's folder, as the torrent declares it.
    pub path: Arc<str>,
    pub size: u64,
    pub done: u64,
    /// Whether it is being fetched at all.
    ///
    /// A torrent added without a choice wants everything, so this is `true` for
    /// most rows most of the time. It lives beside the path rather than in a
    /// list of indices next to it, because two lists indexed against each other
    /// are two lists that eventually disagree.
    pub wanted: bool,
}

impl FileRow {
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.done >= self.size
    }

    /// Progress in ten-thousandths, integral for the same reason the row's is.
    #[must_use]
    pub const fn progress_bp(&self) -> u64 {
        match (self.done * 10_000).checked_div(self.size) {
            Some(bp) => bp,
            None => 0,
        }
    }
}

/// How a peer is connected. Worth showing on its own: it is the only place uTP
/// becomes visible, and whether it works at all was the open question of the
/// whole engine spike.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transport {
    Tcp,
    Utp,
    Socks,
    Unknown,
}

impl Transport {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Utp => "uTP",
            Self::Socks => "SOCKS",
            Self::Unknown => "—",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PeerRow {
    pub addr: Arc<str>,
    /// What the peer says it is. Absent until the handshake carries it.
    pub client: Option<Arc<str>>,
    pub transport: Transport,
    pub state: &'static str,
    /// Totals for this connection, not rates: librqbit counts bytes, and a rate
    /// would have to be derived from two samples we do not keep.
    pub downloaded: u64,
    pub uploaded: u64,
}

/// The inside of the one torrent whose panel is open.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Details {
    pub id: TorrentId,
    pub files: Vec<FileRow>,
    pub peers: Vec<PeerRow>,
}

/// The file selection that turning files on or off would produce.
///
/// `current` is what is being fetched now, `None` meaning everything — which is
/// how a torrent added without a choice is recorded, and not the same as a list
/// that happens to name every file. `file` names one, or is `None` for all of
/// them at once, which is the only thing that makes a torrent of four thousand
/// files editable by hand.
///
/// Returns `None` when the change would leave nothing to fetch. Refused rather
/// than obeyed: it is the rule the add dialog already enforces with a disabled
/// button, and a torrent that downloads no files is not a state anyone reaches
/// for on purpose. An index at or past `count` is refused the same way — the
/// caller knows the file count and has no business sending one.
#[must_use]
pub fn select_files(
    current: Option<&[usize]>,
    count: usize,
    file: Option<usize>,
    wanted: bool,
) -> Option<Vec<usize>> {
    let mut chosen = current.map_or_else(
        || vec![true; count],
        |list| {
            let mut flags = vec![false; count];
            for &i in list.iter().filter(|&&i| i < count) {
                flags[i] = true;
            }
            flags
        },
    );

    match file {
        Some(i) if i < count => chosen[i] = wanted,
        Some(_) => return None,
        None => chosen.fill(wanted),
    }

    let kept: Vec<usize> = chosen.iter().enumerate().filter(|(_, w)| **w).map(|(i, _)| i).collect();
    (!kept.is_empty()).then_some(kept)
}

#[cfg(test)]
mod tests {
    use super::{select_files, FileRow, Transport};
    use std::sync::Arc;

    fn file(size: u64, done: u64) -> FileRow {
        FileRow { path: Arc::from("a/b.mkv"), size, done, wanted: true }
    }

    #[test]
    fn progress_survives_a_zero_length_file() {
        // Torrents do contain them, and dividing by the length is the crash.
        assert_eq!(file(0, 0).progress_bp(), 0);
        assert!(file(0, 0).is_complete(), "nothing to fetch means it is done");
    }

    #[test]
    fn progress_is_exact_at_the_ends() {
        assert_eq!(file(400, 0).progress_bp(), 0);
        assert_eq!(file(400, 100).progress_bp(), 2500);
        assert_eq!(file(400, 400).progress_bp(), 10_000);
    }

    #[test]
    fn every_transport_reads_as_something() {
        // The dash matters: an unknown transport must not render as an empty
        // cell that looks like a bug.
        assert_eq!(Transport::Utp.label(), "uTP");
        assert_eq!(Transport::Unknown.label(), "—");
    }

    #[test]
    fn no_restriction_is_every_file_rather_than_none() {
        // librqbit records a torrent added without a choice as `None`, which
        // reads as "no list" and means the opposite of an empty one.
        assert_eq!(select_files(None, 3, Some(1), false), Some(vec![0, 2]));
    }

    #[test]
    fn turning_one_on_keeps_the_rest_where_they_were() {
        assert_eq!(select_files(Some(&[0]), 3, Some(2), true), Some(vec![0, 2]));
        assert_eq!(select_files(Some(&[0, 2]), 3, Some(0), false), Some(vec![2]));
    }

    #[test]
    fn all_at_once_is_the_only_way_a_four_thousand_file_torrent_is_editable() {
        assert_eq!(select_files(Some(&[1]), 3, None, true), Some(vec![0, 1, 2]));
    }

    #[test]
    fn a_change_that_would_leave_nothing_is_refused() {
        // Both routes to it: the last tick, and "None" over the whole list.
        assert_eq!(select_files(Some(&[1]), 3, Some(1), false), None);
        assert_eq!(select_files(None, 3, None, false), None);
    }

    #[test]
    fn turning_off_a_file_that_is_already_off_changes_nothing() {
        // Two clicks racing a tick must not produce a different answer than one.
        assert_eq!(select_files(Some(&[0, 2]), 3, Some(1), false), Some(vec![0, 2]));
    }

    #[test]
    fn an_index_the_torrent_does_not_have_is_refused() {
        // It can only come from a stale view of a torrent whose metadata moved,
        // and guessing what was meant is worse than doing nothing.
        assert_eq!(select_files(None, 3, Some(3), true), None);
    }

    #[test]
    fn a_stale_index_in_the_current_list_is_dropped_rather_than_panicking() {
        // Defensive: the list comes back from librqbit, and indexing a shorter
        // file list with it would be the crash.
        assert_eq!(select_files(Some(&[0, 9]), 2, Some(1), true), Some(vec![0, 1]));
    }
}

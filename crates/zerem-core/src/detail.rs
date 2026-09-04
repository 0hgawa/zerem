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
    /// Whether this file is being fetched *before* the others.
    ///
    /// While any file in a torrent is pinned, it is the only thing coming down
    /// — see [`crate::choice`] for why that is the honest shape of "priority"
    /// on top of the one lever librqbit offers.
    pub first: bool,
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

#[cfg(test)]
mod tests {
    use super::{FileRow, Transport};
    use std::sync::Arc;

    fn file(size: u64, done: u64) -> FileRow {
        FileRow { path: Arc::from("a/b.mkv"), size, done, wanted: true, first: false }
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
}

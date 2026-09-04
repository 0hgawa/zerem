//! What travels back up: the UI's intent.
//!
//! Only the commands the engine actually carries out live here. A variant that
//! nothing sends and nothing handles is not a plan, it is dead code — the rest
//! join as the phases that need them land.

use zerem_core::TorrentId;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// Add a magnet, URL or path straight away, without asking.
    ///
    /// For what arrives on the command line, where there is no dialog to
    /// answer. Everything the user starts inside the app goes through
    /// `Inspect` instead.
    Add {
        source: String,
    },
    /// Read a torrent without starting it, so the user can be shown what is in
    /// it before deciding.
    ///
    /// A magnet has to reach the swarm for its metadata first, which can take
    /// seconds — the snapshot says it is fetching rather than looking stuck.
    Inspect {
        source: String,
    },
    /// Accept what `Inspect` found, downloading only these files. `None` is all
    /// of them, which is what librqbit wants rather than a list naming each one.
    ConfirmAdd {
        only_files: Option<Vec<usize>>,
    },
    /// Throw away what `Inspect` found.
    CancelAdd,
    Start(TorrentId),
    Pause(TorrentId),
    Remove {
        id: TorrentId,
        /// The data goes to the recycle bin, never to `remove_file` — a
        /// mis-click must stay recoverable.
        delete_data: bool,
    },
    /// How many torrents may download at once. Zero is no limit.
    ///
    /// Seeding is never counted and never queued: a finished torrent costs no
    /// download bandwidth, which is the thing being rationed.
    SetMaxActive(u32),
    /// Whether the next torrent added is added stopped.
    SetAddPaused(bool),
    /// Global transfer caps in bytes per second. `None` is unlimited.
    ///
    /// Takes effect at once — librqbit's rate limiters are settable while the
    /// session runs, so this is not a restart-only setting.
    SetLimits {
        down: Option<u32>,
        up: Option<u32>,
    },
    /// Fetch this file, or stop fetching it, on a torrent already in the list.
    ///
    /// The command names one file and the answer, never the whole selection.
    /// The engine reads what is current and applies the change to *that*, so a
    /// click cannot carry a second-old view of the torrent back over whatever
    /// happened in between.
    SetFileWanted {
        id: TorrentId,
        /// Which one, or `None` for every file at once — the only thing that
        /// makes a torrent of four thousand files editable by hand.
        file: Option<usize>,
        wanted: bool,
    },
    /// Fetch this file before the others, or stop doing that.
    ///
    /// While anything in a torrent is pinned it is the only thing coming down.
    /// Not a queue position like qBittorrent's — librqbit has none to offer,
    /// and [`zerem_core::choice`] says what was done instead and why.
    SetFileFirst {
        id: TorrentId,
        file: usize,
        first: bool,
    },
    /// Build the inside of this torrent — files and peers — into each snapshot.
    ///
    /// `None` stops. The panel being closed has to mean the work is not done,
    /// not merely not drawn: a client that polls the peer table of every torrent
    /// it holds burns a core without moving a byte.
    WatchDetails(Option<TorrentId>),
    /// Where torrents added from now on are written.
    ///
    /// Only from now on: the ones already in the list keep their folder, because
    /// changing a setting cannot move data that is already on disk.
    SetDownloadDir(std::path::PathBuf),
}

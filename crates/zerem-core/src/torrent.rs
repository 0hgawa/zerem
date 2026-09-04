//! The domain row — what the engine reports and the table draws.
//!
//! Every field is integral. No floats anywhere, so `PartialEq` is exact and
//! cannot be tripped by rounding noise: a ratio wobbling in its last float digit
//! would mark every row dirty on every tick and silently defeat the model diff
//! the whole UI rests on.

use std::sync::Arc;

use crate::content::Content;

/// A torrent's identity. A newtype because a view index is also a number, and
/// confusing the two is the single easiest way to act on the wrong torrent
/// after a re-sort.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TorrentId(pub u32);

/// Why a running torrent is not moving.
///
/// Worth a column because "Downloading" over a row that has transferred
/// nothing for a minute is a lie, and a progress bar that never fills with no
/// word of explanation is the spinner-forever that this exists to replace.
///
/// Deliberately short of everything one might want to say. Whether the tracker
/// answered, and whether the listening port is reachable from outside, are both
/// invisible from here — librqbit reports neither — so this says exactly what
/// the peer counts support and stops. A guess dressed up as a diagnosis is
/// worse than the word "Downloading".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stall {
    /// A magnet whose file list has not come back from the swarm yet.
    Metadata,
    /// Nobody has been found at all — no tracker, no DHT, no exchange.
    NoPeers,
    /// Peers are known, and none of them is connected.
    Connecting,
    /// Connected, and nothing is arriving. Usually means no one in the swarm
    /// has the pieces still wanted.
    Idle,
}

impl Stall {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Metadata => "Fetching metadata",
            Self::NoPeers => "No peers found",
            Self::Connecting => "Connecting",
            Self::Idle => "No one is sharing",
        }
    }

    /// Whether this is something wrong or something in progress.
    ///
    /// Only a fault takes the warning colour. Fetching metadata and connecting
    /// are what a healthy torrent does in its first seconds, and painting those
    /// orange would teach the user to ignore the colour by the third torrent.
    #[must_use]
    pub const fn is_fault(self) -> bool {
        matches!(self, Self::NoPeers | Self::Idle)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Paused,
    Checking,
    Downloading,
    Seeding,
    /// Stalled on something the user has to resolve — no disk space, a missing
    /// output folder, a file it cannot write. Always carries a message in
    /// [`TorrentRow::error`]; a state with no explanation is a dead end.
    Error,
}

impl State {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Paused => "Paused",
            Self::Checking => "Checking",
            Self::Downloading => "Downloading",
            Self::Seeding => "Seeding",
            Self::Error => "Error",
        }
    }

    /// The discriminant the UI colours by. Kept as a plain integer because it
    /// crosses into Slint, which has no enums.
    #[must_use]
    pub const fn kind(self) -> i32 {
        match self {
            Self::Paused => 0,
            Self::Downloading => 1,
            Self::Seeding => 2,
            Self::Checking => 3,
            Self::Error => 4,
        }
    }

    /// Whether the torrent is doing anything. A failed one is not: it consumes
    /// nothing and will not move until someone intervenes.
    #[must_use]
    pub const fn is_active(self) -> bool {
        !matches!(self, Self::Paused | Self::Error)
    }
}

/// One torrent, as of one snapshot.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TorrentRow {
    pub id: TorrentId,
    /// Shared rather than owned: the name never changes, and cloning a `String`
    /// for 2000 rows a second is 2000 allocations for an identical value.
    pub name: Arc<str>,
    /// `name`, folded to lower case once when the torrent is added, so the name
    /// sort is a byte comparison. Folding inside the comparator instead was
    /// measured at 3.2 ms per sort — a sort makes ~22 000 comparisons and each
    /// one re-walked both strings through the Unicode tables.
    pub name_key: Arc<str>,
    pub size: u64,
    pub done: u64,
    pub state: State,
    pub down_bps: u64,
    pub up_bps: u64,
    pub peers_connected: u32,
    pub peers_total: u32,
    /// Seconds remaining, or `None` for "no estimate" — which is not the same
    /// as zero and must not sort like it.
    pub eta: Option<u32>,
    /// Share ratio in hundredths. Integral so equality is exact.
    pub ratio_x100: u32,
    /// Why it stopped, when [`State::Error`]. Shared rather than owned because
    /// the same message repeats on every tick until the cause is fixed, and
    /// re-allocating it each time would dirty the row for nothing.
    ///
    /// The engine's own error text is the useful half of a failure; dropping it
    /// and keeping only the state would leave the user a red row and no way to
    /// act on it.
    pub error: Option<Arc<str>>,
    /// Where this torrent's data lives. Fixed for its lifetime, so it is shared
    /// rather than owned and costs the diff nothing.
    ///
    /// Carried on the row, not fetched on demand, because "open folder" has to
    /// work from a right-click without a panel being open first.
    pub folder: Arc<str>,
    /// Lower-case hex infohash, which is what a magnet link is made of.
    pub info_hash: Arc<str>,
    /// What the torrent mostly holds, which is what the row's icon shows.
    ///
    /// `Copy` and fixed once the metadata arrives, so it costs the diff nothing
    /// — the whole reason it is resolved in the engine rather than recomputed
    /// from the file list on every tick.
    pub content: Content,
    /// Why this torrent is not moving, when it should be and is not.
    ///
    /// Decided in the engine rather than here, because the honest answer needs
    /// to know how long the torrent has been still — and a row is one tick by
    /// definition. `None` on anything that is transferring, seeding, hashing,
    /// paused or failed.
    pub stall: Option<Stall>,
}

impl TorrentRow {
    /// A freshly added torrent: nothing transferred, nothing known, paused.
    ///
    /// Folds the sort key, so it allocates. Anything building a row once per
    /// tick wants [`Self::shared`] instead.
    #[must_use]
    pub fn new(id: TorrentId, name: &str, size: u64) -> Self {
        Self::shared(id, name.into(), name.to_lowercase().into(), size)
    }

    /// A row whose name and sort key are already shared.
    ///
    /// Both are fixed for the life of a torrent, and the engine holds them as
    /// `Arc<str>` for exactly that reason. Re-deriving them here would allocate
    /// three times per row per tick and re-fold every name through the Unicode
    /// tables once a second — which is the cost [`crate::sort`] measured at
    /// 3.2 ms and moved out of the comparator in the first place.
    #[must_use]
    pub fn shared(id: TorrentId, name: Arc<str>, name_key: Arc<str>, size: u64) -> Self {
        Self {
            id,
            name,
            name_key,
            size,
            done: 0,
            state: State::Paused,
            down_bps: 0,
            up_bps: 0,
            peers_connected: 0,
            peers_total: 0,
            eta: None,
            ratio_x100: 0,
            error: None,
            folder: Arc::from(""),
            info_hash: Arc::from(""),
            content: Content::Unknown,
            stall: None,
        }
    }

    /// A magnet link for this torrent, for the clipboard.
    ///
    /// Built here rather than kept as a third field: it is derived from two
    /// things the row already has, and a copy of it on every row would be a
    /// string per torrent that nothing reads until someone right-clicks.
    #[must_use]
    pub fn magnet(&self) -> String {
        // `dn` is a display hint, so a name that would need escaping is simply
        // left out rather than mangled — the infohash is the part that matters.
        let plain = self.name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        if plain {
            format!("magnet:?xt=urn:btih:{}&dn={}", self.info_hash, self.name)
        } else {
            format!("magnet:?xt=urn:btih:{}", self.info_hash)
        }
    }

    /// Stop with a reason the user can act on.
    pub fn fail(&mut self, reason: impl Into<Arc<str>>) {
        self.pause();
        self.state = State::Error;
        self.error = Some(reason.into());
    }

    /// What the state column shows, in order of how much it says.
    ///
    /// A failure shows its message rather than the word "Error": the colour
    /// already says something is wrong, so the text is free to say what. A
    /// torrent that is running but standing still says why instead of
    /// "Downloading", which over an empty progress bar means nothing.
    #[must_use]
    pub fn status_text(&self) -> &str {
        self.error.as_deref().or_else(|| self.stall.map(Stall::label)).unwrap_or_else(|| self.state.label())
    }

    /// The discriminant the state column and the progress bar colour by.
    ///
    /// Its own value rather than [`State::kind`] so a stalled download stops
    /// reading as a healthy one: the words say something is wrong and the row
    /// has to agree with them. Only a fault takes it — see [`Stall::is_fault`].
    #[must_use]
    pub const fn status_kind(&self) -> i32 {
        match self.stall {
            Some(stall) if stall.is_fault() => 5,
            _ => self.state.kind(),
        }
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.state.is_active()
    }

    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.done >= self.size
    }

    /// Progress in ten-thousandths — the sort key. Integral, so ordering is
    /// stable and the comparison exact.
    #[must_use]
    pub const fn progress_bp(&self) -> u64 {
        match (self.done * 10_000).checked_div(self.size) {
            Some(bp) => bp,
            None => 0,
        }
    }

    /// Stop this torrent.
    ///
    /// Lives here rather than in the engine because the UI performs the same
    /// transition optimistically the instant the button is clicked, and two
    /// copies of it would be two chances to disagree.
    ///
    /// Clearing the rates matters: a paused torrent transfers nothing, and a row
    /// that still shows 4 MB/s under the word "Paused" is the kind of small lie
    /// that makes a client feel untrustworthy.
    pub fn pause(&mut self) {
        self.state = State::Paused;
        self.down_bps = 0;
        self.up_bps = 0;
        self.eta = None;
        // `error` only means anything alongside `State::Error`. Leaving it set
        // would show a stale failure under a torrent that is merely stopped.
        self.error = None;
        // Same for the stall: a stopped torrent is not failing to move, it is
        // not trying.
        self.stall = None;
    }

    /// Start this torrent. What it becomes depends on whether it already has
    /// the data.
    pub fn resume(&mut self) {
        self.state = if self.is_complete() { State::Seeding } else { State::Downloading };
        self.error = None;
    }
}

/// The session totals shown in the status bar.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SessionStats {
    pub down_bps: u64,
    pub up_bps: u64,
    pub active: u32,
    pub paused: u32,
}

impl SessionStats {
    /// Fold the rows into their totals. One pass, no allocation.
    #[must_use]
    pub fn of(rows: &[TorrentRow]) -> Self {
        rows.iter().fold(Self::default(), |mut acc, t| {
            acc.down_bps += t.down_bps;
            acc.up_bps += t.up_bps;
            if t.is_active() {
                acc.active += 1;
            } else {
                acc.paused += 1;
            }
            acc
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionStats, Stall, State, TorrentId, TorrentRow};

    fn row(id: u32, name: &str, size: u64) -> TorrentRow {
        TorrentRow::new(TorrentId(id), name, size)
    }

    #[test]
    fn a_new_torrent_starts_paused_and_empty() {
        let t = row(1, "Archipelago.2160p", 1000);
        assert_eq!(t.state, State::Paused);
        assert!(!t.is_active());
        assert_eq!(t.progress_bp(), 0);
    }

    #[test]
    fn the_sort_key_is_folded_at_construction() {
        let t = row(1, "Archipelago.WEB-DL", 1);
        assert_eq!(&*t.name, "Archipelago.WEB-DL");
        assert_eq!(&*t.name_key, "archipelago.web-dl");
    }

    #[test]
    fn progress_survives_a_zero_size() {
        // A magnet whose metadata has not arrived reports size 0, and dividing
        // by it is the crash that would take the whole window down.
        let t = row(1, "pending metadata", 0);
        assert_eq!(t.progress_bp(), 0);
    }

    #[test]
    fn progress_is_exact_at_the_ends() {
        let mut t = row(1, "x", 400);
        assert_eq!(t.progress_bp(), 0);
        t.done = 100;
        assert_eq!(t.progress_bp(), 2500);
        t.done = 400;
        assert_eq!(t.progress_bp(), 10_000);
        assert!(t.is_complete());
    }

    #[test]
    fn pausing_clears_the_rates_it_no_longer_earns() {
        let mut t = row(1, "x", 1000);
        t.state = State::Downloading;
        t.down_bps = 4_000_000;
        t.up_bps = 900;
        t.eta = Some(120);

        t.pause();
        assert_eq!(t.state, State::Paused);
        assert_eq!(t.down_bps, 0);
        assert_eq!(t.up_bps, 0);
        assert_eq!(t.eta, None);
    }

    #[test]
    fn a_magnet_link_always_carries_the_infohash() {
        let mut t = row(1, "debian-13.6.0-amd64.iso", 1);
        t.info_hash = std::sync::Arc::from("2c6b6858d61da9543d4231a71db4b1c9264b0685");
        assert_eq!(
            t.magnet(),
            "magnet:?xt=urn:btih:2c6b6858d61da9543d4231a71db4b1c9264b0685&dn=debian-13.6.0-amd64.iso"
        );
    }

    #[test]
    fn a_name_that_would_need_escaping_is_dropped_not_mangled() {
        // `dn` is only a display hint. An unescaped space or ampersand would
        // produce a link that silently means something else.
        let mut t = row(1, "The Longest Winter & Co", 1);
        t.info_hash = std::sync::Arc::from("abc123");
        assert_eq!(t.magnet(), "magnet:?xt=urn:btih:abc123");
    }

    #[test]
    fn a_failure_stops_the_torrent_and_keeps_the_reason() {
        let mut t = row(1, "x", 1000);
        t.state = State::Downloading;
        t.down_bps = 4_000_000;

        t.fail("not enough disk space: 2.3 GB more required");
        assert_eq!(t.state, State::Error);
        assert_eq!(t.down_bps, 0, "a failed torrent transfers nothing");
        assert!(!t.is_active(), "it will not move until someone intervenes");
        // The message is the useful half of a failure. Losing it would leave a
        // red row and no way to act on it.
        assert_eq!(t.status_text(), "not enough disk space: 2.3 GB more required");
    }

    #[test]
    fn a_healthy_torrent_shows_its_state_not_an_error() {
        let mut t = row(1, "x", 1000);
        assert_eq!(t.status_text(), "Paused");
        t.resume();
        assert_eq!(t.status_text(), "Downloading");
    }

    #[test]
    fn resuming_clears_a_previous_failure() {
        // Retrying after freeing disk space must not leave the old message
        // sitting under a running torrent.
        let mut t = row(1, "x", 1000);
        t.fail("no space");
        t.resume();
        assert_eq!(t.state, State::Downloading);
        assert_eq!(t.error, None);
        assert_eq!(t.status_text(), "Downloading");
    }

    #[test]
    fn resuming_seeds_when_the_data_is_already_there() {
        let mut t = row(1, "x", 1000);
        t.resume();
        assert_eq!(t.state, State::Downloading);

        t.done = 1000;
        t.resume();
        assert_eq!(t.state, State::Seeding, "a complete torrent seeds, it does not download");
    }

    #[test]
    fn a_stalled_download_says_why_instead_of_saying_it_is_downloading() {
        // The spinner-forever this replaces: a progress bar that never fills,
        // under the word "Downloading", with nothing to act on.
        let mut t = row(1, "x", 1000);
        t.resume();
        assert_eq!(t.status_text(), "Downloading");

        t.stall = Some(Stall::NoPeers);
        assert_eq!(t.status_text(), "No peers found");
    }

    #[test]
    fn a_failure_outranks_a_stall_because_it_is_the_reason_for_it() {
        let mut t = row(1, "x", 1000);
        t.stall = Some(Stall::Idle);
        t.fail("no space left on device");
        assert_eq!(t.status_text(), "no space left on device");
    }

    #[test]
    fn only_a_fault_recolours_the_row() {
        // Fetching metadata and connecting are what a healthy torrent does in
        // its first seconds. Painting those orange would teach the user to
        // ignore the colour by the third torrent.
        let mut t = row(1, "x", 1000);
        t.resume();
        for waiting in [Stall::Metadata, Stall::Connecting] {
            t.stall = Some(waiting);
            assert_eq!(t.status_kind(), State::Downloading.kind(), "{waiting:?} is not a fault");
        }
        for fault in [Stall::NoPeers, Stall::Idle] {
            t.stall = Some(fault);
            assert_eq!(t.status_kind(), 5, "{fault:?} is one");
        }
    }

    #[test]
    fn pausing_clears_the_stall_it_no_longer_has() {
        // A stopped torrent is not failing to move, it is not trying. Leaving
        // the reason set would put "No peers found" under the word Paused.
        let mut t = row(1, "x", 1000);
        t.stall = Some(Stall::NoPeers);
        t.pause();
        assert_eq!(t.stall, None);
        assert_eq!(t.status_text(), "Paused");
    }

    #[test]
    fn totals_split_active_from_paused() {
        let mut rows = vec![row(1, "a", 100), row(2, "b", 100), row(3, "c", 100)];
        rows[0].state = State::Downloading;
        rows[0].down_bps = 500;
        rows[1].state = State::Seeding;
        rows[1].up_bps = 300;

        let stats = SessionStats::of(&rows);
        assert_eq!(stats.down_bps, 500);
        assert_eq!(stats.up_bps, 300);
        assert_eq!(stats.active, 2);
        assert_eq!(stats.paused, 1);
    }

    #[test]
    fn totals_of_an_empty_session_are_zero() {
        assert_eq!(SessionStats::of(&[]), SessionStats::default());
    }
}

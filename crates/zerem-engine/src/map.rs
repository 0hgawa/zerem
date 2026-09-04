//! librqbit's `TorrentStats` → Zerem's `TorrentRow`.
//!
//! The whole adapter between the engine and the domain. It either accounts for
//! every state librqbit can report or it silently loses one, which is why the
//! `match` in `state_of` is exhaustive on purpose.
//!
//! Two traps found while writing it, both of which would have shipped:
//!
//!  * `Speed { mbps: f64 }` is **mebibytes** per second, not megabits — see its
//!    own `as_bytes()`, which multiplies by 1024². Reading the field name as
//!    megabits puts every speed in the UI out by a factor of eight.
//!  * `TorrentStatsState::Error` carries its explanation in a *sibling* field,
//!    `TorrentStats::error`. Matching on the state alone gives a red row with
//!    nothing to act on.
//!
//! And one thing librqbit does not give at all: `LiveStats::time_remaining` is a
//! `DurationWithHumanReadable`, a newtype over `Duration` with a private field,
//! no accessor and no `Deref`. It can be printed and nothing else. So the ETA is
//! computed here from bytes and rate — which is where it belonged anyway, since
//! the seconds have to be smoothed before they reach a column.

use std::sync::Arc;

use librqbit::{TorrentStats, TorrentStatsState};
use zerem_core::{Details, FileRow, PeerRow, Rate, Stall, State, TorrentId, TorrentRow, Transport};

/// How long a running download has to move nothing before the row says so.
///
/// Ten seconds, not one. Peers come and go and a piece can take a moment to
/// land; a client that announces "no one is sharing" during a two-second lull
/// is noise, and noise is what teaches people to stop reading the column. Long
/// enough that seeing it means something, short enough to be there when
/// someone goes looking for why nothing is happening.
const STILL_TICKS: u8 = 10;

/// What one torrent has been doing lately.
///
/// Everything here is state that spans ticks and therefore cannot live on a
/// row, which is one tick by definition: the smoothed rates, and how long the
/// transfer has been standing still. See [`zerem_core::rate`] for why the
/// figures are filtered at all.
#[derive(Clone, Copy, Default, Debug)]
pub struct Trend {
    down: Rate,
    up: Rate,
    still: u8,
}

impl Trend {
    /// Why this row is not moving, if it is not.
    ///
    /// Takes the row it is about to annotate rather than the raw stats: by
    /// this point the rate is the *published* one, and diagnosing off a raw
    /// figure that the column does not show would mean explaining a number
    /// nobody can see.
    fn stall(&mut self, row: &TorrentRow) -> Option<Stall> {
        // A magnet whose file list has not come back from the swarm. Said at
        // once rather than after a wait: it is the difference between "working
        // on it" and "broken", and it is the first thing anyone wants to know
        // after pasting a link.
        if row.size == 0 && row.is_active() {
            return Some(Stall::Metadata);
        }

        // Only a download can stall. Seeding with nobody to talk to is the
        // normal condition of a seed; hashing moves nothing over the network by
        // definition; paused and failed are not trying to move at all.
        if row.state != State::Downloading || row.down_bps > 0 {
            self.still = 0;
            return None;
        }

        self.still = self.still.saturating_add(1);
        if self.still < STILL_TICKS {
            return None;
        }
        // `peers_total` is what librqbit has ever seen, so it only grows: the
        // first arm is "nobody was ever found", not "nobody right now".
        Some(match (row.peers_total, row.peers_connected) {
            (0, _) => Stall::NoPeers,
            (_, 0) => Stall::Connecting,
            _ => Stall::Idle,
        })
    }
}

/// Build a domain row from one librqbit torrent.
///
/// `name` and `id` come from the handle rather than the stats, which carry
/// neither. The name arrives already shared and already folded, because it is
/// fixed for the torrent's life and this runs once per row per second.
pub fn to_row(
    id: TorrentId,
    name: &Arc<str>,
    name_key: &Arc<str>,
    stats: &TorrentStats,
    trend: &mut Trend,
) -> TorrentRow {
    let mut row = TorrentRow::shared(id, name.clone(), name_key.clone(), stats.total_bytes);
    row.done = stats.progress_bytes;
    row.state = state_of(stats);
    row.ratio_x100 = ratio_x100(stats.uploaded_bytes, stats.total_bytes);

    if row.state == State::Error {
        // Never the bare word "Error": without librqbit's own message there is
        // nothing for the user to act on. And never a bare `(os error 112)`
        // either — that is a fact about the kernel, not about the download.
        let raw = stats.error.as_deref().unwrap_or("stopped for an unreported reason");
        row.error = Some(Arc::from(zerem_core::explain(raw).unwrap_or(raw)));
    }

    if let Some(live) = &stats.live {
        // as_bytes(), not the raw `mbps` field — see the module note.
        row.down_bps = trend.down.update(live.download_speed.as_bytes());
        row.up_bps = trend.up.update(live.upload_speed.as_bytes());

        let peers = &live.snapshot.peer_stats;
        row.peers_connected = peers.live;
        // What librqbit has ever seen for this torrent, which is the number
        // every other client shows in brackets.
        row.peers_total = peers.seen;

        // From the *published* rate, not the raw one, so the estimate and the
        // speed beside it never contradict each other — and so the countdown
        // inherits the stability instead of jumping about on its own.
        row.eta = eta_secs(row.size, row.done, row.down_bps);
    } else {
        // Not live, so nothing is moving. Said explicitly rather than left
        // alone: the filters have to start clean for the next time it runs,
        // or resuming would ease up out of a speed from minutes ago.
        trend.down.update(0);
        trend.up.update(0);
    }

    // Last, because it reads the finished row: the state, the size, the
    // published rate and the peer counts all have to be settled first.
    row.stall = trend.stall(&row);
    row
}

/// The inside of one torrent: its files, and who it is talking to.
///
/// Built only for the torrent whose panel is open, which is what keeps this off
/// the per-tick bill for every other row.
///
/// `wanted` and `first` come from the caller rather than from the handle. While
/// a file is pinned the session is fetching that one alone, so the handle can
/// no longer say what the user actually ticked — and the panel has to draw the
/// intention, not the consequence.
// clippy asks for `PeerStatsFilter::default()`. That type cannot be named from
// outside librqbit, so inference through `Default::default()` is not a style
// choice here — it is the only way to call the method at all.
#[allow(clippy::default_trait_access)]
pub fn to_details(
    id: TorrentId,
    handle: &librqbit::ManagedTorrent,
    wanted: &[bool],
    first: &[bool],
) -> Details {
    let stats = handle.stats();

    // `file_progress` is positional against the metadata's file list, so the two
    // are read together or not at all.
    let files = handle
        .with_metadata(|meta| {
            meta.file_infos
                .iter()
                .enumerate()
                .map(|(i, info)| FileRow {
                    path: Arc::from(info.relative_filename.to_string_lossy().as_ref()),
                    size: info.len,
                    done: stats.file_progress.get(i).copied().unwrap_or(0),
                    // Defaulting to fetched: the lists are sized the moment the
                    // metadata lands, and a panel opened in that same tick must
                    // not draw every file as switched off.
                    wanted: wanted.get(i).copied().unwrap_or(true),
                    first: first.get(i).copied().unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();

    // Only a live torrent has peers; a paused one simply has none to show.
    let peers = handle.live().map_or_else(Vec::new, |live| {
        // `Default::default()` rather than `PeerStatsFilter::default()`: none of
        // these types can be *named* from outside the crate — `PeerStatsFilter`,
        // `PeerStatsSnapshot`, `PeerStats` and `ConnectionKind` are all reachable
        // only through inference, even though the methods returning them are
        // public. Upstream gap, worked around rather than blocked on.
        let snapshot = live.per_peer_stats_snapshot(Default::default());
        let mut peers: Vec<PeerRow> = snapshot
            .peers
            .into_iter()
            .map(|(addr, p)| PeerRow {
                addr: Arc::from(addr.as_str()),
                client: p.client_name.map(|c| Arc::from(c.as_str())),
                // Through `Display`, for the same reason: the enum cannot be
                // matched on by name from here.
                transport: transport_of(p.conn_kind.map(|k| k.to_string()).as_deref()),
                state: p.state,
                downloaded: p.counters.fetched_bytes,
                uploaded: p.counters.uploaded_bytes,
            })
            .collect();
        // A HashMap hands them over in a different order every tick, which would
        // make the list jump under the cursor. Sorted by address, so a peer
        // stays where the eye left it.
        peers.sort_unstable_by(|a, b| a.addr.cmp(&b.addr));
        peers
    });

    Details { id, files, peers }
}

/// librqbit's own `Display` strings, which are the only handle on this enum
/// from outside the crate.
fn transport_of(kind: Option<&str>) -> Transport {
    match kind {
        Some("tcp") => Transport::Tcp,
        Some("uTP") => Transport::Utp,
        Some("socks") => Transport::Socks,
        // Deliberately not a panic and not a silent Tcp: an upstream rename
        // should show as "unknown" in one column, not as a wrong answer
        // everywhere or a crash.
        _ => Transport::Unknown,
    }
}

/// Seconds until complete, or `None` when there is no honest estimate — which
/// is not the same as zero and must not sort like it.
fn eta_secs(size: u64, done: u64, down_bps: u64) -> Option<u32> {
    let remaining = size.checked_sub(done)?;
    if remaining == 0 || down_bps == 0 {
        return None;
    }
    Some(u32::try_from(remaining / down_bps).unwrap_or(u32::MAX))
}

/// librqbit reports "initializing" for both a fresh torrent and a re-hash, and
/// says nothing about seeding — a finished torrent is simply `Live` with
/// `finished: true`.
const fn state_of(stats: &TorrentStats) -> State {
    match stats.state {
        TorrentStatsState::Error => State::Error,
        TorrentStatsState::Paused => State::Paused,
        // `paused` here means it was added paused and has not started; either
        // way it is doing hash work, which is what the user sees.
        TorrentStatsState::Initializing { paused } => {
            if paused {
                State::Paused
            } else {
                State::Checking
            }
        }
        TorrentStatsState::Live if stats.finished => State::Seeding,
        TorrentStatsState::Live => State::Downloading,
    }
}

/// Share ratio in hundredths, saturating rather than dividing by zero: a magnet
/// whose metadata has not arrived reports `total_bytes: 0`.
fn ratio_x100(uploaded: u64, total: u64) -> u32 {
    uploaded.saturating_mul(100).checked_div(total).map_or(0, |r| u32::try_from(r).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::{eta_secs, ratio_x100, transport_of, Trend, STILL_TICKS};
    use zerem_core::{Stall, State, TorrentId, TorrentRow, Transport};

    /// A torrent that is running and has moved nothing.
    fn stuck(peers_total: u32, peers_connected: u32) -> TorrentRow {
        let mut row = TorrentRow::new(TorrentId(1), "x", 4_000_000_000);
        row.state = State::Downloading;
        row.peers_total = peers_total;
        row.peers_connected = peers_connected;
        row
    }

    /// Feed the same row until the rule has made up its mind.
    fn after(trend: &mut Trend, row: &TorrentRow, ticks: usize) -> Option<Stall> {
        (0..ticks).map(|_| trend.stall(row)).last().flatten()
    }

    #[test]
    fn a_lull_is_not_a_diagnosis() {
        // Peers come and go and a piece takes a moment to land. Announcing a
        // fault two seconds in is what teaches people to stop reading the
        // column at all.
        let mut trend = Trend::default();
        assert_eq!(after(&mut trend, &stuck(30, 4), usize::from(STILL_TICKS) - 1), None);
        assert_eq!(trend.stall(&stuck(30, 4)), Some(Stall::Idle), "and then it says so");
    }

    #[test]
    fn one_byte_arriving_clears_the_count() {
        // Otherwise a slow torrent accumulates its way to a fault it does not
        // have, purely because the ticks add up.
        let mut trend = Trend::default();
        after(&mut trend, &stuck(30, 4), usize::from(STILL_TICKS) - 1);

        let mut moving = stuck(30, 4);
        moving.down_bps = 1;
        assert_eq!(trend.stall(&moving), None);

        assert_eq!(after(&mut trend, &stuck(30, 4), 2), None, "the count started over");
    }

    #[test]
    fn the_reason_is_whichever_the_peer_counts_support() {
        // `peers_total` is what librqbit has ever seen, so nothing found means
        // nothing was *ever* found — not "nobody right now".
        let ticks = usize::from(STILL_TICKS);
        assert_eq!(after(&mut Trend::default(), &stuck(0, 0), ticks), Some(Stall::NoPeers));
        assert_eq!(after(&mut Trend::default(), &stuck(30, 0), ticks), Some(Stall::Connecting));
        assert_eq!(after(&mut Trend::default(), &stuck(30, 4), ticks), Some(Stall::Idle));
    }

    #[test]
    fn a_magnet_without_its_file_list_says_so_at_once() {
        // No waiting: it is the difference between "working on it" and
        // "broken", and it is what someone wants the second after pasting.
        let mut row = stuck(0, 0);
        row.size = 0;
        assert_eq!(Trend::default().stall(&row), Some(Stall::Metadata));
    }

    #[test]
    fn a_seed_with_nobody_to_talk_to_is_not_a_fault() {
        // It is the normal condition of a seed. Warning about it would put an
        // orange row under every finished torrent in the list.
        let mut row = stuck(0, 0);
        row.state = State::Seeding;
        row.done = row.size;
        assert_eq!(after(&mut Trend::default(), &row, 60), None);
    }

    #[test]
    fn hashing_and_stopping_are_never_diagnosed() {
        // Checking moves nothing over the network by definition, and a paused
        // torrent is not failing to move — it is not trying.
        let ticks = usize::from(STILL_TICKS) * 2;
        for state in [State::Checking, State::Paused, State::Error] {
            let mut row = stuck(0, 0);
            row.state = state;
            assert_eq!(after(&mut Trend::default(), &row, ticks), None, "{state:?}");
        }
    }

    #[test]
    fn no_rate_means_no_estimate_rather_than_zero() {
        // Zero would sort to the top of an ascending ETA column and read as
        // "about to finish", which is the opposite of the truth.
        assert_eq!(eta_secs(1000, 100, 0), None);
    }

    #[test]
    fn a_finished_torrent_has_no_estimate() {
        assert_eq!(eta_secs(1000, 1000, 500), None);
    }

    #[test]
    fn eta_is_the_remainder_over_the_rate() {
        assert_eq!(eta_secs(1000, 0, 100), Some(10));
        assert_eq!(eta_secs(1000, 500, 100), Some(5));
    }

    #[test]
    fn over_reported_progress_does_not_underflow() {
        // A subtraction here would wrap to ~18 exabytes and show an ETA of
        // several billion years.
        assert_eq!(eta_secs(1000, 1500, 100), None);
    }

    #[test]
    fn every_transport_string_librqbit_emits_is_recognised() {
        // Matched on `Display` output because the enum cannot be named from
        // outside librqbit. That makes these three strings a contract with a
        // crate that does not know it has one — so they get a test.
        assert_eq!(transport_of(Some("tcp")), Transport::Tcp);
        assert_eq!(transport_of(Some("uTP")), Transport::Utp);
        assert_eq!(transport_of(Some("socks")), Transport::Socks);
    }

    #[test]
    fn an_unrecognised_transport_is_unknown_rather_than_wrong() {
        // If upstream renames one, the column should say so — not quietly
        // claim every peer is on TCP.
        assert_eq!(transport_of(None), Transport::Unknown);
        assert_eq!(transport_of(Some("quic")), Transport::Unknown);
    }

    #[test]
    fn ratio_survives_metadata_that_has_not_arrived() {
        // A magnet reports total_bytes: 0 until the metadata is fetched.
        assert_eq!(ratio_x100(0, 0), 0);
        assert_eq!(ratio_x100(500, 0), 0);
    }

    #[test]
    fn ratio_is_uploaded_over_total() {
        assert_eq!(ratio_x100(0, 1000), 0);
        assert_eq!(ratio_x100(500, 1000), 50);
        assert_eq!(ratio_x100(1000, 1000), 100);
        assert_eq!(ratio_x100(2500, 1000), 250);
    }
}

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
use zerem_core::{Details, FileRow, PeerRow, Rate, State, TorrentId, TorrentRow, Transport};

/// One torrent's smoothed rates.
///
/// Held by the caller across ticks — a filter with no memory is not a filter,
/// and a row is one tick by definition. See [`zerem_core::rate`] for why the
/// figures are filtered at all.
#[derive(Clone, Copy, Default, Debug)]
pub struct Rates {
    down: Rate,
    up: Rate,
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
    rates: &mut Rates,
) -> TorrentRow {
    let mut row = TorrentRow::shared(id, name.clone(), name_key.clone(), stats.total_bytes);
    row.done = stats.progress_bytes;
    row.state = state_of(stats);
    row.ratio_x100 = ratio_x100(stats.uploaded_bytes, stats.total_bytes);

    if row.state == State::Error {
        // Never the bare word "Error": without librqbit's own message there is
        // nothing for the user to act on.
        row.error = Some(Arc::from(stats.error.as_deref().unwrap_or("stopped for an unreported reason")));
    }

    if let Some(live) = &stats.live {
        // as_bytes(), not the raw `mbps` field — see the module note.
        row.down_bps = rates.down.update(live.download_speed.as_bytes());
        row.up_bps = rates.up.update(live.upload_speed.as_bytes());

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
        rates.down.update(0);
        rates.up.update(0);
    }

    row
}

/// The inside of one torrent: its files, and who it is talking to.
///
/// Built only for the torrent whose panel is open, which is what keeps this off
/// the per-tick bill for every other row.
// clippy asks for `PeerStatsFilter::default()`. That type cannot be named from
// outside librqbit, so inference through `Default::default()` is not a style
// choice here — it is the only way to call the method at all.
#[allow(clippy::default_trait_access)]
pub fn to_details(id: TorrentId, handle: &librqbit::ManagedTorrent) -> Details {
    let stats = handle.stats();
    // `None` is librqbit's "no restriction", which means every file — not none
    // of them. Read the other way round, a torrent downloading normally would
    // be drawn with everything switched off.
    let only = handle.only_files();

    // `file_progress` is positional against the metadata's file list, so the two
    // are read together or not at all.
    let files = handle
        .with_metadata(|meta| {
            // A lookup, not a search: this runs once a second while the panel is
            // open, and `contains` inside the loop is quadratic in the file
            // count — which torrents do reach four figures of.
            let count = meta.file_infos.len();
            let wanted = only.as_deref().map_or_else(
                || vec![true; count],
                |list| {
                    let mut flags = vec![false; count];
                    for &i in list.iter().filter(|&&i| i < count) {
                        flags[i] = true;
                    }
                    flags
                },
            );
            meta.file_infos
                .iter()
                .enumerate()
                .map(|(i, info)| FileRow {
                    path: Arc::from(info.relative_filename.to_string_lossy().as_ref()),
                    size: info.len,
                    done: stats.file_progress.get(i).copied().unwrap_or(0),
                    wanted: wanted[i],
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
    use super::{eta_secs, ratio_x100, transport_of};
    use zerem_core::Transport;

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

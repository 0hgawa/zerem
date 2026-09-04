//! librqbit's `TorrentStats` → Zerem's `TorrentRow`.
//!
//! The point of the spike that survives it: this is the whole adapter between
//! the engine and the domain, and it either accounts for every state librqbit
//! can report or it silently loses one. It is written against `zerem-core`
//! directly so that when Phase 2 replaces `synthetic.rs`, this file moves over
//! as-is.
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
use zerem_core::{State, TorrentId, TorrentRow};

/// Build a domain row from one librqbit torrent.
///
/// `name` and `id` come from the handle rather than the stats, which carry
/// neither.
pub fn to_row(id: TorrentId, name: &str, stats: &TorrentStats) -> TorrentRow {
    let mut row = TorrentRow::new(id, name, stats.total_bytes);
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
        row.down_bps = live.download_speed.as_bytes();
        row.up_bps = live.upload_speed.as_bytes();

        let peers = &live.snapshot.peer_stats;
        row.peers_connected = peers.live;
        // What librqbit has ever seen for this torrent, which is the number
        // every other client shows in brackets.
        row.peers_total = peers.seen;

        row.eta = eta_secs(row.size, row.done, row.down_bps);
    }

    row
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
fn state_of(stats: &TorrentStats) -> State {
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
    uploaded
        .saturating_mul(100)
        .checked_div(total)
        .map_or(0, |r| u32::try_from(r).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::{eta_secs, ratio_x100};

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

//! View ordering.
//!
//! Sorting lives here, in Rust, and never in the `.slint`. A sort expression
//! written inside the UI is re-evaluated on every frame; done here it runs once
//! per tick, into a buffer that is reused rather than reallocated.
//!
//! Two things keep the cost down, and Phase 0 found both by measuring: the name
//! key is folded once at construction rather than inside the comparator
//! (3200 µs → 75 µs at 2000 rows), and a column whose key cannot change between
//! ticks is not re-sorted at all.

use std::cmp::Ordering;

use crate::torrent::TorrentRow;

/// How many columns the table has. The one place the count is written down.
pub const COLUMNS: usize = 8;

/// The column headings, in order.
///
/// Here rather than in the `.slint` for the reason every other string is: the
/// UI never invents text, and the header and the column menu have to name the
/// same eight things or one of them is lying.
pub const TITLES: [&str; COLUMNS] = ["Name", "Size", "State", "Down", "Up", "Peers", "ETA", "Ratio"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sort {
    pub col: usize,
    pub desc: bool,
}

impl Sort {
    pub const NAME: usize = 0;
    pub const SIZE: usize = 1;

    /// The direction a column takes when it is first clicked.
    ///
    /// Text reads naturally ascending; every numeric column is asked about
    /// because the user wants the top of it — biggest, fastest, most peers.
    #[must_use]
    pub const fn first_click(col: usize) -> Self {
        Self { col, desc: col != Self::NAME }
    }

    /// Clicking the active column flips it; clicking another adopts that
    /// column's natural direction.
    #[must_use]
    pub const fn clicked(self, col: usize) -> Self {
        if self.col == col {
            Self { col, desc: !self.desc }
        } else {
            Self::first_click(col)
        }
    }

    /// Whether this column's key can change while the app runs.
    ///
    /// Name and size are fixed for the life of a torrent, so an order sorted by
    /// either only needs rebuilding when the row set itself changes — which
    /// turns the default case from a sort per second into no work at all.
    ///
    /// It is also why [`Self::SIZE`] is the key behind the merged size-and-
    /// progress column: progress moves every tick, so sorting by it would cost
    /// a full re-sort per second for an order nobody asks for.
    #[must_use]
    pub const fn is_volatile(self) -> bool {
        !matches!(self.col, Self::NAME | Self::SIZE)
    }
}

impl Default for Sort {
    fn default() -> Self {
        Self::first_click(Self::NAME)
    }
}

/// `None` means "no estimate", which belongs at the bottom of an ascending
/// sort, not the top — the opposite of what `Option`'s own ordering gives.
const fn eta_key(t: &TorrentRow) -> u32 {
    match t.eta {
        Some(s) => s,
        None => u32::MAX,
    }
}

fn cmp_by(a: &TorrentRow, b: &TorrentRow, col: usize) -> Ordering {
    match col {
        0 => a.name_key.cmp(&b.name_key),
        1 => a.size.cmp(&b.size),
        2 => a.state.kind().cmp(&b.state.kind()),
        3 => a.down_bps.cmp(&b.down_bps),
        4 => a.up_bps.cmp(&b.up_bps),
        5 => a.peers_connected.cmp(&b.peers_connected),
        6 => eta_key(a).cmp(&eta_key(b)),
        _ => a.ratio_x100.cmp(&b.ratio_x100),
    }
}

/// Fill `out` with the display order.
///
/// `out` is passed in rather than returned so the caller keeps one buffer for
/// the life of the program. The id tie-break is always ascending, including
/// under `desc`: without it, rows holding equal keys — every paused torrent at
/// 0 B/s, say — would trade places between ticks and dirty the whole table for
/// nothing.
pub fn order(rows: &[TorrentRow], sort: Sort, out: &mut Vec<usize>) {
    out.clear();
    out.extend(0..rows.len());
    out.sort_unstable_by(|&a, &b| {
        let primary = cmp_by(&rows[a], &rows[b], sort.col);
        let primary = if sort.desc { primary.reverse() } else { primary };
        primary.then_with(|| rows[a].id.cmp(&rows[b].id))
    });
}

#[cfg(test)]
mod tests {
    use super::{order, Sort, COLUMNS};
    use crate::torrent::{State, TorrentId, TorrentRow};
    use std::sync::Arc;

    /// Names chosen so a raw byte sort and a case-insensitive one disagree.
    fn fixture() -> Vec<TorrentRow> {
        let mut rows: Vec<_> = ["Zebra", "apple", "Mango", "banana"]
            .iter()
            .enumerate()
            .map(|(i, n)| TorrentRow::new(TorrentId(i as u32), n, 1000 * (i as u64 + 1)))
            .collect();
        rows[0].state = State::Downloading;
        rows[0].down_bps = 900;
        rows[0].eta = Some(30);
        rows[2].state = State::Seeding;
        rows[2].up_bps = 400;
        rows
    }

    fn names(rows: &[TorrentRow], out: &[usize]) -> Vec<String> {
        out.iter().map(|&i| rows[i].name.to_string()).collect()
    }

    #[test]
    fn the_name_sort_ignores_case() {
        // The folded key is what makes this true. Raw bytes would put every
        // capitalised name before every lower-case one.
        let rows = fixture();
        let mut out = Vec::new();
        order(&rows, Sort::first_click(Sort::NAME), &mut out);
        assert_eq!(names(&rows, &out), ["apple", "banana", "Mango", "Zebra"]);
    }

    #[test]
    fn every_column_produces_a_permutation() {
        let rows = fixture();
        let mut out = Vec::new();
        for col in 0..COLUMNS {
            for desc in [false, true] {
                order(&rows, Sort { col, desc }, &mut out);
                let mut seen = out.clone();
                seen.sort_unstable();
                seen.dedup();
                assert_eq!(seen.len(), rows.len(), "column {col} dropped or duplicated a row");
            }
        }
    }

    #[test]
    fn equal_keys_keep_their_order_in_both_directions() {
        // Two of the four rows sit at 0 B/s, which is the tie the id break has
        // to resolve — otherwise they swap every tick and dirty the table.
        let rows = fixture();
        let (mut asc, mut desc) = (Vec::new(), Vec::new());
        order(&rows, Sort { col: 3, desc: false }, &mut asc);
        order(&rows, Sort { col: 3, desc: true }, &mut desc);

        let idle_asc: Vec<_> = asc.iter().copied().filter(|&i| rows[i].down_bps == 0).collect();
        let idle_desc: Vec<_> = desc.iter().copied().filter(|&i| rows[i].down_bps == 0).collect();
        assert_eq!(idle_asc, idle_desc);
    }

    #[test]
    fn no_estimate_sorts_below_every_estimate() {
        let rows = fixture();
        let mut out = Vec::new();
        order(&rows, Sort { col: 6, desc: false }, &mut out);
        assert_eq!(rows[out[0]].eta, Some(30), "the only estimate comes first");
        assert!(out[1..].iter().all(|&i| rows[i].eta.is_none()));
    }

    #[test]
    fn clicking_the_active_column_flips_it() {
        let s = Sort::default();
        assert_eq!(s.col, Sort::NAME);
        assert!(!s.desc, "text starts ascending");
        assert!(s.clicked(Sort::NAME).desc, "the same column flips");
        assert!(s.clicked(3).desc, "a numeric column starts descending");
        assert!(!s.clicked(3).clicked(Sort::NAME).desc, "back to text, ascending again");
    }

    #[test]
    fn only_columns_that_can_change_are_volatile() {
        assert!(!Sort::first_click(Sort::NAME).is_volatile());
        assert!(!Sort::first_click(Sort::SIZE).is_volatile());
        for col in 2..COLUMNS {
            assert!(Sort::first_click(col).is_volatile(), "column {col} changes every tick");
        }
    }
    /// Ten times the rows costs about eighteen times the work, not a hundred.
    ///
    /// A ratio and not a stopwatch, because a stopwatch on a shared CI runner
    /// measures the runner. Sorting is n log n, so ten times the input is
    /// 10 × log₂(10) ≈ 33 times the comparisons and, measured, about 18 times
    /// the wall clock. Quadratic would be a hundred. Thirty-five is the line:
    /// far enough above the truth to survive a noisy machine, far enough below
    /// a hundred to catch somebody putting a `contains` inside the comparator.
    ///
    /// That is the regression worth a test. Ten per cent slower is what the
    /// criterion benches in `benches/ordering.rs` are for, and it is not
    /// something CI can tell you.
    #[test]
    fn sorting_stays_n_log_n() {
        fn rows(count: usize) -> Vec<TorrentRow> {
            (0..count)
                .map(|i| {
                    let name = format!("Some.Release.Name.S{:02}E{:02}.1080p.WEB-DL.x265", i % 40, i % 24);
                    TorrentRow::shared(
                        TorrentId(i as u32),
                        Arc::from(name.as_str()),
                        Arc::from(name.to_lowercase().as_str()),
                        ((i * 7919) % 100_000) as u64,
                    )
                })
                .collect()
        }
        fn micros(rows: &[TorrentRow]) -> u128 {
            let mut out = Vec::with_capacity(rows.len());
            // Warm, so the first run's page faults are not the measurement.
            order(rows, Sort::default(), &mut out);
            let at = std::time::Instant::now();
            for _ in 0..3 {
                order(rows, Sort::default(), &mut out);
            }
            at.elapsed().as_micros().max(1)
        }

        let (small, large) = (rows(2_000), rows(20_000));
        let ratio = micros(&large) as f64 / micros(&small) as f64;
        assert!(
            ratio < 35.0,
            "ten times the rows cost {ratio:.0} times the work — sorting is no longer n log n"
        );
    }
}

//! View ordering.
//!
//! Sorting lives here, in Rust, and never in the `.slint`. A sort expression
//! written inside the UI is re-evaluated on every frame; done here it runs once
//! per tick, into a buffer that is reused rather than reallocated.
//!
//! Survives the spike — this is `zerem-core` material.

//! Two things keep the per-tick cost down, and both were found by measuring:
//! the name key is folded once at creation rather than inside the comparator,
//! and a column whose key cannot change between ticks is not re-sorted at all.

use std::cmp::Ordering;

use crate::fake::Torrent;

/// Column indices, matching the header order in `app.slint`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sort {
    pub col: usize,
    pub desc: bool,
}

impl Sort {
    pub const NAME: usize = 0;

    /// The direction a column should take when it is first clicked.
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
}

/// Whether a column's key can change while the app runs.
///
/// Name and size are fixed for the life of a torrent, so an order sorted by
/// either only needs rebuilding when the row set itself changes — which turns
/// the common case from a sort per second into no work at all.
#[must_use]
pub const fn is_volatile(col: usize) -> bool {
    !matches!(col, 0 | 1)
}

/// `None` means "no estimate", which belongs at the bottom of an ascending
/// sort, not the top — the opposite of what `Option`'s own ordering gives.
const fn eta_key(t: &Torrent) -> u32 {
    match t.eta {
        Some(s) => s,
        None => u32::MAX,
    }
}

fn cmp_by(a: &Torrent, b: &Torrent, col: usize) -> Ordering {
    match col {
        0 => a.name_key.cmp(&b.name_key),
        1 => a.size.cmp(&b.size),
        2 => a.progress_bp().cmp(&b.progress_bp()),
        3 => a.state.kind().cmp(&b.state.kind()),
        4 => a.down_bps.cmp(&b.down_bps),
        5 => a.up_bps.cmp(&b.up_bps),
        6 => a.peers_connected.cmp(&b.peers_connected),
        7 => eta_key(a).cmp(&eta_key(b)),
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
pub fn order(rows: &[Torrent], sort: Sort, out: &mut Vec<usize>) {
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
    use super::{is_volatile, order, Sort};
    use crate::fake::Session;

    #[test]
    fn the_name_sort_ignores_case() {
        // The folded key is what makes this true, and it is why the sort is a
        // byte comparison rather than a Unicode walk. Raw bytes would put every
        // capitalised name before every lower-case one.
        let s = Session::new(400);
        let mut out = Vec::new();
        order(&s.torrents, Sort::first_click(0), &mut out);
        for pair in out.windows(2) {
            let (a, b) = (&s.torrents[pair[0]], &s.torrents[pair[1]]);
            assert!(a.name.to_lowercase() <= b.name.to_lowercase(), "{} sorted before {}", a.name, b.name);
        }
    }

    #[test]
    fn only_columns_that_can_change_are_volatile() {
        // Name and size never move, so those sorts survive a tick untouched.
        assert!(!is_volatile(0));
        assert!(!is_volatile(1));
        for col in 2..=8 {
            assert!(is_volatile(col), "column {col} changes every tick");
        }
    }

    #[test]
    fn ordering_is_a_permutation_of_every_row() {
        let s = Session::new(500);
        let mut out = Vec::new();
        order(&s.torrents, Sort::first_click(4), &mut out);
        assert_eq!(out.len(), 500);
        let mut seen = out.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 500, "no row may be dropped or duplicated");
    }

    #[test]
    fn equal_keys_keep_a_stable_order_in_both_directions() {
        // Most rows are paused at 0 B/s, so the download column is almost all
        // ties — exactly the case that would churn the table if unstable.
        let s = Session::new(400);
        let (mut asc, mut desc) = (Vec::new(), Vec::new());
        order(&s.torrents, Sort { col: 4, desc: false }, &mut asc);
        order(&s.torrents, Sort { col: 4, desc: false }, &mut desc);
        assert_eq!(asc, desc, "the same sort must give the same order twice");

        order(&s.torrents, Sort { col: 4, desc: true }, &mut desc);
        let idle_asc: Vec<_> = asc.iter().filter(|&&i| s.torrents[i].down_bps == 0).collect();
        let idle_desc: Vec<_> = desc.iter().filter(|&&i| s.torrents[i].down_bps == 0).collect();
        assert_eq!(idle_asc, idle_desc, "tied rows keep their order when reversed");
    }

    #[test]
    fn no_estimate_sorts_below_every_estimate() {
        let s = Session::new(300);
        let mut out = Vec::new();
        order(&s.torrents, Sort { col: 7, desc: false }, &mut out);
        let first_none = out.iter().position(|&i| s.torrents[i].eta.is_none());
        let last_some = out.iter().rposition(|&i| s.torrents[i].eta.is_some());
        if let (Some(n), Some(sm)) = (first_none, last_some) {
            assert!(sm < n, "every estimate must come before the first ∞");
        }
    }

    #[test]
    fn clicking_the_active_column_flips_it() {
        let s = Sort::first_click(0);
        assert!(!s.desc, "text starts ascending");
        assert!(s.clicked(0).desc, "same column flips");
        assert!(s.clicked(4).desc, "a numeric column starts descending");
        assert!(!s.clicked(4).clicked(0).desc, "back to text, ascending again");
    }
}

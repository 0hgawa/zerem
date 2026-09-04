//! The table model.
//!
//! The obvious implementation rebuilds a `VecModel` every tick. It reallocates
//! the whole list, drops the scroll position and loses the selection, and costs
//! the same whether one row moved or all of them did.
//!
//! This one keeps two things per row: the domain value, cheap to compare and
//! allocating nothing, and the formatted row the UI draws. A tick compares the
//! domain values, and only a row that actually changed is re-formatted and
//! notified — `row_changed(i)`, never a wholesale reset.
//!
//! Phase 0 measured the two constants that govern it: **comparing a row costs
//! ~0.03 µs, formatting one costs ~1.1 µs.** Comparing is free; formatting is
//! the bill. A realistic tick over 2000 rows came to 92 µs.

use std::cell::RefCell;
use std::collections::HashSet;
use std::time::Instant;

use slint::{Model, ModelNotify, ModelTracker};
use zerem_core::{fmt, TorrentId, TorrentRow};

use crate::Row;

/// One row: what the engine said, and what the UI draws.
struct Entry {
    src: TorrentRow,
    view: Row,
}

/// What one `apply` cost.
#[derive(Clone, Copy, Default, Debug)]
pub struct ApplyStats {
    pub micros: u64,
    pub changed: usize,
    /// Set when the row set itself changed and the list had to be rebuilt.
    /// False on every ordinary tick — a reset is what costs the scroll position.
    pub reset: bool,
}

pub struct TorrentModel {
    entries: RefCell<Vec<Entry>>,
    /// Indices to notify, collected during the diff and drained once the borrow
    /// is released. Kept across ticks so the steady state allocates nothing.
    dirty: RefCell<Vec<usize>>,
    notify: ModelNotify,
}

impl TorrentModel {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
            dirty: RefCell::new(Vec::new()),
            notify: ModelNotify::default(),
        }
    }

    /// Apply a snapshot's rows, in `order`, with `selected` marking the chosen
    /// ids.
    pub fn apply(&self, rows: &[TorrentRow], order: &[usize], selected: &HashSet<TorrentId>) -> ApplyStats {
        let started = Instant::now();

        if self.entries.borrow().len() == order.len() {
            let changed = self.diff(rows, order, selected);
            // Notified outside the borrow: a peer may read `row_data` straight
            // back, and doing that under the mutable borrow is a panic.
            for &i in self.dirty.borrow().iter() {
                self.notify.row_changed(i);
            }
            return ApplyStats { micros: started.elapsed().as_micros() as u64, changed, reset: false };
        }

        self.rebuild(rows, order, selected);
        self.notify.reset();
        ApplyStats { micros: started.elapsed().as_micros() as u64, changed: order.len(), reset: true }
    }

    /// Compare in place, recording which rows moved. Returns the count.
    fn diff(&self, rows: &[TorrentRow], order: &[usize], selected: &HashSet<TorrentId>) -> usize {
        let mut entries = self.entries.borrow_mut();
        let mut dirty = self.dirty.borrow_mut();
        dirty.clear();

        for (i, &src_index) in order.iter().enumerate() {
            let next = &rows[src_index];
            let entry = &mut entries[i];
            let is_selected = selected.contains(&next.id);
            // The domain comparison is the cheap one — integers and one
            // pointer-equal `Arc<str>`. Formatting only happens past it.
            if entry.src == *next && entry.view.selected == is_selected {
                continue;
            }
            entry.src = next.clone();
            entry.view = build(next, is_selected);
            dirty.push(i);
        }
        dirty.len()
    }

    fn rebuild(&self, rows: &[TorrentRow], order: &[usize], selected: &HashSet<TorrentId>) {
        let mut entries = self.entries.borrow_mut();
        entries.clear();
        entries.reserve(order.len());
        for &src_index in order {
            let src = &rows[src_index];
            let view = build(src, selected.contains(&src.id));
            entries.push(Entry { src: src.clone(), view });
        }
    }

    /// The id at a view position — the only correct way to get from a click to
    /// a torrent. A view index is meaningless the moment the sort changes.
    #[must_use]
    pub fn id_at(&self, view_index: usize) -> Option<TorrentId> {
        self.entries.borrow().get(view_index).map(|e| e.src.id)
    }

    /// Whether every id in `selection` is currently running. Decides what the
    /// toolbar's single button offers.
    #[must_use]
    pub fn all_running(&self, selection: &HashSet<TorrentId>) -> bool {
        !selection.is_empty()
            && self
                .entries
                .borrow()
                .iter()
                .filter(|e| selection.contains(&e.src.id))
                .all(|e| e.src.is_active())
    }
}

impl Default for TorrentModel {
    fn default() -> Self {
        Self::new()
    }
}

impl Model for TorrentModel {
    type Data = Row;

    fn row_count(&self) -> usize {
        self.entries.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        self.entries.borrow().get(row).map(|e| e.view.clone())
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

/// Domain row → the strings the UI draws. The only place formatting happens,
/// and it runs once per changed row per tick rather than once per row per frame.
fn build(t: &TorrentRow, selected: bool) -> Row {
    Row {
        id: t.id.0 as i32,
        name: t.name.as_ref().into(),
        content: t.content.kind(),
        progress: t.progress_bp() as f32 / 10_000.0,
        progress_text: fmt::progress(t.done, t.size).into(),
        state: t.status_text().into(),
        kind: t.state.kind(),
        active: t.is_active(),
        down: fmt::speed(t.down_bps).into(),
        up: fmt::speed(t.up_bps).into(),
        peers: fmt::peers(t.peers_connected, t.peers_total).into(),
        eta: fmt::eta(t.eta).into(),
        ratio: fmt::ratio(t.ratio_x100).into(),
        selected,
    }
}

#[cfg(test)]
mod tests {
    use super::TorrentModel;
    use slint::Model;
    use std::collections::HashSet;
    use zerem_core::{sort, Sort, State, TorrentId, TorrentRow};

    fn rows(n: u32) -> Vec<TorrentRow> {
        (0..n)
            .map(|i| {
                let mut t = TorrentRow::new(TorrentId(i), &format!("torrent {i:04}"), 1_000_000);
                if i % 10 == 0 {
                    t.state = State::Downloading;
                    t.down_bps = 1000 + u64::from(i);
                }
                t
            })
            .collect()
    }

    fn fixture(n: u32) -> (Vec<TorrentRow>, Vec<usize>, TorrentModel) {
        let rows = rows(n);
        let mut order = Vec::new();
        sort::order(&rows, Sort::default(), &mut order);
        let model = TorrentModel::new();
        model.apply(&rows, &order, &HashSet::new());
        (rows, order, model)
    }

    #[test]
    fn an_unchanged_snapshot_dirties_nothing() {
        let (rows, order, model) = fixture(300);
        let stats = model.apply(&rows, &order, &HashSet::new());
        assert_eq!(stats.changed, 0, "re-applying identical data must be free");
        assert!(!stats.reset);
    }

    #[test]
    fn a_tick_dirties_exactly_the_rows_that_moved() {
        let (mut rows, order, model) = fixture(1000);
        rows[3].down_bps += 1;
        rows[7].up_bps += 1;
        let stats = model.apply(&rows, &order, &HashSet::new());
        assert_eq!(stats.changed, 2);
        assert!(!stats.reset, "an ordinary tick must never reset the model");
    }

    #[test]
    fn selecting_dirties_only_the_selected_row() {
        let (rows, order, model) = fixture(400);
        let id = model.id_at(7).expect("row 7 exists");
        let selected: HashSet<TorrentId> = std::iter::once(id).collect();
        let stats = model.apply(&rows, &order, &selected);
        assert_eq!(stats.changed, 1);
        assert!(model.row_data(7).expect("row 7").selected);
    }

    #[test]
    fn a_changed_row_count_rebuilds() {
        let (mut rows, mut order, model) = fixture(300);
        rows.truncate(280);
        sort::order(&rows, Sort::default(), &mut order);
        let stats = model.apply(&rows, &order, &HashSet::new());
        assert!(stats.reset, "adding or removing torrents is the one reset case");
        assert_eq!(model.row_count(), 280);
    }

    #[test]
    fn reordering_maps_ids_to_their_new_positions() {
        let (mut rows, mut order, model) = fixture(300);
        rows[42].size = u64::MAX;
        sort::order(&rows, Sort { col: Sort::SIZE, desc: true }, &mut order);
        model.apply(&rows, &order, &HashSet::new());
        assert_eq!(model.id_at(0), Some(TorrentId(42)), "the largest is on top");
    }

    #[test]
    fn the_toolbar_only_offers_pause_when_everything_picked_is_running() {
        let (rows, order, model) = fixture(100);
        model.apply(&rows, &order, &HashSet::new());

        let running: HashSet<TorrentId> = std::iter::once(TorrentId(0)).collect();
        assert!(model.all_running(&running));

        let mixed: HashSet<TorrentId> = [TorrentId(0), TorrentId(1)].into_iter().collect();
        assert!(!model.all_running(&mixed), "one paused torrent in the set is enough");

        assert!(!model.all_running(&HashSet::new()), "an empty selection runs nothing");
    }
}

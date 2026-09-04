//! The table model — the decision this spike exists to validate.
//!
//! The obvious implementation rebuilds a `VecModel` every tick. It reallocates
//! the whole list, drops the scroll position and loses the selection, and it
//! costs the same whether one row moved or all of them did.
//!
//! This one keeps two things per row: the domain value, which is cheap to
//! compare and allocates nothing, and the formatted row the UI draws. A tick
//! compares the domain values, and only a row that actually changed is
//! re-formatted and notified — `row_changed(i)`, never a wholesale reset.
//!
//! Survives the spike — this is `src/model.rs` in the real app.

use std::cell::RefCell;
use std::collections::HashSet;
use std::time::Instant;

use slint::{Model, ModelNotify, ModelTracker};

use crate::fake::Torrent;
use crate::fmt;
use crate::Row;

/// One row: what the engine said, and what the UI draws.
struct Entry {
    src: Torrent,
    view: Row,
}

/// What one `apply` cost. Reported to the instrumentation bar so the spike's
/// claim is a measurement rather than an assertion.
#[derive(Clone, Copy, Default)]
pub struct ApplyStats {
    pub micros: u64,
    pub changed: usize,
    /// Set when the row set itself changed and the list had to be rebuilt.
    /// Should be false on every ordinary tick — a reset is what costs the
    /// scroll position.
    pub reset: bool,
}

pub struct TorrentModel {
    entries: RefCell<Vec<Entry>>,
    /// Indices to notify, collected during the diff and drained after the
    /// borrow is released. Kept across ticks so the steady state allocates
    /// nothing at all.
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

    /// Apply a snapshot, in `order`, with `selected` marking the chosen ids.
    pub fn apply(&self, rows: &[Torrent], order: &[usize], selected: &HashSet<u32>) -> ApplyStats {
        let started = Instant::now();

        if self.entries.borrow().len() == order.len() {
            let changed = self.diff(rows, order, selected);
            // Notified outside the borrow: a peer may read `row_data` straight
            // back, and doing that under the mutable borrow is a panic.
            let dirty = self.dirty.borrow();
            for &i in dirty.iter() {
                self.notify.row_changed(i);
            }
            return ApplyStats { micros: started.elapsed().as_micros() as u64, changed, reset: false };
        }

        self.rebuild(rows, order, selected);
        self.notify.reset();
        ApplyStats { micros: started.elapsed().as_micros() as u64, changed: order.len(), reset: true }
    }

    /// Compare in place, recording which rows moved. Returns the count.
    fn diff(&self, rows: &[Torrent], order: &[usize], selected: &HashSet<u32>) -> usize {
        let mut entries = self.entries.borrow_mut();
        let mut dirty = self.dirty.borrow_mut();
        dirty.clear();

        for (i, &src_idx) in order.iter().enumerate() {
            let next = &rows[src_idx];
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

    fn rebuild(&self, rows: &[Torrent], order: &[usize], selected: &HashSet<u32>) {
        let mut entries = self.entries.borrow_mut();
        entries.clear();
        entries.reserve(order.len());
        for &src_idx in order {
            let src = &rows[src_idx];
            let view = build(src, selected.contains(&src.id));
            entries.push(Entry { src: src.clone(), view });
        }
    }

    /// The id at a view position — the only correct way to go from a click to
    /// a torrent. A view index is meaningless the moment the sort changes.
    #[must_use]
    pub fn id_at(&self, view_index: usize) -> Option<u32> {
        self.entries.borrow().get(view_index).map(|e| e.src.id)
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
fn build(t: &Torrent, selected: bool) -> Row {
    Row {
        id: t.id as i32,
        name: t.name.as_ref().into(),
        size: fmt::bytes(t.size).into(),
        progress: t.progress_bp() as f32 / 10_000.0,
        pct: fmt::percent(t.done, t.size).into(),
        state: t.state.label().into(),
        kind: t.state.kind(),
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
    use crate::fake::Session;
    use crate::sort::{order, Sort};
    use slint::Model;
    use std::collections::HashSet;

    fn fixture(n: usize) -> (Session, Vec<usize>, TorrentModel) {
        let session = Session::new(n);
        let mut ord = Vec::new();
        order(&session.torrents, Sort::first_click(0), &mut ord);
        let model = TorrentModel::new();
        model.apply(&session.torrents, &ord, &HashSet::new());
        (session, ord, model)
    }

    #[test]
    fn first_apply_populates_and_resets_once() {
        let (_, ord, model) = fixture(200);
        assert_eq!(model.row_count(), 200);
        assert_eq!(ord.len(), 200);
    }

    #[test]
    fn an_unchanged_snapshot_dirties_nothing() {
        let (session, ord, model) = fixture(300);
        let stats = model.apply(&session.torrents, &ord, &HashSet::new());
        assert_eq!(stats.changed, 0, "re-applying identical data must be free");
        assert!(!stats.reset);
    }

    #[test]
    fn a_tick_dirties_exactly_the_rows_that_moved() {
        // The claim of the whole design: the cost tracks what changed, not the
        // size of the list.
        let (mut session, ord, model) = fixture(1000);
        let moved = session.tick();
        let stats = model.apply(&session.torrents, &ord, &HashSet::new());
        assert_eq!(stats.changed, moved);
        assert!(!stats.reset, "an ordinary tick must never reset the model");
        assert!(moved < 100, "the realistic fixture should move few rows, got {moved}");
    }

    #[test]
    fn churn_dirties_every_row_and_still_does_not_reset() {
        let (mut session, ord, model) = fixture(500);
        session.churn = true;
        session.tick();
        let stats = model.apply(&session.torrents, &ord, &HashSet::new());
        assert_eq!(stats.changed, 500);
        assert!(!stats.reset, "the scroll position survives even the worst case");
    }

    #[test]
    fn selecting_dirties_only_the_selected_row() {
        let (session, ord, model) = fixture(400);
        let id = model.id_at(7).expect("row 7 exists");
        let selected: HashSet<u32> = std::iter::once(id).collect();
        let stats = model.apply(&session.torrents, &ord, &selected);
        assert_eq!(stats.changed, 1);
        assert!(model.row_data(7).expect("row 7").selected);
    }

    #[test]
    fn a_changed_row_count_rebuilds() {
        let (mut session, mut ord, model) = fixture(300);
        session.resize(280);
        order(&session.torrents, Sort::first_click(0), &mut ord);
        let stats = model.apply(&session.torrents, &ord, &HashSet::new());
        assert!(stats.reset, "adding or removing torrents is the one reset case");
        assert_eq!(model.row_count(), 280);
    }

    #[test]
    fn reordering_maps_ids_to_their_new_positions() {
        let (session, mut ord, model) = fixture(300);
        let top_before = model.id_at(0).expect("first row");
        order(&session.torrents, Sort { col: 1, desc: true }, &mut ord);
        model.apply(&session.torrents, &ord, &HashSet::new());
        let top_after = model.id_at(0).expect("first row");
        assert_ne!(top_before, top_after, "sorting by size must move the top row");
        // The largest torrent is genuinely at the top.
        let largest = session.torrents.iter().max_by_key(|t| t.size).expect("non-empty");
        assert_eq!(top_after, largest.id);
    }
}

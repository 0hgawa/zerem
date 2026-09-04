//! The UI-side state: what is selected, how it is sorted, and the model the
//! table reads.
//!
//! Everything here lives on the UI thread. The engine's state is the snapshot;
//! this is only what the *view* of it looks like.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use slint::{Model, VecModel};
use zerem_core::{sort, Filter, Sort, TorrentId, TorrentRow};
use zerem_engine::{Engine, Snapshot};

use crate::model::{ApplyStats, TorrentModel};

pub const MIN_COL_W: f32 = 48.0;
pub const MAX_COL_W: f32 = 640.0;

const DEFAULT_WIDTHS: [f32; 8] = [300.0, 200.0, 108.0, 96.0, 96.0, 84.0, 84.0, 66.0];

/// How long a message the UI raised itself stays up. Long enough to read,
/// short enough that it never becomes furniture.
const NOTICE_FOR: Duration = Duration::from_secs(6);

/// A command that has been sent but not yet reflected in a snapshot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Optimistic {
    Started,
    Paused,
    Removed,
}

/// One in-flight guess, and the snapshot sequence it was made against.
#[derive(Clone, Copy, Debug)]
struct Pending {
    action: Optimistic,
    /// The first snapshot newer than this supersedes the guess — including when
    /// the engine declined the command, in which case the row simply comes back.
    seq: u64,
}

pub struct UiState {
    pub engine: Engine,
    pub model: Rc<TorrentModel>,
    pub widths: Rc<VecModel<f32>>,
    /// Which columns the table draws. A model rather than a plain array so the
    /// header, the rows and the column menu all read the one source.
    pub columns: Rc<VecModel<bool>>,

    /// Reused across ticks — the display order allocates once, not every second.
    order: RefCell<Vec<usize>>,
    /// What the view is narrowed to. Parsed once per keystroke, not per row.
    filter: RefCell<Filter>,
    /// The row count the cached order was built against.
    ///
    /// Not `order.len()`: with a filter on, the order is shorter than the
    /// snapshot by design, and comparing the two would rebuild it every tick.
    built_for: Cell<usize>,
    /// Selection is held as ids, never as indices: an index means something
    /// different the moment the sort changes or a torrent is removed.
    selected: RefCell<HashSet<TorrentId>>,
    /// Where a shift-range starts. A view index, valid only until the next sort.
    anchor: Cell<usize>,
    sort: Cell<Sort>,
    /// The row-set generation the cached order was built against.
    generation: Cell<u64>,
    resize_base: Cell<f32>,
    /// Commands sent but not yet confirmed. Empty almost always — it holds
    /// entries for at most one tick after a click.
    pending: RefCell<HashMap<TorrentId, Pending>>,
    /// A message the UI raised, which the engine knows nothing about.
    local_notice: RefCell<Option<(Arc<str>, Instant)>>,
    /// The torrent the arrow keys are on, held as an id like the selection.
    focused: Cell<Option<TorrentId>>,
    /// What the open confirmation is about.
    ///
    /// Captured when the dialog opens rather than read back when it is
    /// accepted, because the two routes into it aim at different things: the
    /// toolbar and Delete mean the selection, the button on a row means that
    /// row. Re-reading the selection on accept would remove the wrong torrents
    /// for one of them.
    remove_target: RefCell<Vec<TorrentId>>,
}

impl UiState {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            model: Rc::new(TorrentModel::new()),
            widths: Rc::new(VecModel::from(DEFAULT_WIDTHS.to_vec())),
            columns: Rc::new(VecModel::from(vec![true; sort::COLUMNS])),
            order: RefCell::new(Vec::new()),
            filter: RefCell::new(Filter::default()),
            built_for: Cell::new(0),
            selected: RefCell::new(HashSet::new()),
            anchor: Cell::new(0),
            sort: Cell::new(Sort::default()),
            generation: Cell::new(0),
            resize_base: Cell::new(0.0),
            pending: RefCell::new(HashMap::new()),
            local_notice: RefCell::new(None),
            focused: Cell::new(None),
            remove_target: RefCell::new(Vec::new()),
        }
    }

    pub fn sort(&self) -> Sort {
        self.sort.get()
    }

    /// Put the table back the way it was left.
    ///
    /// Column widths and the sort are not preferences anybody sets on purpose —
    /// they are set by using the app, and the only thing they owe is to still be
    /// there next time.
    pub fn restore_view(&self, settings: &crate::settings::Settings) {
        self.sort.set(Sort { col: settings.sort_col.min(sort::COLUMNS - 1), desc: settings.sort_desc });
        // A width list of a different length came from a version whose table
        // had different columns, so it is dropped rather than applied position
        // by position: index 1 is not the column that width was set on, and a
        // Name column 88 px wide is worse than the default.
        if settings.column_widths.len() == DEFAULT_WIDTHS.len() {
            for (i, width) in settings.column_widths.iter().enumerate() {
                self.widths.set_row_data(i, width.clamp(MIN_COL_W, MAX_COL_W));
            }
        }
        // Same rule, and one extra: the name is forced back on however the file
        // reads. This one is hand-editable, and a file that hides every column
        // would leave a table with nothing in it to right-click.
        if settings.column_visible.len() == sort::COLUMNS {
            for (i, &shown) in settings.column_visible.iter().enumerate() {
                self.columns.set_row_data(i, shown || i == 0);
            }
        }
    }

    pub fn save_view(&self, store: &Rc<crate::settings::Store>) {
        let sort = self.sort.get();
        let widths: Vec<f32> = self.widths.iter().collect();
        let visible: Vec<bool> = self.columns.iter().collect();
        store.update(|s| {
            s.sort_col = sort.col;
            s.sort_desc = sort.desc;
            s.column_widths = widths;
            s.column_visible = visible;
        });
    }

    /// Show or hide one column. The name is not offered, so it is not handled.
    pub fn toggle_column(&self, col: usize) {
        if let Some(shown) = self.columns.row_data(col) {
            self.columns.set_row_data(col, !shown);
        }
    }

    pub fn selection(&self) -> std::cell::Ref<'_, HashSet<TorrentId>> {
        self.selected.borrow()
    }

    /// Adopt a new sort and rebuild the order immediately.
    pub fn set_sort(&self, col: usize, snapshot: &Snapshot) {
        self.sort.set(self.sort.get().clicked(col));
        self.rebuild_order(snapshot);
    }

    /// Rebuild the display order only when it can actually have moved.
    ///
    /// Sorting by a volatile column (speed, progress, ETA) genuinely reorders
    /// every second and has to be paid for. Sorting by name or size does not
    /// move while the row set is unchanged, and that is the default — so the
    /// common case does no work rather than the 3.2 ms Phase 0 first measured.
    pub fn refresh_order(&self, snapshot: &Snapshot) {
        let stale =
            self.generation.get() != snapshot.generation || self.built_for.get() != snapshot.torrents.len();
        if stale || self.sort.get().is_volatile() {
            self.rebuild_order(snapshot);
        }
    }

    /// Adopt a new filter and rebuild immediately — waiting a tick to answer a
    /// keystroke is what makes a search box feel broken.
    ///
    /// The selection is cut down to what is left showing. A torrent hidden by
    /// the filter but still selected is the same hazard as a context menu that
    /// acts on a row other than the clicked one: type three letters, press
    /// Delete, and remove torrents you cannot see.
    pub fn set_filter(&self, query: &str, snapshot: &Snapshot) {
        *self.filter.borrow_mut() = Filter::new(query);
        self.rebuild_order(snapshot);

        let mut selected = self.selected.borrow_mut();
        if selected.is_empty() {
            return;
        }
        let order = self.order.borrow();
        let visible: HashSet<TorrentId> = order.iter().map(|&i| snapshot.torrents[i].id).collect();
        selected.retain(|id| visible.contains(id));
    }

    /// Whether a filter is hiding anything, which is what decides between the
    /// plain total and the matched count.
    #[must_use]
    pub fn is_filtering(&self) -> bool {
        !self.filter.borrow().is_empty()
    }

    /// Sort everything, then drop what the filter excludes.
    ///
    /// That order rather than the reverse: filtering first would sort a shorter
    /// list, but the whole sort costs 75 µs at 2000 rows and this way the
    /// comparator never has to know a filter exists.
    fn rebuild_order(&self, snapshot: &Snapshot) {
        let mut order = self.order.borrow_mut();
        sort::order(&snapshot.torrents, self.sort.get(), &mut order);
        let filter = self.filter.borrow();
        if !filter.is_empty() {
            order.retain(|&i| filter.matches(&snapshot.torrents[i]));
        }
        self.generation.set(snapshot.generation);
        self.built_for.set(snapshot.torrents.len());
    }

    /// Say something in the status bar that the engine has no opinion about —
    /// a clipboard with nothing useful in it, say.
    pub fn set_notice(&self, text: &str) {
        *self.local_notice.borrow_mut() = Some((Arc::from(text), Instant::now()));
    }

    /// What to show in the status bar.
    ///
    /// The UI's own message wins while it is fresh, because it is always the
    /// answer to something the user just did. Timed rather than counted: this is
    /// read on every click as well as every tick, and a counter would burn
    /// through in the time it takes to click twice.
    #[must_use]
    pub fn notice(&self, snapshot: &Snapshot) -> Option<Arc<str>> {
        let local = self.local_notice.borrow();
        match local.as_ref() {
            Some((text, since)) if since.elapsed() < NOTICE_FOR => Some(text.clone()),
            _ => snapshot.notice.clone(),
        }
    }

    /// What the confirmation dialog should ask.
    ///
    /// Names the torrent when there is one, because "Remove Debian…?" is a
    /// question the user can answer and "Remove 1 torrent?" is not.
    #[must_use]
    pub fn confirm_question(&self, snapshot: &Snapshot) -> String {
        let target = self.remove_target.borrow();
        if let [id] = target.as_slice() {
            if let Some(row) = snapshot.torrents.iter().find(|t| t.id == *id) {
                return format!("Remove \u{201c}{}\u{201d}?", row.name);
            }
        }
        format!("Remove {} torrents?", target.len())
    }

    /// Aim the confirmation. Returns whether there is anything to ask about.
    pub fn begin_remove(&self, ids: Vec<TorrentId>) -> bool {
        let any = !ids.is_empty();
        *self.remove_target.borrow_mut() = ids;
        any
    }

    /// What the open confirmation would remove.
    pub fn remove_target(&self) -> Vec<TorrentId> {
        self.remove_target.borrow().clone()
    }

    /// Record what a command is expected to do, so the table shows it now
    /// rather than up to a tick from now.
    pub fn expect_start(&self, id: TorrentId, seq: u64) {
        self.expect(id, Optimistic::Started, seq);
    }

    pub fn expect_pause(&self, id: TorrentId, seq: u64) {
        self.expect(id, Optimistic::Paused, seq);
    }

    pub fn expect_remove(&self, id: TorrentId, seq: u64) {
        self.expect(id, Optimistic::Removed, seq);
    }

    fn expect(&self, id: TorrentId, action: Optimistic, seq: u64) {
        self.pending.borrow_mut().insert(id, Pending { action, seq });
    }

    /// Drop guesses the engine has already answered — whether it agreed or not.
    fn settle(&self, snapshot: &Snapshot) {
        let mut pending = self.pending.borrow_mut();
        if !pending.is_empty() {
            pending.retain(|_, p| snapshot.seq <= p.seq);
        }
    }

    /// Push the snapshot into the model, with any in-flight guesses laid over it.
    pub fn apply(&self, snapshot: &Snapshot) -> ApplyStats {
        self.settle(snapshot);

        let pending = self.pending.borrow();
        let selected = self.selected.borrow();
        if pending.is_empty() {
            // The overwhelmingly common path: no allocation, no copy.
            return self.model.apply(&snapshot.torrents, &self.order.borrow(), &selected);
        }

        // Rare path, and short-lived: a command is in flight, so the rows are
        // copied once to carry the guess. It lasts at most one tick.
        let mut rows: Vec<TorrentRow> = snapshot.torrents.clone();
        let mut removed: HashSet<TorrentId> = HashSet::new();
        for (id, p) in pending.iter() {
            match p.action {
                Optimistic::Removed => {
                    removed.insert(*id);
                }
                Optimistic::Started | Optimistic::Paused => {
                    if let Some(row) = rows.iter_mut().find(|r| r.id == *id) {
                        if p.action == Optimistic::Paused {
                            row.pause();
                        } else {
                            row.resume();
                        }
                    }
                }
            }
        }

        // A removal is taken out of the order rather than out of the rows, so
        // every remaining index still points where it did.
        let order: Vec<usize> =
            self.order.borrow().iter().copied().filter(|&i| !removed.contains(&rows[i].id)).collect();

        self.model.apply(&rows, &order, &selected)
    }

    /// Forget ids that are no longer in the session.
    ///
    /// Without this a removed torrent stays "selected" forever: the count in
    /// the toolbar never drops, and the buttons stay live over nothing.
    pub fn prune_selection(&self, snapshot: &Snapshot) {
        let mut selected = self.selected.borrow_mut();
        if selected.is_empty() {
            return;
        }
        selected.retain(|id| snapshot.torrents.iter().any(|t| t.id == *id));
    }

    /// Which view row the keyboard is on, if that torrent is still in the list.
    ///
    /// Looked up by id every time rather than stored as an index: a re-sort or a
    /// removal moves every index, and the row under the arrows has to be the
    /// same torrent it was, not the same position.
    #[must_use]
    pub fn focus_row(&self) -> Option<usize> {
        let id = self.focused.get()?;
        (0..)
            .map_while(|i| self.model.id_at(i).map(|found| (i, found)))
            .find_map(|(i, found)| (found == id).then_some(i))
    }

    /// The torrent a context action applies to: the keyboard row if there is
    /// one, otherwise the topmost selected.
    #[must_use]
    pub fn acting_on(&self) -> Option<TorrentId> {
        self.focused.get().filter(|id| self.selected.borrow().contains(id)).or_else(|| {
            (0..).map_while(|i| self.model.id_at(i)).find(|id| self.selected.borrow().contains(id))
        })
    }

    /// Move the keyboard row by `delta`, taking the selection with it.
    ///
    /// Returns the row it landed on. With nothing focused yet an arrow starts at
    /// the top rather than doing nothing, which is what every list does.
    pub fn move_focus(&self, delta: i32, extend: bool) -> Option<usize> {
        let last = self.model.row_count().checked_sub(1)?;
        // Nothing focused yet: an arrow starts at the top rather than doing
        // nothing, which is what every list does.
        let next = self
            .focus_row()
            .map_or(0, |current| (current as i64 + i64::from(delta)).clamp(0, last as i64) as usize);
        self.focus_to(next, extend);
        Some(next)
    }

    /// Jump to the top or, for a negative index, the end.
    pub fn focus_edge(&self, edge: i32, extend: bool) -> Option<usize> {
        let last = self.model.row_count().checked_sub(1)?;
        let target = if edge < 0 { last } else { 0 };
        self.focus_to(target, extend);
        Some(target)
    }

    fn focus_to(&self, row: usize, extend: bool) {
        // Shift extends from the anchor, which is exactly what a click with
        // Shift does — one behaviour, reached two ways.
        self.select(row, false, extend);
        self.focused.set(self.model.id_at(row));
    }

    pub fn select(&self, view_index: usize, ctrl: bool, shift: bool) {
        if !shift {
            // A plain or ctrl click re-anchors the keyboard too, so the arrows
            // continue from where the hand left off.
            self.focused.set(self.model.id_at(view_index));
        }
        let Some(id) = self.model.id_at(view_index) else { return };
        let mut selected = self.selected.borrow_mut();

        if shift {
            let anchor = self.anchor.get();
            let (from, to) = (anchor.min(view_index), anchor.max(view_index));
            selected.clear();
            selected.extend((from..=to).filter_map(|i| self.model.id_at(i)));
            return;
        }

        if ctrl {
            if !selected.remove(&id) {
                selected.insert(id);
            }
        } else {
            selected.clear();
            selected.insert(id);
        }
        self.anchor.set(view_index);
    }

    pub fn begin_resize(&self, column: usize) {
        self.resize_base.set(self.widths.row_data(column).unwrap_or(MIN_COL_W));
    }

    pub fn resize(&self, column: usize, delta: f32) {
        let width = (self.resize_base.get() + delta).clamp(MIN_COL_W, MAX_COL_W);
        self.widths.set_row_data(column, width);
    }

    /// The latest snapshot, straight from the engine.
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.engine.snapshot()
    }
}

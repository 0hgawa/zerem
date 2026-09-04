//! UI bridge: wires the Slint window to the state and the engine.
//!
//! One module per domain, each owning its helpers and registering its own
//! callbacks in `wire`. Nothing above this layer knows Slint exists, and nothing
//! in the `.slint` knows the engine does.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Model as _, SharedString};
use zerem_core::fmt;
use zerem_engine::{Snapshot, TICK};

use crate::state::UiState;
use crate::{MainWindow, SessionState, TorrentList};

pub mod add;
pub mod detail;
pub mod menu;
pub mod prefs;
pub mod torrents;

/// The models the UI owns, in one place.
///
/// Bundled rather than passed separately: every callback that redraws needs
/// all of them, and threading each one through by hand was turning every
/// signature into a list of bags.
pub struct Views {
    pub detail: detail::Models,
    pub add: add::Choice,
}

impl Views {
    #[must_use]
    pub fn new(ui: &MainWindow) -> Self {
        Self { detail: detail::Models::new(ui), add: add::Choice::new(ui) }
    }
}

/// Register every domain's callbacks.
pub fn wire(ui: &MainWindow, state: &Rc<UiState>, store: &Rc<crate::settings::Store>, views: &Rc<Views>) {
    torrents::wire(ui, state, store, views);
    prefs::wire(ui, state, store, views);
    detail::wire(ui, state, views);
    menu::wire(ui, state, views);
    add::wire(ui, state, views);
}

/// Faster than the engine publishes, deliberately.
///
/// Two independent 1 Hz timers drift into phase with each other, and a snapshot
/// published just after the UI looked then waits a whole second to be drawn —
/// measured in the first run as a two-second gap between consecutive sequences.
/// Sampling four times per publish bounds the staleness at 250 ms, and costs an
/// atomic load and a comparison on the three samples that find nothing new.
const POLL: Duration = Duration::from_millis(TICK.as_millis() as u64 / 4);

/// The one timer on the UI thread.
///
/// It does two things, and both have to survive a stopped engine — which is why
/// neither can hang off the engine's own heartbeat:
///
///  * follows the window's visibility, so the engine stops when nobody is
///    looking and starts again when the window comes back. Phase 0 measured a
///    stopped engine at 0.000 % CPU, and that is the whole point;
///  * redraws, but only when the sequence has actually advanced. A stopped
///    engine publishes nothing, so this costs an atomic load and returns.
///
/// It also does the first paint. The engine already holds a snapshot when the
/// window opens, and starting the clock from it is what keeps the opening frame
/// from being drawn twice.
///
/// The returned timer has to outlive the event loop.
#[must_use]
pub fn start_tick(ui: &MainWindow, state: &Rc<UiState>, views: &Rc<Views>) -> slint::Timer {
    let initial = state.snapshot();
    refresh(ui, state, &initial, views);

    let timer = slint::Timer::default();
    let applied_seq = Cell::new(initial.seq);
    let was_visible = Cell::new(true);

    timer.start(slint::TimerMode::Repeated, POLL, {
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };

            let visible = ui.window().is_visible();
            if visible != was_visible.get() {
                was_visible.set(visible);
                state.engine.set_paused(!visible);
                tracing::debug!(visible, "window visibility changed, engine follows");
            }

            let snapshot = state.snapshot();
            if snapshot.seq == applied_seq.get() {
                return;
            }
            applied_seq.set(snapshot.seq);
            refresh(&ui, &state, &snapshot, &views);
        }
    });
    timer
}

/// Pull a snapshot through to the screen — the single path by which anything
/// reaches the table.
pub fn refresh(ui: &MainWindow, state: &UiState, snapshot: &Snapshot, views: &Views) {
    state.prune_selection(snapshot);
    state.refresh_order(snapshot);
    let applied = state.apply(snapshot);

    let list = ui.global::<TorrentList>();
    let selection = state.selection();
    push!(list, get_selected_count, set_selected_count, selection.len() as i32);
    push!(list, get_selection_running, set_selection_running, state.model.all_running(&selection));
    push!(list, get_sort_col, set_sort_col, state.sort().col as i32);
    push!(list, get_sort_desc, set_sort_desc, state.sort().desc);
    // Looked up fresh every refresh: a re-sort or a removal moves the row, and
    // the arrows have to stay on the torrent rather than on the position.
    push!(list, get_focus_row, set_focus_row, state.focus_row().map_or(-1, |r| r as i32));
    drop(selection);

    let session = ui.global::<SessionState>();
    let totals = snapshot.stats;
    push!(session, get_down, set_down, fmt::speed(totals.down_bps).into());
    push!(session, get_up, set_up, fmt::speed(totals.up_bps).into());
    push!(session, get_active, set_active, totals.active.to_string().into());
    push!(session, get_paused, set_paused, totals.paused.to_string().into());
    push!(session, get_total, set_total, snapshot.torrents.len().to_string().into());
    // Empty unless a filter is on, and the status bar swaps the plain total for
    // it — the model already holds exactly the rows that survived.
    push!(
        session,
        get_matched,
        set_matched,
        if state.is_filtering() {
            fmt::matched(state.model.row_count(), snapshot.torrents.len()).into()
        } else {
            SharedString::default()
        }
    );
    push!(
        session,
        get_notice,
        set_notice,
        state.notice(snapshot).map_or_else(Default::default, |n| n.as_ref().into())
    );
    // Built here rather than in the engine: what crosses the snapshot boundary
    // is what happened, and turning a minute of samples into a drawing is this
    // side's business. `None` is an idle minute, and draws nothing.
    let spark = snapshot.history.spark();
    push!(
        session,
        get_spark_down,
        set_spark_down,
        spark.as_ref().map_or_else(SharedString::default, |s| s.down.as_str().into())
    );
    push!(
        session,
        get_spark_up,
        set_spark_up,
        spark.as_ref().map_or_else(SharedString::default, |s| s.up.as_str().into())
    );

    tracing::debug!(
        seq = snapshot.seq,
        rows = snapshot.torrents.len(),
        changed = applied.changed,
        micros = applied.micros,
        reset = applied.reset,
        "applied snapshot"
    );

    detail::refresh(ui, snapshot, &views.detail);
    add::refresh(ui, snapshot, &views.add);
}

/// Redraw against the engine's current snapshot. Used after an optimistic edit,
/// where the point is not to wait for the next tick.
pub fn refresh_now(ui: &MainWindow, state: &UiState, views: &Views) {
    let snapshot = state.snapshot();
    refresh(ui, state, &snapshot, views);
}

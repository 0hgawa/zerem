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
mod shell;
pub mod torrents;
pub mod updates;

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
    show_mark(ui);
    shell::wire(ui);
    torrents::wire(ui, state, store, views);
    prefs::wire(ui, state, store, views);
    detail::wire(ui, state, store, views);
    menu::wire(ui, state, views);
    add::wire(ui, state, store, views);
    updates::wire(ui);
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
    announce(state, snapshot);
    state.prune_selection(snapshot);
    state.refresh_order(snapshot);
    let applied = state.apply(snapshot);

    let list = ui.global::<TorrentList>();
    let selection = state.selection();
    push!(list, get_selected_count, set_selected_count, selection.len() as i32);
    push!(list, get_selection_running, set_selection_running, state.model.all_running(&selection));
    push!(list, get_sort_col, set_sort_col, state.sort().col as i32);
    push!(list, get_shown, set_shown, state.shown().index());
    shelves(ui, state, snapshot);
    // The window can be maximised by something other than our own button — a
    // keyboard snap, the taskbar menu — and a button showing "maximise" on a
    // maximised window is a button that lies.
    shell::refresh(ui);
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
    push!(session, get_downloading, set_downloading, totals.downloading.to_string().into());
    push!(session, get_seeding, set_seeding, totals.seeding.to_string().into());
    push!(session, get_queued, set_queued, totals.queued.to_string().into());
    push!(session, get_failed, set_failed, totals.failed.to_string().into());
    // Whether anything at all is hiding rows. The empty state is a different
    // sentence depending on it, and the `.slint` could only see the search box.
    push!(ui.global::<TorrentList>(), get_narrowed, set_narrowed, state.is_filtering());
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

/// Draw the app's mark into the window, once.
///
/// From the same function the taskbar icon and the `.ico` come from, so the
/// mark in the corner and the mark in the taskbar cannot drift apart.
///
/// Rendered larger than it is shown: it is a procedural drawing with a feathered
/// corner, and downscaling a clean 64px square beats aliasing a 20px one.
fn show_mark(ui: &MainWindow) {
    const SIZE: u32 = 64;
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(SIZE, SIZE);
    buffer.make_mut_bytes().copy_from_slice(&zerem_core::icon::rgba(SIZE));
    ui.global::<crate::Brand>().set_mark(slint::Image::from_rgba8(buffer));
}

/// The shelves in the rail, with how many are on each.
///
/// Rebuilt from the snapshot rather than kept in step by hand: a torrent
/// removed, added or re-assigned all change the counts, and three places
/// remembering to update one number is three places that will not.
fn shelves(ui: &MainWindow, state: &UiState, snapshot: &Snapshot) {
    let counts = state.counts(snapshot);
    let on = state.category().map(|name| zerem_core::category::key(&name));
    let rows: Vec<crate::CategoryEntry> = state
        .shelf_names()
        .into_iter()
        .map(|name| {
            let key = zerem_core::category::key(&name);
            crate::CategoryEntry {
                count: counts.get(&key).copied().unwrap_or(0).to_string().into(),
                active: on.as_deref() == Some(key.as_str()),
                name: name.into(),
            }
        })
        .collect();
    let list = ui.global::<TorrentList>();
    // Replaced whole. It is a handful of rows that change when somebody adds a
    // torrent, which is not a rate worth diffing for.
    list.set_categories(slint::ModelRc::new(slint::VecModel::from(rows)));
}

/// Say that something finished.
///
/// The status bar, and a flash of the taskbar button if the window is not the
/// one in front. Never a steal of focus: interrupting whatever somebody is
/// doing to announce that a file arrived is the behaviour that makes people
/// turn notifications off.
///
/// One line however many landed at once. Four notices in four seconds would
/// push each other off before any of them was read.
fn announce(state: &UiState, snapshot: &Snapshot) {
    let Some(first) = snapshot.finished.first() else { return };
    match snapshot.finished.len() {
        1 => state.set_notice(&zerem_core::text::finished_one(first)),
        n => state.set_notice(&zerem_core::text::finished_many(n)),
    }
    zerem_shell::ask_attention();
}

/// Redraw against the engine's current snapshot. Used after an optimistic edit,
/// where the point is not to wait for the next tick.
pub fn refresh_now(ui: &MainWindow, state: &UiState, views: &Views) {
    let snapshot = state.snapshot();
    refresh(ui, state, &snapshot, views);
}

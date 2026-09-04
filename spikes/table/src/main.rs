// Zerem — spike S0.1 (virtualised table) + S0.3 (snapshot tick).
//
// What is being measured, and the bar each has to clear:
//
//   S0.1  2000 rows, sortable, multi-selectable, resizable columns.
//         60 fps while scrolling · under 1 % CPU at rest · scroll position and
//         selection survive every tick.
//   S0.3  A tick costs what changed, not what is on screen. No reallocation of
//         the model, no reset, diff under 1 ms at 2000 rows.
//
// The console prints one line per tick; that log is what docs/spike-report.md
// is built from.
//
// Keys:  S stress (redraw flat out)   C churn (every row moves)
//        T theme                      1/2/3 → 200 / 2000 / 10000 rows
// Env:   ZEREM_ROWS · ZEREM_TICK_MS · ZEREM_SORT_COL

mod fake;
mod fmt;
mod model;
mod sort;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Model, ModelRc, RenderingState, VecModel};

use fake::Session;
use model::TorrentModel;
use sort::Sort;

slint::include_modules!();

const MIN_COL_W: f32 = 48.0;
const MAX_COL_W: f32 = 640.0;

/// Overridable so one build can be measured at several list sizes and tick
/// rates. `ZEREM_TICK_MS=60000` in particular isolates what the window costs
/// with nothing at all changing — the answer turned out to be zero.
fn env_usize(key: &str, fallback: usize) -> usize {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(fallback)
}

/// Write a UI property only when the value actually differs — the scalar form
/// of the rule the model diff follows.
macro_rules! push {
    ($ui:expr, $get:ident, $set:ident, $value:expr) => {{
        let next = $value;
        if $ui.$get() != next {
            $ui.$set(next);
        }
    }};
}

struct App {
    session: RefCell<Session>,
    model: Rc<TorrentModel>,
    /// Reused across ticks — the display order allocates once, not every second.
    order: RefCell<Vec<usize>>,
    selected: RefCell<HashSet<u32>>,
    /// Where a shift-range starts. A view index, valid only until the sort
    /// changes, which is why the selection itself is kept as ids.
    anchor: Cell<usize>,
    sort: Cell<Sort>,
    widths: Rc<VecModel<f32>>,
    resize_base: Cell<f32>,
    frames: Cell<u32>,
    stress: Cell<bool>,
}

impl App {
    fn new() -> Self {
        let mut session = Session::new(env_usize("ZEREM_ROWS", 2000));
        // `ZEREM_CHURN=1` starts in the worst case, so the ceiling can be
        // measured without a hand on the keyboard.
        session.churn = env_usize("ZEREM_CHURN", 0) == 1;
        Self {
            session: RefCell::new(session),
            model: Rc::new(TorrentModel::new()),
            order: RefCell::new(Vec::new()),
            selected: RefCell::new(HashSet::new()),
            anchor: Cell::new(0),
            // `ZEREM_SORT_COL=4` starts on a volatile column, which is how the
            // expensive sort path gets measured without a hand on the mouse.
            sort: Cell::new(Sort::first_click(env_usize("ZEREM_SORT_COL", Sort::NAME).min(8))),
            widths: Rc::new(VecModel::from(vec![300.0, 88.0, 132.0, 104.0, 96.0, 96.0, 84.0, 84.0, 66.0])),
            resize_base: Cell::new(0.0),
            frames: Cell::new(0),
            stress: Cell::new(false),
        }
    }

    /// Rebuild the display order unconditionally, returning the microseconds it
    /// took. Used when the user changes the sort or the row set.
    fn sort_now(&self) -> u64 {
        let started = Instant::now();
        let session = self.session.borrow();
        sort::order(&session.torrents, self.sort.get(), &mut self.order.borrow_mut());
        started.elapsed().as_micros() as u64
    }

    /// The per-tick path: re-sort only when the order can actually have moved.
    ///
    /// Sorting by a volatile column (speed, progress, ETA) genuinely reorders
    /// every second and has to be paid for. Sorting by name or size does not
    /// move while the row set is unchanged, and that is the default — so the
    /// common case does no work rather than 3.2 ms of it.
    fn reorder(&self) -> Option<u64> {
        let unchanged_row_set = self.order.borrow().len() == self.session.borrow().torrents.len();
        if !sort::is_volatile(self.sort.get().col) && unchanged_row_set {
            return None;
        }
        Some(self.sort_now())
    }

    /// Push the current state into the UI and report what it cost.
    fn apply(&self, ui: &MainWindow) -> model::ApplyStats {
        let session = self.session.borrow();
        let order = self.order.borrow();
        let selected = self.selected.borrow();
        let stats = self.model.apply(&session.torrents, &order, &selected);

        let ts = ui.global::<TableState>();
        push!(ts, get_diff_us, set_diff_us, format!("{} µs", stats.micros).into());
        push!(ts, get_changed, set_changed, stats.changed.to_string().into());
        push!(ts, get_total_count, set_total_count, session.torrents.len().to_string().into());
        push!(ts, get_active_count, set_active_count, session.active_count().to_string().into());
        push!(ts, get_sel_count, set_sel_count, selected.len().to_string().into());
        push!(ts, get_sort_col, set_sort_col, self.sort.get().col as i32);
        push!(ts, get_sort_desc, set_sort_desc, self.sort.get().desc);
        stats
    }

    fn select(&self, view_index: usize, ctrl: bool, shift: bool) {
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
}

/// Register every callback. The real app splits this into `bridge/<domain>.rs`,
/// one `wire` each; at this size one function is the same shape.
fn wire(ui: &MainWindow, app: &Rc<App>) {
    let ts = ui.global::<TableState>();

    ts.on_sort({
        let (app, ui) = (app.clone(), ui.as_weak());
        move |col| {
            let Some(ui) = ui.upgrade() else { return };
            app.sort.set(app.sort.get().clicked(col.max(0) as usize));
            app.sort_now();
            // Applied here rather than at the next tick: a sort that waits up
            // to a second to appear reads as a broken click.
            app.apply(&ui);
        }
    });

    ts.on_resize_begin({
        let app = app.clone();
        move |idx| {
            app.resize_base.set(app.widths.row_data(idx.max(0) as usize).unwrap_or(MIN_COL_W));
        }
    });

    ts.on_resize_move({
        let app = app.clone();
        move |idx, delta| {
            let width = (app.resize_base.get() + delta).clamp(MIN_COL_W, MAX_COL_W);
            app.widths.set_row_data(idx.max(0) as usize, width);
        }
    });

    ts.on_select({
        let (app, ui) = (app.clone(), ui.as_weak());
        move |index, ctrl, shift| {
            let Some(ui) = ui.upgrade() else { return };
            app.select(index.max(0) as usize, ctrl, shift);
            app.apply(&ui);
        }
    });

    ts.on_toggle_stress({
        let (app, ui) = (app.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            app.stress.set(!app.stress.get());
            ui.global::<TableState>().set_stress(app.stress.get());
            ui.window().request_redraw();
        }
    });

    ts.on_toggle_churn({
        let (app, ui) = (app.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let mut session = app.session.borrow_mut();
            session.churn = !session.churn;
            ui.global::<TableState>().set_churn(session.churn);
        }
    });

    ts.on_toggle_theme({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let theme = ui.global::<Theme>();
            theme.set_dark(!theme.get_dark());
        }
    });

    ts.on_set_count({
        let (app, ui) = (app.clone(), ui.as_weak());
        move |count| {
            let Some(ui) = ui.upgrade() else { return };
            app.session.borrow_mut().resize(count.max(0) as usize);
            app.selected.borrow_mut().clear();
            app.anchor.set(0);
            app.sort_now();
            app.apply(&ui);
        }
    });
}

/// Count real repaints. Stress mode asks for the next frame from inside this
/// one, so the counter reports the ceiling the table can reach.
///
/// Only the GPU renderers offer the hook. Under `SLINT_BACKEND=winit-software`
/// there is none, and that is worth measuring rather than crashing over — the
/// frame count is instrumentation, not the app.
fn install_frame_counter(ui: &MainWindow, app: &Rc<App>) {
    let result = ui.window().set_rendering_notifier({
        let (app, ui) = (app.clone(), ui.as_weak());
        move |state, _| {
            if !matches!(state, RenderingState::BeforeRendering) {
                return;
            }
            app.frames.set(app.frames.get() + 1);
            if app.stress.get() {
                if let Some(ui) = ui.upgrade() {
                    ui.window().request_redraw();
                }
            }
        }
    });
    if let Err(e) = result {
        eprintln!("no frame counter on this renderer ({e:?}); frames will read 0");
    }
}

/// The snapshot tick. The returned timer has to outlive the event loop.
fn start_tick(ui: &MainWindow, app: &Rc<App>) -> slint::Timer {
    let tick_ms = env_usize("ZEREM_TICK_MS", 1000) as u64;
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(tick_ms), {
        let (app, ui) = (app.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let moved = app.session.borrow_mut().tick();
            let sort_us = app.reorder();
            let stats = app.apply(&ui);

            let frames = app.frames.replace(0);
            let ts = ui.global::<TableState>();
            push!(ts, get_fps, set_fps, frames.to_string().into());
            push!(
                ts,
                get_sort_us,
                set_sort_us,
                sort_us.map_or_else(|| "skipped".into(), |us| format!("{us} µs")).into()
            );

            println!(
                "rows={:<6} moved={:<5} changed={:<5} sort={:<8} diff={:<5}us frames={:<4} reset={}",
                ts.get_total_count(),
                moved,
                stats.changed,
                sort_us.map_or_else(|| "skip".to_string(), |us| format!("{us}us")),
                stats.micros,
                frames,
                stats.reset,
            );
        }
    });
    timer
}

fn main() -> Result<(), slint::PlatformError> {
    // femtovg so the rendering notifier hands back a real GL context; the frame
    // counter has to observe actual repaints, not a schedule.
    if std::env::var_os("SLINT_BACKEND").is_none() {
        std::env::set_var("SLINT_BACKEND", "winit-femtovg");
    }

    let ui = MainWindow::new()?;
    let app = Rc::new(App::new());

    let ts = ui.global::<TableState>();
    ts.set_rows(ModelRc::from(app.model.clone()));
    ts.set_cw(ModelRc::from(app.widths.clone()));

    app.sort_now();
    app.apply(&ui);

    wire(&ui, &app);
    install_frame_counter(&ui, &app);
    let _tick = start_tick(&ui, &app);

    ui.run()
}

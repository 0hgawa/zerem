//! The table: sorting, selection, column widths, adding, and the one
//! destructive command — which asks first.

use std::rc::Rc;

use slint::ComponentHandle;
use zerem_core::TorrentId;
use zerem_engine::Command;

use crate::state::UiState;
use crate::{MainWindow, TorrentList};

pub fn wire(
    ui: &MainWindow,
    state: &Rc<UiState>,
    store: &Rc<crate::settings::Store>,
    views: &Rc<super::Views>,
) {
    let list = ui.global::<TorrentList>();

    list.on_sort({
        let (state, store, ui, views) = (state.clone(), store.clone(), ui.as_weak(), views.clone());
        move |col| {
            let Some(ui) = ui.upgrade() else { return };
            state.set_sort(col.max(0) as usize, &state.snapshot());
            // Saved as it changes rather than only on the way out: closing the
            // window hides to the tray, so the graceful exit that would have
            // written this may never happen. The store debounces, so a burst of
            // clicks still costs one write.
            state.save_view(&store);
            // Applied here rather than at the next tick: a sort that waits up
            // to a second to appear reads as a broken click.
            super::refresh_now(&ui, &state, &views);
        }
    });

    list.on_select({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |index, ctrl, shift| {
            let Some(ui) = ui.upgrade() else { return };
            state.select(index.max(0) as usize, ctrl, shift);
            // A plain click opens the drawer on what was clicked — the way a
            // mail client shows a message. Reaching for a button to see what
            // is inside a torrent is a step nobody should have to find.
            //
            // Only a plain one. Ctrl and Shift are building a selection of
            // several, and a drawer can only show one: opening it there would
            // be picking a torrent out of the group on the user's behalf.
            // Opens, and only opens. Clicking the same row again used to close
            // it, and that had to go: a double click *is* two single clicks, so
            // double-clicking a row opened the panel, closed it, and then
            // opened the folder — the panel flickering every time.
            //
            // Any scheme that keeps both has to work out which gesture is
            // happening, and the only way to do that is to wait out the
            // double-click interval before acting on the first click. That
            // taxes the common case, opening the panel, to serve the rare one.
            // Escape closes it instead, and so does its own ×.
            if !ctrl && !shift {
                ui.global::<crate::DetailState>().set_open(true);
            }
            // The drawer follows the selection: what it shows has to be what
            // is highlighted, or it is showing the wrong torrent.
            super::detail::follow_selection(&ui, &state);
            super::refresh_now(&ui, &state, &views);
        }
    });

    list.on_select_all({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            state.select_all();
            super::detail::follow_selection(&ui, &state);
            super::refresh_now(&ui, &state, &views);
        }
    });

    list.on_show_state({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |index| {
            let Some(ui) = ui.upgrade() else { return };
            state.set_shown(zerem_core::Shown::from_index(index), &state.snapshot());
            // Narrowing can cut the selection down, and the drawer has to stop
            // showing a torrent the list no longer holds.
            super::detail::follow_selection(&ui, &state);
            super::refresh_now(&ui, &state, &views);
        }
    });

    wire_rail(ui, state, store, views);
}

/// The rail: which states and which shelves the view is narrowed to.
///
/// Its own function because it is its own question. Everything above acts on a
/// torrent; these three act on what is being looked at.
fn wire_rail(
    ui: &MainWindow,
    state: &Rc<UiState>,
    store: &Rc<crate::settings::Store>,
    views: &Rc<super::Views>,
) {
    let list = ui.global::<TorrentList>();

    list.on_show_all({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let list = ui.global::<TorrentList>();
            // The box is cleared here as well as in the state: it is a two-way
            // binding, and leaving it holding text the filter no longer has is
            // a search box that lies about what it is doing.
            list.set_filter(slint::SharedString::default());
            state.show_all(&state.snapshot());
            super::refresh_now(&ui, &state, &views);
        }
    });

    list.on_show_category({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |name| {
            let Some(ui) = ui.upgrade() else { return };
            let chosen = (!name.is_empty()).then(|| name.to_string());
            state.set_category(chosen, &state.snapshot());
            super::detail::follow_selection(&ui, &state);
            super::refresh_now(&ui, &state, &views);
        }
    });

    list.on_toggle_rail({
        let (store, ui) = (store.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let list = ui.global::<TorrentList>();
            // Open, then folded, then gone, then open again. Downwards, so
            // the press that gives up room is the same press every time and
            // the one that gives it back is the one after the last.
            let next = match list.get_rail_state() {
                2 => 1,
                1 => 0,
                _ => 2,
            };
            list.set_rail_state(next);
            store.update(|s| s.rail_state = u8::try_from(next).unwrap_or(2));
        }
    });

    list.on_open_row_folder({
        let (state, ui) = (state.clone(), ui.as_weak());
        move |index| {
            let Some(ui) = ui.upgrade() else { return };
            // The click that came first has already selected the row, so this
            // acts on the same torrent the eye is on.
            let _ = index;
            ui.global::<TorrentList>().invoke_open_folder();
            let _ = &state;
        }
    });

    list.on_resize_begin({
        let state = state.clone();
        move |col| state.begin_resize(col.max(0) as usize)
    });

    list.on_resize_move({
        let (state, store) = (state.clone(), store.clone());
        move |col, delta| {
            state.resize(col.max(0) as usize, delta);
            // Every pixel of the drag lands here; the 400 ms debounce is what
            // turns the whole drag into one write.
            state.save_view(&store);
        }
    });

    list.on_start_selected(on_selection(ui, state, views, |state, id, seq| {
        state.engine.send(Command::Start(id));
        state.expect_start(id, seq);
    }));

    list.on_pause_selected(on_selection(ui, state, views, |state, id, seq| {
        state.engine.send(Command::Pause(id));
        state.expect_pause(id, seq);
    }));

    // The hover button. Same two actions, aimed at the row under the pointer
    // instead of at the selection.
    list.on_start_row(on_row(ui, state, views, |state, id, seq| {
        state.engine.send(Command::Start(id));
        state.expect_start(id, seq);
    }));

    list.on_pause_row(on_row(ui, state, views, |state, id, seq| {
        state.engine.send(Command::Pause(id));
        state.expect_pause(id, seq);
    }));

    // Every keystroke. Rebuilt and redrawn at once rather than on the next
    // tick: a search box that answers a second later reads as broken.
    list.on_filter_changed({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |text| {
            let Some(ui) = ui.upgrade() else { return };
            state.set_filter(&text, &state.snapshot());
            // Filtering can cut the selection down, and the drawer has to
            // follow it or it is showing a torrent that is no longer picked.
            super::detail::follow_selection(&ui, &state);
            super::refresh_now(&ui, &state, &views);
        }
    });

    wire_adding(ui, state, views);
    wire_columns(ui, state, store);
    wire_removing(ui, state, store, views);
}

// --- adding ----------------------------------------------------------------

fn wire_adding(ui: &MainWindow, state: &Rc<UiState>, views: &Rc<super::Views>) {
    let list = ui.global::<TorrentList>();

    list.on_add_magnet_from_clipboard({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            match clipboard_text() {
                Some(text) => add(&state, &text),
                // Said in the status bar rather than a popup: a failed paste
                // should not interrupt anything.
                None => state.set_notice(zerem_core::tr("The clipboard has no magnet link in it")),
            }
            super::refresh_now(&ui, &state, &views);
        }
    });

    list.on_add_torrent_file({
        let state = state.clone();
        move || {
            // Off the UI thread, always. A native dialog opened on the event
            // loop stalls the render loop for as long as it is up — the lesson
            // Vayou already paid for.
            //
            // The thread talks to the engine directly rather than hopping back
            // here: `UiState` is `Rc` and cannot cross a thread, while `Engine`
            // is a handle built to. Nothing is lost by not redrawing — the tick
            // is 250 ms away and adding a torrent is not instant anyway.
            let engine = state.engine.clone();
            std::thread::spawn(move || {
                let Some(paths) = rfd::FileDialog::new()
                    .add_filter("Torrent", &["torrent"])
                    .set_title("Add a torrent")
                    .pick_files()
                else {
                    return;
                };
                for path in paths {
                    let source = path.to_string_lossy().into_owned();
                    tracing::debug!(source, "inspecting");
                    engine.send(Command::Inspect { source });
                }
            });
        }
    });
}

/// Read the clipboard, keeping only something that could plausibly be a
/// torrent. Pasting a paragraph of prose should say so, not be handed to the
/// engine to reject with a worse message.
fn clipboard_text() -> Option<String> {
    let text = arboard::Clipboard::new().ok()?.get_text().ok()?;
    let text = text.trim().to_string();
    let plausible = text.starts_with("magnet:")
        || text.starts_with("http://")
        || text.starts_with("https://")
        || text.ends_with(".torrent");
    plausible.then_some(text)
}

/// Never adds directly: it reads the torrent and lets the dialog ask.
fn add(state: &UiState, source: &str) {
    tracing::debug!(source, "inspecting");
    state.engine.send(Command::Inspect { source: source.to_string() });
}

// --- columns ---------------------------------------------------------------

/// Which columns the table draws, from a right-click in its header.
///
/// The menu stays open while it is used: hiding four columns is one gesture,
/// not four trips back to a right-click.
fn wire_columns(ui: &MainWindow, state: &Rc<UiState>, store: &Rc<crate::settings::Store>) {
    let list = ui.global::<TorrentList>();

    list.on_open_columns({
        let ui = ui.as_weak();
        move |x, y| {
            let Some(ui) = ui.upgrade() else { return };
            let list = ui.global::<TorrentList>();
            list.set_columns_x(x);
            list.set_columns_y(y);
            list.set_columns_open(true);
        }
    });

    list.on_close_columns({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<TorrentList>().set_columns_open(false);
        }
    });

    list.on_toggle_column({
        let (state, store) = (state.clone(), store.clone());
        move |col| {
            state.toggle_column(col.max(0) as usize);
            // Saved as it changes, like the widths and the sort: closing the
            // window hides to the tray, so the graceful exit may never come.
            state.save_view(&store);
        }
    });
}
// --- removing --------------------------------------------------------------

fn wire_removing(
    ui: &MainWindow,
    state: &Rc<UiState>,
    store: &Rc<crate::settings::Store>,
    views: &Rc<super::Views>,
) {
    let list = ui.global::<TorrentList>();

    // The toolbar, the context menu and Delete: the selection.
    list.on_remove_selected({
        let (state, ui) = (state.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let targets: Vec<TorrentId> = state.selection().iter().copied().collect();
            confirm_remove(&ui, &state, targets);
        }
    });

    // The button on a row: that row, whatever is selected.
    list.on_remove_row({
        let (state, ui) = (state.clone(), ui.as_weak());
        move |index| {
            let Some(ui) = ui.upgrade() else { return };
            let Some(id) = state.model.id_at(index.max(0) as usize) else { return };
            confirm_remove(&ui, &state, vec![id]);
        }
    });

    list.on_confirm_cancel({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<TorrentList>().set_confirming(false);
        }
    });

    list.on_confirm_accept({
        let (state, store, ui, views) = (state.clone(), store.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let list = ui.global::<TorrentList>();
            let delete_data = list.get_confirm_delete_data();
            list.set_confirming(false);

            let seq = state.snapshot().seq;
            // What the dialog was opened about, not what is selected now — the
            // row button aims at a torrent that may not be selected at all.
            let targets = state.remove_target();
            // Which shelf a torrent was on outlives the torrent otherwise: the
            // assignment is keyed by info hash and nothing else would ever ask
            // about that hash again.
            state.forget_assigned(&targets, &store);
            for id in targets {
                state.engine.send(Command::Remove { id, delete_data });
                state.expect_remove(id, seq);
            }
            super::refresh_now(&ui, &state, &views);
        }
    });
}

/// Open the confirmation over `targets`. The single path to removing anything,
/// which is why the drawer reaches for it too rather than asking its own way.
pub(super) fn confirm_remove(ui: &MainWindow, state: &UiState, targets: Vec<TorrentId>) {
    if !state.begin_remove(targets) {
        return;
    }
    let question = state.confirm_question(&state.snapshot());
    let list = ui.global::<TorrentList>();
    list.set_confirm_what(question.into());
    // Reset every time it opens: "also delete the data" must never be
    // inherited from a previous answer.
    list.set_confirm_delete_data(false);
    list.set_confirming(true);
}

// --- shared ----------------------------------------------------------------

/// Run `act` for every selected torrent, then redraw.
///
/// `act` both sends the command and records what it is expected to do. The
/// engine answers on its own tick, up to a second away; the recorded guess is
/// what the table shows in the meantime, and the first snapshot past it is the
/// truth — including when the engine declined, in which case the row comes back
/// on its own.
fn on_selection(
    ui: &MainWindow,
    state: &Rc<UiState>,
    views: &Rc<super::Views>,
    act: fn(&UiState, TorrentId, u64),
) -> impl FnMut() + 'static {
    let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
    move || {
        let Some(ui) = ui.upgrade() else { return };
        let seq = state.snapshot().seq;
        // Collected so the borrow is released before `refresh` reads it back.
        let targets: Vec<TorrentId> = state.selection().iter().copied().collect();
        for id in targets {
            act(&state, id, seq);
        }
        super::refresh_now(&ui, &state, &views);
    }
}

/// Run `act` for the torrent at one view row, then redraw.
///
/// Deliberately blind to the selection: the hover button acts on what the
/// cursor is over and nothing else, which is the whole reason it saves a step
/// over the toolbar. It resolves the row through the model rather than trusting
/// the index, because an index is only meaningful against the sort it was drawn
/// under.
fn on_row(
    ui: &MainWindow,
    state: &Rc<UiState>,
    views: &Rc<super::Views>,
    act: fn(&UiState, TorrentId, u64),
) -> impl FnMut(i32) + 'static {
    let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
    move |index| {
        let Some(ui) = ui.upgrade() else { return };
        let Some(id) = state.model.id_at(index.max(0) as usize) else { return };
        act(&state, id, state.snapshot().seq);
        super::refresh_now(&ui, &state, &views);
    }
}

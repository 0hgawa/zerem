//! The right-click menu and the arrow keys.
//!
//! Neither adds a capability — everything here is reachable from the toolbar or
//! a shortcut. They exist because the row under the cursor is where the hand
//! already is, and because a list you cannot walk with the keyboard is a list
//! that fights anyone who is not holding a mouse.

use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::UiState;
use crate::{MainWindow, TorrentList};

pub fn wire(ui: &MainWindow, state: &Rc<UiState>, views: &Rc<super::Views>) {
    let list = ui.global::<TorrentList>();

    list.on_open_menu({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |index, x, y| {
            let Some(ui) = ui.upgrade() else { return };
            let row = index.max(0) as usize;

            // A right-click on a row that is not selected selects it first. A
            // menu that acts on something other than what was clicked is the
            // classic way to delete the wrong thing.
            let already = state.model.id_at(row).is_some_and(|id| state.selection().contains(&id));
            if !already {
                state.select(row, false, false);
                super::detail::follow_selection(&ui, &state);
            }

            let list = ui.global::<TorrentList>();
            list.set_menu_x(x);
            list.set_menu_y(y);
            list.set_menu_open(true);
            super::refresh_now(&ui, &state, &views);
        }
    });

    list.on_close_menu({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<TorrentList>().set_menu_open(false);
        }
    });

    list.on_move_focus({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |delta, extend| {
            let Some(ui) = ui.upgrade() else { return };
            if let Some(row) = state.move_focus(delta, extend) {
                after_focus(&ui, &state, &views, row);
            }
        }
    });

    list.on_focus_edge({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |edge, extend| {
            let Some(ui) = ui.upgrade() else { return };
            if let Some(row) = state.focus_edge(edge, extend) {
                after_focus(&ui, &state, &views, row);
            }
        }
    });

    list.on_open_folder({
        let (state, ui) = (state.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            match folder_of(&state) {
                Some(folder) => zerem_shell::reveal(std::path::Path::new(&folder)),
                None => state.set_notice("That torrent has no folder yet"),
            }
            let _ = ui;
        }
    });

    list.on_copy_magnet({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            match magnet_of(&state) {
                Some(magnet) => match arboard::Clipboard::new().and_then(|mut c| c.set_text(magnet)) {
                    Ok(()) => state.set_notice("Magnet link copied"),
                    Err(e) => state.set_notice(&format!("Could not reach the clipboard: {e}")),
                },
                None => state.set_notice("That torrent has no infohash yet"),
            }
            super::refresh_now(&ui, &state, &views);
        }
    });
}

/// Redraw, keep the drawer pointed at the new row, and nudge the list to scroll.
fn after_focus(ui: &MainWindow, state: &UiState, views: &super::Views, row: usize) {
    let list = ui.global::<TorrentList>();
    list.set_focus_row(row as i32);
    // Bumped rather than set: holding an arrow at the end of the list keeps the
    // same row, and the scroll still has to fire.
    list.set_focus_tick(list.get_focus_tick().wrapping_add(1));

    super::detail::follow_selection(ui, state);
    super::refresh_now(ui, state, views);
}

fn folder_of(state: &UiState) -> Option<String> {
    let id = state.acting_on()?;
    let snapshot = state.snapshot();
    let row = snapshot.torrents.iter().find(|t| t.id == id)?;
    (!row.folder.is_empty()).then(|| row.folder.to_string())
}

fn magnet_of(state: &UiState) -> Option<String> {
    let id = state.acting_on()?;
    let snapshot = state.snapshot();
    let row = snapshot.torrents.iter().find(|t| t.id == id)?;
    (!row.info_hash.is_empty()).then(|| row.magnet())
}

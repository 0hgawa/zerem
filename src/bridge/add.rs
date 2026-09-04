//! The add dialog: inspect, choose, confirm.
//!
//! Adding is two steps rather than one because the two decisions that matter —
//! where it goes, and what comes down — are made here, and every other client
//! buries them in a cramped modal.
//!
//! The tick marks live on this side rather than in the engine. The engine
//! reports what the torrent contains; what the user has chosen so far is a view
//! concern, and round-tripping every checkbox through a command channel would
//! make a click wait on a thread for no reason.

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, Model as _, ModelRc, VecModel};
use zerem_core::{fmt, Pending};
use zerem_engine::{Command, Snapshot};

use crate::state::UiState;
use crate::{AddState, MainWindow, PendingFileEntry};

pub struct Choice {
    rows: Rc<VecModel<PendingFileEntry>>,
    /// Which files are ticked, and the sizes to total. Held here so a click is
    /// a local edit rather than a round trip.
    files: RefCell<Vec<(u64, bool)>>,
    /// The pending torrent this choice belongs to, so a second `Inspect` does
    /// not inherit the first one's ticks.
    source: RefCell<String>,
}

impl Choice {
    #[must_use]
    pub fn new(ui: &MainWindow) -> Self {
        let rows = Rc::new(VecModel::default());
        ui.global::<AddState>().set_files(ModelRc::from(rows.clone()));
        Self { rows, files: RefCell::new(Vec::new()), source: RefCell::new(String::new()) }
    }

    fn adopt(&self, pending: &Pending) {
        *self.source.borrow_mut() = pending.source.to_string();
        *self.files.borrow_mut() = pending.files.iter().map(|f| (f.size, f.wanted)).collect();
        self.rows.set_vec(
            pending
                .files
                .iter()
                .map(|f| PendingFileEntry {
                    path: f.path.as_ref().into(),
                    size: fmt::bytes(f.size).into(),
                    wanted: f.wanted,
                })
                .collect::<Vec<_>>(),
        );
    }

    fn set(&self, index: usize, wanted: bool) {
        if let Some(slot) = self.files.borrow_mut().get_mut(index) {
            slot.1 = wanted;
        }
        if let Some(mut row) = self.rows.row_data(index) {
            row.wanted = wanted;
            self.rows.set_row_data(index, row);
        }
    }

    fn only_files(&self) -> Option<Vec<usize>> {
        let files = self.files.borrow();
        if files.iter().all(|(_, wanted)| *wanted) {
            // librqbit wants `None`, not a list naming every file.
            return None;
        }
        Some(files.iter().enumerate().filter(|(_, (_, w))| *w).map(|(i, _)| i).collect())
    }

    /// "3 of 12 files · 1.44 GB of 3.72 GB" — and how many are ticked, which is
    /// what decides whether Add can be pressed.
    fn summary(&self) -> (String, usize) {
        let files = self.files.borrow();
        let chosen: Vec<u64> = files.iter().filter(|(_, w)| *w).map(|(s, _)| *s).collect();
        let total: u64 = files.iter().map(|(s, _)| *s).sum();
        let picked: u64 = chosen.iter().sum();
        let text = format!(
            "{} of {} files · {} of {}",
            chosen.len(),
            files.len(),
            fmt::bytes(picked),
            fmt::bytes(total)
        );
        (text, chosen.len())
    }
}

pub fn wire(ui: &MainWindow, state: &Rc<UiState>, views: &Rc<super::Views>) {
    let add = ui.global::<AddState>();

    add.on_cancel({
        let (state, ui) = (state.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<AddState>().set_open(false);
            state.engine.send(Command::CancelAdd);
        }
    });

    add.on_confirm({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<AddState>().set_open(false);
            state.engine.send(Command::ConfirmAdd { only_files: views.add.only_files() });
            super::refresh_now(&ui, &state, &views);
        }
    });

    add.on_toggle_file({
        let (views, ui) = (views.clone(), ui.as_weak());
        move |index| {
            let Some(ui) = ui.upgrade() else { return };
            let index = index.max(0) as usize;
            let now = views.add.rows.row_data(index).is_some_and(|r| r.wanted);
            views.add.set(index, !now);
            show_choice(&ui, &views.add);
        }
    });

    add.on_set_all({
        let (views, ui) = (views.clone(), ui.as_weak());
        move |wanted| {
            let Some(ui) = ui.upgrade() else { return };
            for i in 0..views.add.rows.row_count() {
                views.add.set(i, wanted);
            }
            show_choice(&ui, &views.add);
        }
    });

    // Same folder setting the preferences panel edits — changing it here is not
    // a per-torrent destination, it is the destination, chosen at the moment
    // somebody is actually thinking about it.
    add.on_change_destination({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<crate::Prefs>().invoke_pick_download_dir();
        }
    });
}

/// Open or update the dialog from a snapshot.
pub fn refresh(ui: &MainWindow, snapshot: &Snapshot, choice: &Choice) {
    let add = ui.global::<AddState>();

    let Some(pending) = &snapshot.pending else {
        // The engine has let go of it — confirmed, cancelled, or never asked.
        add.set_open(false);
        return;
    };

    // A different torrent, or the metadata just arrived: adopt it, which is also
    // what stops a second inspect inheriting the first one's ticks.
    if *choice.source.borrow() != *pending.source || choice.rows.row_count() != pending.files.len() {
        choice.adopt(pending);
    }

    push!(add, get_name, set_name, pending.name.as_ref().into());
    push!(add, get_fetching, set_fetching, pending.fetching);
    push!(add, get_error, set_error, pending.error.as_deref().unwrap_or_default().into());
    add.set_open(true);
    show_choice(ui, choice);
}

/// Push what is ticked, and whether Add can be pressed at all.
fn show_choice(ui: &MainWindow, choice: &Choice) {
    let add = ui.global::<AddState>();
    let (summary, chosen) = choice.summary();
    push!(add, get_summary, set_summary, summary.into());
    // Confirming with nothing ticked would add a torrent that downloads nothing.
    push!(add, get_can_add, set_can_add, chosen > 0 && !add.get_fetching() && add.get_error().is_empty());
}

/// Show the destination the next torrent will use.
pub fn show_destination(ui: &MainWindow, dir: &str) {
    push!(ui.global::<AddState>(), get_destination, set_destination, dir.into());
}

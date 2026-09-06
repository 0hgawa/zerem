//! The drawer: what is inside the selected torrent.
//!
//! The rule this module exists to enforce is in the architecture doc and is
//! about cost, not tidiness: **details are built only for the watched torrent,
//! and only while the drawer is open**. Closing it sends `WatchDetails(None)`,
//! and the engine stops assembling file lists and peer tables altogether.
//!
//! The two lists use plain `VecModel`s rather than the diffing model the main
//! table needs. They hold tens of rows, not thousands, and they exist only while
//! someone is looking at them — the diff would cost more to maintain than it
//! could ever save.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use slint::{
    ComponentHandle, Image, Model as _, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel,
};
use zerem_core::{fmt, Details, TorrentId};
use zerem_engine::{Command, Snapshot};

use crate::state::UiState;
use crate::{DetailState, FileEntry, MainWindow, PeerEntry};

pub struct Models {
    files: Rc<VecModel<FileEntry>>,
    peers: Rc<VecModel<PeerEntry>>,
    /// Each file's size and whether it is wanted — the two numbers the line
    /// above the list is built from. Held beside the model because the model
    /// carries sizes as finished strings, and because a tick has to move now
    /// rather than on the next tick, summary included.
    sizes: RefCell<Vec<(u64, bool)>>,
    /// Which lines are pinned, under the same in-flight rule as the ticks: a
    /// snapshot already on its way when the pin was clicked knows nothing about
    /// it and would un-light it for a quarter of a second.
    pins: RefCell<Vec<bool>>,
    /// The folders somebody has shut, by path.
    ///
    /// Shut and not open, so a torrent nobody has touched shows its whole tree
    /// — which is the answer the panel gave before it had folders at all, and
    /// the one somebody expects the first time they open one.
    ///
    /// By path rather than by row, because the list is rebuilt from every
    /// snapshot and a row number means nothing across two of them. It is not
    /// persisted and not cleared between torrents either: a set of paths is a
    /// few hundred bytes, and shutting `Disc 1` in one torrent and finding it
    /// shut in the next one that has a `Disc 1` is a coincidence nobody is
    /// harmed by.
    shut: RefCell<std::collections::BTreeSet<std::sync::Arc<str>>>,
    /// Every file's path, in the torrent's own order.
    ///
    /// Kept because a click on a folder has to find the files under it, and a
    /// folder that is shut has no rows to read them off. Filled beside `sizes`,
    /// from the same snapshot, so the two cannot disagree about how many files
    /// there are.
    paths: RefCell<Vec<std::sync::Arc<str>>>,
    /// The desktop's own icon for each extension seen so far.
    ///
    /// Cached because the answer is the same for every `.mkv` in a season pack
    /// and asking the shell is a COM call — once per kind, not once per file
    /// per second. A miss that comes back empty is cached too: a platform with
    /// no answer should be asked once, not forty times.
    icons: RefCell<HashMap<String, Image>>,
    /// The desktop's folder icon. Asked for once, on the first drawer that
    /// opens, and never again: there is one folder icon and it does not change
    /// while the app runs.
    folder: RefCell<Option<Image>>,
    /// The flag for each country seen so far.
    ///
    /// Keyed by the country and not by the peer, which is what it was at first:
    /// a swarm is fifty peers from a dozen countries, so twelve entries answer
    /// for fifty. Keyed by address it also grew without bound — every peer ever
    /// seen, on every port it ever reconnected from, for the life of the
    /// process. There are two hundred and thirty-nine countries and there
    /// always will be.
    ///
    /// An address nobody has been delegated keys the empty string, so it is
    /// looked up once rather than once a tick.
    flags: RefCell<HashMap<String, Image>>,
    /// Whose files these are. Taken from the details the drawer last drew
    /// rather than from the selection: a click acts on the torrent whose lines
    /// are on screen, which for one tick after the selection moves is not yet
    /// the selected one.
    shown: RefCell<Option<TorrentId>>,
    /// The sequence a file edit was made against, or 0 for none in flight.
    guess: Cell<u64>,
}

impl Models {
    #[must_use]
    pub fn new(ui: &MainWindow) -> Self {
        let this = Self {
            files: Rc::new(VecModel::default()),
            peers: Rc::new(VecModel::default()),
            sizes: RefCell::new(Vec::new()),
            pins: RefCell::new(Vec::new()),
            shut: RefCell::new(std::collections::BTreeSet::new()),
            paths: RefCell::new(Vec::new()),
            icons: RefCell::new(HashMap::new()),
            folder: RefCell::new(None),
            flags: RefCell::new(HashMap::new()),
            shown: RefCell::new(None),
            guess: Cell::new(0),
        };
        let detail = ui.global::<DetailState>();
        detail.set_files(ModelRc::from(this.files.clone()));
        detail.set_peers(ModelRc::from(this.peers.clone()));
        this
    }

    /// Take the snapshot's answer for which files are wanted — unless a click
    /// is still in flight.
    ///
    /// The same rule the table's optimistic edits follow: the guess is made
    /// against a sequence number, and the first snapshot published after it is
    /// the truth, whether the engine agreed or not. A snapshot that was already
    /// on its way when the click happened knows nothing about it and would put
    /// the tick back for a quarter of a second.
    fn adopt(&self, seq: u64, details: &Details) {
        // A different torrent drops the guess whatever its sequence says — two
        // torrents with the same number of files would otherwise inherit each
        // other's ticks.
        let same = self.shown.replace(Some(details.id)) == Some(details.id);

        let mut sizes = self.sizes.borrow_mut();
        let in_flight = same && seq <= self.guess.get() && sizes.len() == details.files.len();
        if !in_flight {
            self.guess.set(0);
            *sizes = details.files.iter().map(|f| (f.size, f.wanted)).collect();
            *self.paths.borrow_mut() = details.files.iter().map(|f| std::sync::Arc::clone(&f.path)).collect();
            *self.pins.borrow_mut() = details.files.iter().map(|f| f.first).collect();
        }
    }

    /// Which files sit under this folder, by the index the engine knows them by.
    ///
    /// Read off the paths rather than off the rows: a shut folder has no rows
    /// underneath it, and it is exactly the folder somebody is most likely to
    /// tick without opening first.
    fn files_under(&self, folder: &str) -> Vec<usize> {
        self.paths
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, path)| {
                path.len() > folder.len()
                    && path.starts_with(folder)
                    // The separator matters: without it `Disc 1` claims the
                    // files of `Disc 10`.
                    && path[folder.len()..].starts_with(std::path::is_separator)
            })
            .map(|(at, _)| at)
            .collect()
    }

    /// Flip one line, or every line, without waiting for the engine.
    ///
    /// The optimistic half of the rule the whole app follows: the tick moves on
    /// the click and the next snapshot is the truth — which puts it back if the
    /// engine said no.
    fn set_wanted(&self, file: Option<usize>, wanted: bool) {
        let mut sizes = self.sizes.borrow_mut();
        let range = match file {
            Some(i) if i < sizes.len() => i..i + 1,
            Some(_) => return,
            None => 0..sizes.len(),
        };
        for i in range {
            sizes[i].1 = wanted;
        }
        drop(sizes);

        // The rows are a tree, so a file's index is not its row any more —
        // setting row `i` would tick whatever happened to be there. Folder rows
        // are left alone and catch up on the next snapshot, a quarter of a
        // second later: what has to move on the click is the thing clicked, and
        // clicking a folder moves every file under it at once.
        for row in 0..self.files.row_count() {
            let Some(mut entry) = self.files.row_data(row) else { continue };
            let Ok(at) = usize::try_from(entry.at) else { continue };
            if let Some(&(_, now)) = self.sizes.borrow().get(at) {
                if entry.wanted != now {
                    entry.wanted = now;
                    self.files.set_row_data(row, entry);
                }
            }
        }
    }

    /// The desktop's icon for whatever this path ends in.
    ///
    /// Keyed by extension and folded to lower case, because `.MKV` and `.mkv`
    /// are one kind of file and asking twice would cache two copies of one
    /// picture.
    fn icon_for(&self, path: &str) -> Image {
        let extension = std::path::Path::new(path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if let Some(cached) = self.icons.borrow().get(&extension) {
            return cached.clone();
        }
        let image = zerem_shell::file_icon(&extension).map_or_else(Image::default, |bitmap| {
            let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(bitmap.width, bitmap.height);
            buffer.make_mut_bytes().copy_from_slice(&bitmap.rgba);
            Image::from_rgba8_premultiplied(buffer)
        });
        self.icons.borrow_mut().insert(extension, image.clone());
        image
    }

    /// The desktop's own folder icon.
    fn folder_icon(&self) -> Image {
        if let Some(cached) = self.folder.borrow().as_ref() {
            return cached.clone();
        }
        let image = zerem_shell::folder_icon().map_or_else(Image::default, |bitmap| {
            let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(bitmap.width, bitmap.height);
            buffer.make_mut_bytes().copy_from_slice(&bitmap.rgba);
            Image::from_rgba8_premultiplied(buffer)
        });
        *self.folder.borrow_mut() = Some(image.clone());
        image
    }

    /// The flag of whoever was given this address.
    ///
    /// The country lookup runs every time — it is a binary search and a short
    /// walk, and a peer list is fifty rows once a second. What is cached is the
    /// picture, which costs a decode and an allocation.
    ///
    /// An address nobody has been delegated, and a country the flag set does
    /// not carry, both come back as an empty image: the row keeps its slot and
    /// simply has a gap where the picture would be.
    fn flag_for(&self, addr: &str) -> Image {
        let country = host_of(addr).and_then(|host| host.parse().ok()).and_then(zerem_core::country);
        let key = country.unwrap_or_default();
        if let Some(cached) = self.flags.borrow().get(key) {
            return cached.clone();
        }
        let image = country.and_then(zerem_core::flag).map_or_else(Image::default, |flag| {
            let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(flag.width, flag.height);
            buffer.make_mut_bytes().copy_from_slice(&flag.pixels);
            Image::from_rgba8(buffer)
        });
        self.flags.borrow_mut().insert(key.to_owned(), image.clone());
        image
    }

    /// Whether a row's file has all of itself.
    fn is_complete(&self, index: i32) -> bool {
        usize::try_from(index).ok().and_then(|at| self.files.row_data(at)).is_some_and(|row| row.complete)
    }

    /// Where a row's file actually is on disk.
    ///
    /// The torrent's folder plus the path the torrent declares. `None` when the
    /// drawer is not on a torrent, when that torrent has no folder yet, or when
    /// the row is not one of its files — all of which mean the same thing to
    /// the caller, which is that there is nothing to open.
    fn path_of(&self, state: &UiState, index: i32) -> Option<std::path::PathBuf> {
        let id = *self.shown.borrow().as_ref()?;
        let snapshot = state.snapshot();
        let row = snapshot.torrents.iter().find(|t| t.id == id)?;
        if row.folder.is_empty() {
            return None;
        }
        let path = self.files.row_data(usize::try_from(index).ok()?)?.path;
        Some(std::path::Path::new(row.folder.as_ref()).join(path.as_str()))
    }

    /// Light or unlight one pin without waiting for the engine. Same optimistic
    /// rule the ticks follow.
    fn set_first(&self, index: usize, first: bool) {
        // The borrow ends before the model is touched, and that is not
        // fussiness: writing a row notifies whoever is watching it, and a
        // notification that came back into this type while the borrow was still
        // live would be a panic rather than a compile error. The model diff
        // learnt the same lesson and says so where it notifies.
        {
            let mut pins = self.pins.borrow_mut();
            let Some(slot) = pins.get_mut(index) else { return };
            *slot = first;
        }
        if let Some(mut row) = self.files.row_data(index) {
            row.first = first;
            self.files.set_row_data(index, row);
        }
    }

    /// Whether the line at `index` is being fetched before the others.
    fn is_first(&self, index: usize) -> bool {
        self.pins.borrow().get(index).copied().unwrap_or(false)
    }

    /// Record which snapshot a file edit was made against, so the one already
    /// in flight when it happened does not undraw it.
    fn expect(&self, seq: u64) {
        self.guess.set(seq);
    }

    /// Whether the line at `index` is currently being fetched.
    fn is_wanted(&self, index: usize) -> bool {
        self.sizes.borrow().get(index).is_some_and(|&(_, wanted)| wanted)
    }

    /// "12 files · 3.72 GB", or "3 of 12 files · 1.44 GB of 3.72 GB" once
    /// something has been left out — and whether anything has.
    ///
    /// A live pin replaces both: while one file is being fetched first, what
    /// matters is not the count, it is that the rest have stopped.
    fn choice(&self) -> (String, bool) {
        let sizes = self.sizes.borrow();
        let pins = self.pins.borrow();
        let pinned = (0..sizes.len()).filter(|&i| pins.get(i).copied().unwrap_or(false)).count();
        if pinned > 0 {
            let waiting =
                (0..sizes.len()).filter(|&i| sizes[i].1 && !pins.get(i).copied().unwrap_or(false)).count();
            return (fmt::fetching_first(pinned, waiting), true);
        }
        let total: u64 = sizes.iter().map(|&(size, _)| size).sum();
        let (count, bytes) = sizes
            .iter()
            .filter(|&&(_, wanted)| wanted)
            .fold((0_usize, 0_u64), |(n, b), &(size, _)| (n + 1, b + size));

        let text = zerem_core::text::files_choice(count, sizes.len(), &fmt::bytes(bytes), &fmt::bytes(total));
        (text, count != sizes.len())
    }
}

pub fn wire(
    ui: &MainWindow,
    state: &Rc<UiState>,
    store: &Rc<crate::settings::Store>,
    views: &Rc<super::Views>,
) {
    wire_menu(ui, state, views);

    let detail = ui.global::<DetailState>();

    detail.on_close({
        let (state, ui) = (state.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<DetailState>().set_open(false);
            // Not merely hidden: the engine stops building this.
            state.engine.send(Command::WatchDetails(None));
        }
    });

    detail.on_resize_begin({
        let state = state.clone();
        move || state.begin_drawer_resize()
    });

    detail.on_resize_move({
        let (state, store, ui) = (state.clone(), store.clone(), ui.as_weak());
        move |delta| {
            let Some(ui) = ui.upgrade() else { return };
            // Pushed straight back rather than waiting for the next tick: a
            // drag that lags its own cursor is the one gesture people notice.
            ui.global::<DetailState>().set_width(state.resize_drawer(delta));
            // Every pixel of the drag lands here; the 400 ms debounce in the
            // store is what turns the whole drag into one write.
            state.save_view(&store);
        }
    });

    detail.on_pick_tab({
        let ui = ui.as_weak();
        move |tab| {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<DetailState>().set_tab(tab);
        }
    });

    // Both of these name the file and the answer, never the whole selection:
    // the engine applies the change to what is current there, so a click made
    // against a second-old panel cannot undo anything that happened since.
    detail.on_toggle_file({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |index| {
            let Some(ui) = ui.upgrade() else { return };
            let Ok(index) = usize::try_from(index) else { return };
            let wanted = !views.detail.is_wanted(index);
            views.detail.expect(state.snapshot().seq);
            views.detail.set_wanted(Some(index), wanted);
            show_choice(&ui, &views.detail);
            if let Some(id) = *views.detail.shown.borrow() {
                state.engine.send(Command::SetFileWanted { id, files: Some(vec![index]), wanted });
            }
        }
    });

    detail.on_retry({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let Some(id) = *views.detail.shown.borrow() else { return };
            // A real retry: librqbit re-initialises an errored torrent, hashes
            // what is on disk and carries on from there.
            state.engine.send(Command::Start(id));
            super::refresh_now(&ui, &state, &views);
        }
    });

    detail.on_toggle_first({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |index| {
            let Some(ui) = ui.upgrade() else { return };
            let Ok(index) = usize::try_from(index) else { return };
            let first = !views.detail.is_first(index);
            views.detail.expect(state.snapshot().seq);
            views.detail.set_first(index, first);
            show_choice(&ui, &views.detail);
            if let Some(id) = *views.detail.shown.borrow() {
                state.engine.send(Command::SetFileFirst { id, file: index, first });
            }
        }
    });

    wire_folders(ui, state, views);

    detail.on_set_all_files({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |wanted| {
            let Some(ui) = ui.upgrade() else { return };
            views.detail.expect(state.snapshot().seq);
            views.detail.set_wanted(None, wanted);
            show_choice(&ui, &views.detail);
            if let Some(id) = *views.detail.shown.borrow() {
                state.engine.send(Command::SetFileWanted { id, files: None, wanted });
            }
        }
    });

    ui.global::<crate::TorrentList>().on_toggle_details({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let detail = ui.global::<DetailState>();
            detail.set_open(!detail.get_open());
            follow_selection(&ui, &state);
            // Redrawn at once so the drawer is not blank for up to a tick after
            // it opens — the first snapshot with details is a moment away.
            super::refresh_now(&ui, &state, &views);
        }
    });
}

/// The right-click menu on a file: where it opened, and the two things it can
/// do that nothing else in the app offers.
fn wire_menu(ui: &MainWindow, state: &Rc<UiState>, views: &Rc<super::Views>) {
    let detail = ui.global::<DetailState>();

    // The three verbs, acting on the torrent this panel is about.
    //
    // Not on the selection: Ctrl-clicking three rows shows the first in the
    // panel, and a Remove that took the other two with it would be acting on
    // something nobody is looking at.
    detail.on_toggle_running({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let Some(id) = *views.detail.shown.borrow() else { return };
            let snapshot = state.snapshot();
            let Some(row) = snapshot.torrents.iter().find(|t| t.id == id) else { return };
            if row.is_active() {
                state.engine.send(Command::Pause(id));
                state.expect_pause(id, snapshot.seq);
            } else {
                state.engine.send(Command::Start(id));
                state.expect_start(id, snapshot.seq);
            }
            super::refresh_now(&ui, &state, &views);
        }
    });

    detail.on_open_folder({
        let (state, views) = (state.clone(), views.clone());
        move || {
            let Some(id) = *views.detail.shown.borrow() else { return };
            let snapshot = state.snapshot();
            let folder = snapshot.torrents.iter().find(|t| t.id == id).map(|t| t.folder.clone());
            match folder.filter(|f| !f.is_empty()) {
                Some(folder) => zerem_shell::reveal(std::path::Path::new(folder.as_ref())),
                None => state.set_notice(zerem_core::tr("That torrent has no folder yet")),
            }
        }
    });

    detail.on_remove({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let Some(id) = *views.detail.shown.borrow() else { return };
            // Through the same confirmation every other route uses. Removing is
            // the one destructive thing the app does and it asks once, in one
            // place, however it was reached.
            super::torrents::confirm_remove(&ui, &state, vec![id]);
        }
    });

    detail.on_open_file_menu({
        let ui = ui.as_weak();
        move |index, x, y| {
            let Some(ui) = ui.upgrade() else { return };
            let detail = ui.global::<DetailState>();
            detail.set_file_menu_index(index);
            detail.set_file_menu_x(x);
            detail.set_file_menu_y(y);
            detail.set_file_menu_open(true);
        }
    });

    detail.on_close_file_menu({
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                ui.global::<DetailState>().set_file_menu_open(false);
            }
        }
    });

    detail.on_open_file({
        let (state, views, ui) = (state.clone(), views.clone(), ui.as_weak());
        move |index| {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<DetailState>().set_file_menu_open(false);
            // The menu greys this row out on a file that has not arrived; a
            // double click has no greyed-out state to show, so it says so. A
            // partial video opens as a few seconds and a codec error, which is
            // a worse answer than being told to wait.
            if !views.detail.is_complete(index) {
                state.set_notice(zerem_core::tr("That file has not finished yet"));
                return;
            }
            match views.detail.path_of(&state, index) {
                Some(path) => zerem_shell::open(&path),
                None => state.set_notice(zerem_core::tr("That file is not on disk yet")),
            }
        }
    });

    detail.on_play_file({
        let (state, views, ui) = (state.clone(), views.clone(), ui.as_weak());
        move |index| {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<DetailState>().set_file_menu_open(false);
            let snapshot = state.snapshot();
            let Some(id) = *views.detail.shown.borrow() else { return };
            let Ok(file) = usize::try_from(index) else { return };
            match snapshot.stream.as_ref() {
                // The whole point: handed to a player *now*, whether or not
                // the file has finished. The engine's reader waits for the
                // pieces it needs and tells the picker to fetch those first.
                Some(at) => zerem_shell::open_url(&at.url(id.0, file)),
                None => state.set_notice(zerem_core::tr("Streaming is not available")),
            }
        }
    });

    detail.on_reveal_file({
        let (state, views, ui) = (state.clone(), views.clone(), ui.as_weak());
        move |index| {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<DetailState>().set_file_menu_open(false);
            match views.detail.path_of(&state, index) {
                Some(path) => zerem_shell::reveal(&path),
                None => state.set_notice(zerem_core::tr("That file is not on disk yet")),
            }
        }
    });
}

/// Put the remembered drawer width back, once, at startup.
///
/// Not part of `refresh`: it changes only when someone drags the edge, and the
/// drag pushes it itself.
pub fn show_width(ui: &MainWindow, state: &UiState) {
    ui.global::<DetailState>().set_width(state.drawer_width());
}

/// Tell the engine what to watch, or to stop.
///
/// Called whenever the drawer opens or closes and whenever the selection moves.
/// With several rows selected it watches the first — a drawer showing three
/// torrents at once would be showing none of them.
pub fn follow_selection(ui: &MainWindow, state: &UiState) {
    let open = ui.global::<DetailState>().get_open();
    let watching = open.then(|| first_selected(state)).flatten();
    state.engine.send(Command::WatchDetails(watching));
}

fn first_selected(state: &UiState) -> Option<TorrentId> {
    // By view order rather than whatever the set hands over, so "the first" is
    // the one nearest the top of the table.
    (0..).map_while(|i| state.model.id_at(i)).find(|id| state.selection().contains(id))
}

/// Push a snapshot's details into the drawer. Does nothing when it is shut.
// The one float this app pushes, and `push!` compares before it sets. Bit
// equality is exactly the question being asked — "is this the same value I
// pushed last time" — and not a numeric closeness the lint assumes it is.
#[allow(clippy::float_cmp, reason = "the comparison is `did it change`, not `is it near`")]
pub fn refresh(ui: &MainWindow, snapshot: &Snapshot, models: &Models) {
    let detail = ui.global::<DetailState>();
    if !detail.get_open() {
        return;
    }

    // A torrent that is no longer there takes its panel with it.
    //
    // Removing one from inside the panel left the panel showing it: the engine
    // stops building details for a torrent it no longer has, `details` goes
    // absent, and the early return below kept the last frame on screen — a
    // whole panel of a torrent that had been deleted, with buttons that still
    // offered to pause it.
    //
    // Closed rather than moved to a neighbour, and the reason is what this
    // panel is: it follows the selection, and after a delete there is no
    // selection. Choosing the next row would be the app picking a torrent on
    // somebody's behalf — and "next" here is whatever the current sort put
    // there, which by size or by speed is an unrelated torrent rather than the
    // next thing you were dealing with.
    if let Some(shown) = *models.shown.borrow() {
        if !snapshot.torrents.iter().any(|t| t.id == shown) {
            detail.set_open(false);
            return;
        }
    }

    let Some(details) = &snapshot.details else {
        // Watched but not built yet — the command and the tick can cross.
        return;
    };

    let row = snapshot.torrents.iter().find(|t| t.id == details.id);
    let name = row.map_or_else(SharedString::default, |t| t.name.as_ref().into());
    push!(detail, get_title, set_title, name);
    // The state column already carries this, elided into whatever fits. Here
    // there is room to read it, and the one button that acts on it beside it.
    push!(
        detail,
        get_fault,
        set_fault,
        row.and_then(|t| t.error.as_deref()).map_or_else(SharedString::default, Into::into)
    );
    // How it is doing, from the same row the table draws so the two cannot
    // disagree. Pushed rather than derived here: the formatting is already done
    // once for the table, and doing it twice is two chances to differ.
    if let Some(row) = row {
        push!(detail, get_progress, set_progress, row.progress_bp() as f32 / 10_000.0);
        push!(detail, get_progress_text, set_progress_text, fmt::progress(row.done, row.size).into());
        push!(detail, get_state, set_state, row.status_text().into());
        push!(detail, get_kind, set_kind, row.status_kind());
        push!(detail, get_running, set_running, row.is_active());
        push!(detail, get_down, set_down, fmt::speed(row.down_bps).into());
        push!(detail, get_up, set_up, fmt::speed(row.up_bps).into());
        // Empty rather than "∞" when there is no time to give. The table's ETA
        // column has a heading saying what it is, so an infinity there reads as
        // "not finishing"; alone on a line beside the state it is a symbol with
        // no question attached to it.
        push!(
            detail,
            get_eta,
            set_eta,
            row.eta.map_or_else(SharedString::default, |secs| fmt::eta(Some(secs)).into())
        );
        push!(detail, get_ratio, set_ratio, fmt::ratio(row.ratio_x100).into());
        push!(detail, get_swarm, set_swarm, fmt::peers(row.peers_connected, row.peers_total).into());
    }
    push!(detail, get_files_summary, set_files_summary, details.files.len().to_string().into());
    push!(detail, get_peers_summary, set_peers_summary, details.peers.len().to_string().into());

    models.adopt(snapshot.seq, details);
    // Gathered before the borrow below, because looking one up can insert one.
    let icons: Vec<Image> = details.files.iter().map(|f| models.icon_for(&f.path)).collect();
    apply(
        &models.files,
        build_files(details, &models.sizes.borrow(), &models.pins.borrow(), &icons, &models.shut.borrow()),
    );
    let flags: Vec<Image> = details.peers.iter().map(|p| models.flag_for(&p.addr)).collect();
    apply(&models.peers, build_peers(details, &flags));
    show_choice(ui, models);
}

/// Push what is being fetched, and whether there is anything to put back.
fn show_choice(ui: &MainWindow, models: &Models) {
    let detail = ui.global::<DetailState>();
    let (summary, partial) = models.choice();
    push!(detail, get_files_choice, set_files_choice, summary.into());
    push!(detail, get_files_partial, set_files_partial, partial);
    // The folder the files sit in is the torrent's own name, which the title
    // already carries — read from there rather than derived a second time.
    push!(detail, get_files_folder, set_files_folder, detail.get_title());
    if detail.get_folder_icon().size().width == 0 {
        detail.set_folder_icon(models.folder_icon());
    }
}

/// Replace a model's contents, reusing the rows that did not change.
///
/// Not the diffing machinery the table uses — just enough to stop a fifty-row
/// file list being torn down and rebuilt every second, which is what would throw
/// away the scroll position while someone is reading it.
fn apply<T: Clone + PartialEq + 'static>(model: &Rc<VecModel<T>>, next: Vec<T>) {
    if model.row_count() != next.len() {
        model.set_vec(next);
        return;
    }
    for (i, row) in next.into_iter().enumerate() {
        if model.row_data(i).as_ref() != Some(&row) {
            model.set_row_data(i, row);
        }
    }
}

/// `chosen` is what the drawer believes, which is the snapshot's answer except
/// while a click is still in flight — see [`Models::adopt`].
///
/// `shut` is the folders somebody has closed, by path. Closed rather than open,
/// so a torrent nobody has touched shows its whole tree — which is the answer
/// the panel gave before it had folders at all.
fn build_files(
    details: &Details,
    chosen: &[(u64, bool)],
    pins: &[bool],
    icons: &[Image],
    shut: &std::collections::BTreeSet<std::sync::Arc<str>>,
) -> Vec<FileEntry> {
    // The optimistic edits go on before the tree is built rather than after: a
    // folder adds up what is under it, and adding up the snapshot's answer
    // while the ticks show the click's would draw a half-ticked folder over a
    // column of ticked files.
    let files: Vec<zerem_core::detail::FileRow> = details
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| zerem_core::detail::FileRow {
            wanted: chosen.get(i).map_or(f.wanted, |&(_, wanted)| wanted),
            first: pins.get(i).copied().unwrap_or(f.first),
            ..f.clone()
        })
        .collect();

    zerem_core::tree::flatten(&files, &|path| shut.contains(path))
        .into_iter()
        .map(|node| FileEntry {
            playable: node.at.is_some() && zerem_core::is_playable(&node.path),
            path: node.path.as_ref().into(),
            name: node.name.as_ref().into(),
            // `-1` for a folder, which nothing indexes with. Every callback
            // taking a file takes this, and the `.slint` asks `is-folder`
            // before it uses one.
            at: node.at.map_or(-1, |at| i32::try_from(at).unwrap_or(-1)),
            depth: i32::try_from(node.depth).unwrap_or(0),
            is_folder: node.is_folder(),
            open: node.open,
            files: i32::try_from(node.files).unwrap_or(0),
            partial: node.wanted == zerem_core::tree::Wanted::Part,
            size: fmt::bytes(node.size).into(),
            pct: fmt::percent(node.done, node.size).into(),
            progress: node.progress_bp() as f32 / 10_000.0,
            complete: node.size > 0 && node.done >= node.size,
            wanted: node.wanted != zerem_core::tree::Wanted::None,
            first: node.first,
            icon: node.at.and_then(|at| icons.get(at).cloned()).unwrap_or_default(),
        })
        .collect()
}

/// The address out of a `host:port`, brackets and all.
///
/// librqbit reports a socket address, and an IPv6 one wears square brackets
/// that `IpAddr` will not parse. Splitting on the *last* colon, because an IPv6
/// address is mostly colons.
fn host_of(addr: &str) -> Option<&str> {
    let host = addr.rsplit_once(':').map_or(addr, |(host, _)| host);
    Some(host.trim_start_matches('[').trim_end_matches(']')).filter(|host| !host.is_empty())
}

fn build_peers(details: &Details, flags: &[Image]) -> Vec<PeerEntry> {
    details
        .peers
        .iter()
        .enumerate()
        .map(|(i, p)| PeerEntry {
            addr: p.addr.as_ref().into(),
            flag: flags.get(i).cloned().unwrap_or_default(),
            // An unnamed peer is one that has not said, not one called "".
            client: p.client.as_deref().unwrap_or("unknown").into(),
            transport: p.transport.label().into(),
            state: p.state.into(),
            down: fmt::bytes(p.downloaded).into(),
            up: fmt::bytes(p.uploaded).into(),
        })
        .collect()
}

/// The two the file tree added, in their own function: `wire` was over the
/// hundred lines this workspace allows, and these are the pair that pushed it
/// there. They belong together anyway -- one opens a folder and the other ticks
/// one, and nothing else in the panel knows what a folder is.
fn wire_folders(ui: &MainWindow, state: &Rc<UiState>, views: &Rc<super::Views>) {
    let detail = ui.global::<DetailState>();
    // Open or shut a folder. Nothing is sent to the engine: which folders are
    // showing is the window's own business, and the engine has no opinion about
    // it. The list is rebuilt at once rather than on the next tick, because a
    // chevron that turns a quarter of a second after the click reads as a click
    // that missed.
    detail.on_toggle_folder({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |path| {
            let Some(ui) = ui.upgrade() else { return };
            {
                let mut shut = views.detail.shut.borrow_mut();
                let path: std::sync::Arc<str> = std::sync::Arc::from(path.as_str());
                if !shut.remove(&path) {
                    shut.insert(path);
                }
            }
            super::refresh_now(&ui, &state, &views);
        }
    });

    // Tick or untick everything under a folder, in one command.
    //
    // A folder that is partly wanted ticks whole, which is the answer somebody
    // clicking a half-ticked box is asking for — the other reading, "untick the
    // rest", is a thing nobody wants a checkbox to do.
    detail.on_toggle_folder_files({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |path| {
            let Some(ui) = ui.upgrade() else { return };
            let under = views.detail.files_under(&path);
            if under.is_empty() {
                return;
            }
            let wanted = !under.iter().all(|&i| views.detail.is_wanted(i));
            views.detail.expect(state.snapshot().seq);
            for &i in &under {
                views.detail.set_wanted(Some(i), wanted);
            }
            show_choice(&ui, &views.detail);
            if let Some(id) = *views.detail.shown.borrow() {
                state.engine.send(Command::SetFileWanted { id, files: Some(under), wanted });
            }
        }
    });
}

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
    /// The desktop's own icon for each extension seen so far.
    ///
    /// Cached because the answer is the same for every `.mkv` in a season pack
    /// and asking the shell is a COM call — once per kind, not once per file
    /// per second. A miss that comes back empty is cached too: a platform with
    /// no answer should be asked once, not forty times.
    icons: RefCell<HashMap<String, Image>>,
    /// The flag for each country seen so far, under the same rule as the icons:
    /// a swarm is fifty peers from a dozen countries, and decoding one flag per
    /// peer per second would be decoding the same twelve pictures over and over.
    /// A peer whose address is in space nobody has been given caches an empty
    /// image, so the table is asked once and not once a tick.
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
            icons: RefCell::new(HashMap::new()),
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
            *self.pins.borrow_mut() = details.files.iter().map(|f| f.first).collect();
        }
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
            if let Some(mut row) = self.files.row_data(i) {
                row.wanted = wanted;
                self.files.set_row_data(i, row);
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

    /// The flag of whoever was given this address.
    ///
    /// Keyed by the address rather than the country because the country is what
    /// the lookup costs — a walk through the table — and the peer list hands the
    /// same addresses back every tick. An address nobody has been delegated, and
    /// a country the flag set does not carry, both cache an empty image: the
    /// answer will not change, and the row simply has a gap where the picture
    /// would be.
    fn flag_for(&self, addr: &str) -> Image {
        if let Some(cached) = self.flags.borrow().get(addr) {
            return cached.clone();
        }
        let image = addr
            .rsplit_once(':')
            .map_or(addr, |(host, _)| host.trim_start_matches('[').trim_end_matches(']'))
            .parse()
            .ok()
            .and_then(zerem_core::country)
            .and_then(zerem_core::flag)
            .map_or_else(Image::default, |flag| {
                let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(flag.width, flag.height);
                buffer.make_mut_bytes().copy_from_slice(&flag.pixels);
                Image::from_rgba8(buffer)
            });
        self.flags.borrow_mut().insert(addr.to_owned(), image.clone());
        image
    }

    /// Light or unlight one pin without waiting for the engine. Same optimistic
    /// rule the ticks follow.
    fn set_first(&self, index: usize, first: bool) {
        let mut pins = self.pins.borrow_mut();
        let Some(slot) = pins.get_mut(index) else { return };
        *slot = first;
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
                state.engine.send(Command::SetFileWanted { id, file: Some(index), wanted });
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

    detail.on_set_all_files({
        let (state, ui, views) = (state.clone(), ui.as_weak(), views.clone());
        move |wanted| {
            let Some(ui) = ui.upgrade() else { return };
            views.detail.expect(state.snapshot().seq);
            views.detail.set_wanted(None, wanted);
            show_choice(&ui, &views.detail);
            if let Some(id) = *views.detail.shown.borrow() {
                state.engine.send(Command::SetFileWanted { id, file: None, wanted });
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
pub fn refresh(ui: &MainWindow, snapshot: &Snapshot, models: &Models) {
    let detail = ui.global::<DetailState>();
    if !detail.get_open() {
        return;
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
    push!(detail, get_files_summary, set_files_summary, details.files.len().to_string().into());
    push!(detail, get_peers_summary, set_peers_summary, details.peers.len().to_string().into());

    models.adopt(snapshot.seq, details);
    // Gathered before the borrow below, because looking one up can insert one.
    let icons: Vec<Image> = details.files.iter().map(|f| models.icon_for(&f.path)).collect();
    apply(&models.files, build_files(details, &models.sizes.borrow(), &models.pins.borrow(), &icons));
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
fn build_files(details: &Details, chosen: &[(u64, bool)], pins: &[bool], icons: &[Image]) -> Vec<FileEntry> {
    details
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| FileEntry {
            path: f.path.as_ref().into(),
            size: fmt::bytes(f.size).into(),
            pct: fmt::percent(f.done, f.size).into(),
            progress: f.progress_bp() as f32 / 10_000.0,
            complete: f.is_complete(),
            wanted: chosen.get(i).map_or(f.wanted, |&(_, wanted)| wanted),
            first: pins.get(i).copied().unwrap_or(f.first),
            icon: icons.get(i).cloned().unwrap_or_default(),
        })
        .collect()
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

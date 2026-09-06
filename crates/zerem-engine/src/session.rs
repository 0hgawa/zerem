//! The librqbit session, wrapped so the rest of the program never sees it.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use librqbit::api::TorrentIdOrHash;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session};
use zerem_core::{Content, History, Pending, PendingFile, State, TorrentId, TorrentRow, Waiting};

use crate::config::EngineConfig;
use crate::journal::{Journal, Roster};
use crate::map;
use crate::snapshot::Snapshot;

/// How many ticks a notice stays on screen before it clears itself.
const NOTICE_TICKS: u8 = 8;

/// One torrent, plus what librqbit makes expensive to ask for repeatedly.
pub struct Entry {
    pub handle: Arc<ManagedTorrent>,
    /// `None` until a magnet's metadata arrives. Cached as `Arc<str>` because
    /// `handle.name()` allocates a fresh `String` on every call, and the diff
    /// in the UI depends on the name being pointer-identical between ticks.
    name: Option<Arc<str>>,
    name_key: Option<Arc<str>>,
    /// Both fixed for the torrent's life, so they are resolved once. Asking the
    /// handle every tick would allocate a String per row per second for values
    /// that never change.
    pub folder: Arc<str>,
    info_hash: Arc<str>,
    /// What the torrent holds. `None` until the metadata arrives, and resolved
    /// exactly once after that: a file list does not change, and walking a few
    /// thousand of them per row per second would be the most expensive thing
    /// in the tick by a wide margin.
    content: Option<Content>,
    /// What this torrent has been doing lately — the smoothed figures, and how
    /// long it has been standing still. State that spans ticks lives with the
    /// torrent rather than on the row it produces.
    trend: map::Trend,
    /// The user's ticks. Empty until the metadata arrives, and the truth after
    /// that: while a file is pinned the handle's own `only_files` is narrower
    /// than what was asked for, so it can no longer be read back as the answer.
    pub wanted: Vec<bool>,
    /// Which files to fetch before the others. Held here and nowhere else —
    /// a pin is an instruction given in a moment, not a setting, so it does not
    /// survive a restart. What does survive is the selection it narrowed, which
    /// is what [`crate::journal`] is for.
    pub first: Vec<bool>,
    /// Whether the user wants this running — which is not whether it *is*.
    ///
    /// The queue owns librqbit's paused flag, so that flag stopped being the
    /// answer to "did somebody stop this?". This is the intent the queue works
    /// from, and what it paused is written down so a restart can tell the two
    /// apart again.
    pub wanted_running: bool,
    /// Its turn. Lower goes first; pressing Start moves it below everything.
    pub position: i64,
    /// Whether it was already finished last tick.
    ///
    /// `None` until it has been seen once, which is what stops every torrent
    /// that was already complete announcing itself the moment the app opens.
    pub was_complete: Option<bool>,
}

impl Entry {
    fn new(handle: Arc<ManagedTorrent>) -> Self {
        Self {
            folder: Arc::from(handle.output_folder().to_string_lossy().as_ref()),
            info_hash: Arc::from(format!("{:?}", handle.info_hash()).as_str()),
            handle,
            name: None,
            name_key: None,
            content: None,
            trend: map::Trend::default(),
            wanted: Vec::new(),
            first: Vec::new(),
            wanted_running: true,
            position: 0,
            was_complete: None,
        }
    }

    /// Size the two per-file lists once there is a file list, seeding the ticks
    /// from whatever the session was already fetching.
    ///
    /// Returns the file count, or zero while a magnet is still without its
    /// metadata — which is the one state in which there is nothing to choose.
    fn resolve_files(&mut self) -> usize {
        if !self.wanted.is_empty() {
            return self.wanted.len();
        }
        let count = self.handle.with_metadata(|meta| meta.file_infos.len()).unwrap_or(0);
        if count == 0 {
            return 0;
        }
        self.wanted = zerem_core::flags(self.handle.only_files().as_deref(), count);
        self.first = vec![false; count];
        count
    }

    /// Which files are already on disk, which is what ends a pin.
    fn complete(&self, stats: &librqbit::TorrentStats) -> Vec<bool> {
        self.handle
            .with_metadata(|meta| {
                meta.file_infos
                    .iter()
                    .enumerate()
                    .map(|(i, info)| stats.file_progress.get(i).copied().unwrap_or(0) >= info.len)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolve the name and its folded sort key if they have arrived. Returns
    /// what to display meanwhile.
    fn resolve_name(&mut self) -> (Arc<str>, Arc<str>) {
        if let (Some(name), Some(key)) = (&self.name, &self.name_key) {
            return (name.clone(), key.clone());
        }
        let Some(name) = self.handle.name() else {
            // A magnet before its metadata: the infohash is all there is, and
            // it is better than a blank row. It is already lower case, so it is
            // its own sort key — and, usefully, its own filter key.
            return (self.info_hash.clone(), self.info_hash.clone());
        };
        let name: Arc<str> = Arc::from(name.as_str());
        let key: Arc<str> = Arc::from(name.to_lowercase().as_str());
        self.name = Some(name.clone());
        self.name_key = Some(key.clone());
        (name, key)
    }

    /// Classify the file list if it has arrived.
    ///
    /// An `Unknown` answer is deliberately not cached: it means the metadata is
    /// still in flight, and caching it would freeze the row on the placeholder
    /// icon for the rest of the session.
    fn resolve_content(&mut self) -> Content {
        if let Some(content) = self.content {
            return content;
        }
        let resolved = self
            .handle
            .with_metadata(|meta| {
                Content::of(meta.file_infos.iter().map(|f| (f.relative_filename.as_path(), f.len)))
            })
            .unwrap_or(Content::Unknown);
        if resolved != Content::Unknown {
            self.content = Some(resolved);
        }
        resolved
    }
}

/// Something the user needs to be told, shown in the status bar.
struct Notice {
    text: Arc<str>,
    ticks_left: u8,
}

pub struct TorrentSession {
    pub session: Arc<Session>,
    /// Where a finished magnet read sends its answer.
    reads: tokio::sync::mpsc::UnboundedSender<Read>,
    /// And where the loop collects them. Taken out once, by
    /// [`Self::take_reads`]: the loop selects on it, and a receiver borrowed
    /// from `self` inside a `select!` cannot coexist with the other arms that
    /// need `&mut self`.
    reads_rx: Option<tokio::sync::mpsc::UnboundedReceiver<Read>>,
    pub entries: HashMap<TorrentId, Entry>,
    seq: u64,
    generation: u64,
    notice: Option<Notice>,
    /// Where the *next* torrent goes. Held here rather than read from the
    /// session because librqbit fixes its default folder at construction, and
    /// changing where new downloads land must not mean restarting.
    output_dir: PathBuf,
    /// Whose insides to build. `None` while no panel is open, which is the
    /// point: closed means the work is not done.
    watching: Option<TorrentId>,
    /// What the add dialog is asking about, and the bytes to add it with.
    /// Kept together because one without the other is meaningless.
    pending: Option<Pending>,
    /// `Vec<u8>` rather than librqbit's `Bytes`, which is not re-exported.
    /// `AddTorrent::from_bytes` takes anything that converts.
    pending_bytes: Option<Vec<u8>>,
    /// Where a torrent goes once it has arrived, or `None` for staying put.
    ///
    /// Off by default: moving somebody's files is not something to start doing
    /// because an update shipped.
    pub keep_dir: Option<PathBuf>,
    /// Torrents that crossed into complete on the last publish and have not
    /// been dealt with yet.
    ///
    /// Recorded in `publish`, which is synchronous, and acted on afterwards by
    /// the tick — moving files is neither quick nor synchronous.
    arrived: Vec<TorrentId>,
    /// The one move in flight, if any. One at a time: two large copies at
    /// once turn a sequential read into a seeking one.
    pub moving: Option<crate::arrival::Move>,
    /// The folder torrents are picked up from, or `None` for none.
    pub(crate) watch_dir: Option<PathBuf>,
    /// Ticks since the last sweep of it.
    pub(crate) since_sweep: u8,
    /// The loopback server a media player is pointed at, once it has a port.
    ///
    /// `None` on a machine that would not give one, where the app runs and the
    /// Play row is simply not offered.
    pub(crate) streamer: Option<crate::stream::Streamer>,
    /// The last minute of session throughput, for the footer.
    history: History,
    /// Selections narrowed by a pin, so an interrupted run can put them back.
    journal: Journal,
    /// How many torrents may download at once. Zero is no limit, the same
    /// convention the transfer caps use.
    limit: u32,
    /// The next position handed to a torrent added, and to one sent to the
    /// head of the queue. Signed and open at both ends, so neither move has to
    /// renumber anything else.
    next_back: i64,
    next_front: i64,
    /// What the queue paused, so a restart can tell its own work from the
    /// user's.
    queued: Roster,
    /// Whether the next torrent added is added stopped.
    add_paused: bool,
}

/// How long a magnet is given to produce a file list.
///
/// librqbit gives it forever, which is defensible for a download and not for a
/// dialog somebody is sitting in front of. Two minutes is far longer than a
/// healthy swarm needs and short enough that a task cannot be left running for
/// the life of the process — which is what an unbounded wait on a link nobody
/// seeds actually is.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(120);

/// A finished read, on its way back to the loop.
pub struct Read {
    /// What was asked for. The dialog may have moved on to another link, and
    /// the answer has to be able to say which question it belongs to.
    source: Arc<str>,
    pending: Pending,
    /// The metainfo, so accepting the dialog does not ask the swarm twice.
    /// Absent when the read failed.
    bytes: Option<Vec<u8>>,
}

/// Ask the swarm what is in a torrent, on whatever task is running this.
///
/// Free of `TorrentSession` on purpose: it takes only the librqbit session,
/// which is an `Arc`, so it can be moved onto a task while the engine's loop
/// carries on with everything else.
async fn read_torrent(session: &Arc<Session>, source: Arc<str>) -> Read {
    let read = async {
        let add = AddTorrent::from_cli_argument(&source)
            .context("that is not a magnet link, a URL, or a .torrent file")?;
        let listed = tokio::time::timeout(
            PATIENCE,
            session.add_torrent(add, Some(AddTorrentOptions { list_only: true, ..Default::default() })),
        )
        .await
        .map_err(|_| anyhow::anyhow!("nobody answered with the file list"))?
        .context("reading the torrent")?;

        match listed {
            AddTorrentResponse::ListOnly(listed) => Ok(listed),
            // Already in the list. Not an error worth a dialog — the user gets
            // told, and nothing is added twice.
            AddTorrentResponse::AlreadyManaged(..) | AddTorrentResponse::Added(..) => {
                anyhow::bail!("that torrent is already in the list")
            }
        }
    };

    match read.await {
        Ok(listed) => {
            let files: Vec<PendingFile> = listed
                .info
                .iter_file_details()
                .map(|f| PendingFile {
                    path: Arc::from(f.filename.to_pathbuf().to_string_lossy().as_ref()),
                    size: f.len,
                    wanted: true,
                })
                .collect();
            let name: Arc<str> = Arc::from(listed.info.name().unwrap_or_default().as_ref());
            Read {
                source: Arc::clone(&source),
                pending: Pending {
                    name: if name.is_empty() { Arc::clone(&source) } else { name },
                    source,
                    info_hash: Arc::from(format!("{:?}", listed.info_hash).as_str()),
                    files,
                    fetching: false,
                    error: None,
                },
                bytes: Some(listed.torrent_bytes.to_vec()),
            }
        }
        Err(e) => Read { pending: Pending::failed(&source, &format!("{e:#}")), source, bytes: None },
    }
}

impl TorrentSession {
    pub async fn start(config: &EngineConfig) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&config.download_dir)
            .with_context(|| format!("creating {}", config.download_dir.display()))?;

        // Checked before the session is built, because librqbit does not fail
        // when a listener cannot bind — it logs and carries on. A taken UDP port
        // means uTP is silently off for the whole run, which is precisely the
        // kind of quiet degradation nobody notices until they wonder why seeding
        // is slow.
        let port_conflict = port_in_use(config.port);

        let session = Session::new_with_opts(config.download_dir.clone(), config.to_session_options())
            .await
            .context("starting the BitTorrent session")?;

        // `new_with_opts` re-adds every persisted torrent before it returns, and
        // keeps their ids. The list is already there; this only adopts it.
        let entries: HashMap<TorrentId, Entry> = session.with_torrents(|torrents| {
            torrents.map(|(id, handle)| (TorrentId(id as u32), Entry::new(handle.clone()))).collect()
        });

        tracing::info!(
            addr = ?session.listen_addr(),
            dir = %config.download_dir.display(),
            state = %config.state_dir.display(),
            utp = config.utp,
            restored = entries.len(),
            "session started"
        );

        let (reads_tx, reads_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut this = Self {
            watch_dir: config.watch_dir.clone(),
            since_sweep: 0,
            keep_dir: config.keep_dir.clone(),
            arrived: Vec::new(),
            moving: None,
            streamer: None,
            session,
            entries,
            seq: 0,
            generation: 1,
            notice: None,
            output_dir: config.download_dir.clone(),
            watching: None,
            reads: reads_tx,
            reads_rx: Some(reads_rx),
            pending: None,
            pending_bytes: None,
            history: History::default(),
            journal: Journal::open(&config.state_dir),
            limit: config.max_active,
            next_back: 0,
            next_front: 0,
            queued: Roster::open(&config.state_dir, "queued.txt"),
            add_paused: config.add_paused,
        };
        this.adopt_queue();
        this.restore_narrowed().await;
        if let Some(protocol) = port_conflict {
            // Both places: the status bar so the user knows, and the log so it
            // is still diagnosable from a bug report with no screenshot.
            tracing::warn!(port = config.port, protocol, "the listening port is already taken");
            this.notify(format!(
                "Port {} is already in use ({protocol}) — another torrent client may be running. \
                 Incoming connections will not reach Zerem.",
                config.port
            ));
        }
        // Started after the session exists, and given a way to find a torrent
        // rather than a reference to one: the server runs on its own tasks and
        // the session is a single `&mut self`, so what crosses between them is
        // a question and its answer.
        let session = this.session.clone();
        this.streamer = crate::stream::Streamer::start(std::sync::Arc::new(move |id: TorrentId| {
            session.get(TorrentIdOrHash::Id(id.0 as usize))
        }))
        .await;

        Ok(this)
    }

    /// The oldest torrent that finished and has not been dealt with.
    pub fn next_arrived(&mut self) -> Option<TorrentId> {
        (!self.arrived.is_empty()).then(|| self.arrived.remove(0))
    }

    /// Drop the whole list, for when nothing is going to be done with it.
    pub fn forget_arrived(&mut self) {
        self.arrived.clear();
    }

    /// Say something in the status bar. Named apart from `notify` so the intent
    /// reads at the call site: this one is always about something going wrong.
    pub(crate) fn report(&mut self, text: &str) {
        self.notify(text);
    }

    /// Where torrents are picked up from now on. `None` switches it off.
    pub fn set_watch_dir(&mut self, folder: Option<PathBuf>) {
        self.watch_dir = folder;
    }

    /// Where finished torrents go from now on. `None` switches it off.
    pub fn set_keep_dir(&mut self, folder: Option<PathBuf>) {
        self.keep_dir = folder;
    }

    /// Stop a torrent so its files can move, without recording that somebody
    /// wanted it stopped.
    ///
    /// `set_running(false)` would, and the torrent would come back paused after
    /// a move nobody asked to pause it for.
    pub(crate) async fn pause_for_move(&self, id: TorrentId) -> anyhow::Result<()> {
        let handle = self.entries.get(&id).context("no such torrent")?.handle.clone();
        // Already paused is the state we want, not an error.
        let _ = self.session.pause(&handle).await;
        Ok(())
    }

    /// Drop a torrent from the session, keeping every file on disk.
    pub(crate) async fn forget(&self, id: TorrentId) -> anyhow::Result<()> {
        self.session
            .delete(TorrentIdOrHash::Id(id.0 as usize), false)
            .await
            .context("letting go of the torrent before adding it back")
    }

    /// Put a rebuilt torrent in the old one's place.
    ///
    /// It keeps its place in the queue and its own row in the table, so a move
    /// does not send a finished torrent to the back of a line it has already
    /// left. The id changes because librqbit hands out a new one; everything
    /// above this layer is keyed on the id, so the generation is bumped and the
    /// UI rebuilds its order.
    pub(crate) fn adopt_moved(&mut self, was: TorrentId, handle: Arc<ManagedTorrent>) {
        let old = self.entries.remove(&was);
        let id = TorrentId(handle.id() as u32);
        let mut entry = Entry::new(handle);
        if let Some(old) = old {
            entry.position = old.position;
            entry.wanted_running = old.wanted_running;
            // It has just been verified as complete, so it is complete: without
            // this the rebuilt torrent would announce itself as newly finished
            // the moment its check passes.
            entry.was_complete = Some(true);
        }
        self.entries.insert(id, entry);
        // The drawer follows an id, and the id has just changed. Without this
        // a panel open on the torrent that moved would empty itself and stay
        // empty: `publish` looks the watched id up and finds nothing.
        if self.watching == Some(was) {
            self.watching = Some(id);
        }
        self.generation += 1;
    }

    pub fn notify(&mut self, text: impl Into<Arc<str>>) {
        self.notice = Some(Notice { text: text.into(), ticks_left: NOTICE_TICKS });
    }

    /// Read a torrent without starting it.
    ///
    /// `list_only` is what makes this possible: librqbit fetches the metadata —
    /// from the swarm, for a magnet — and hands back the file list plus the
    /// torrent bytes, without touching the disk. The bytes are kept so
    /// confirming does not fetch a second time.
    /// Start reading a torrent, and come back at once.
    ///
    /// The fetch runs on a task of its own and the answer arrives through
    /// [`Self::take_reads`]. That shape is the whole point: it used to be
    /// awaited inside the engine's loop, and a magnet is metadata that comes
    /// from a swarm with no promise about when — so a link nobody was seeding
    /// stopped the loop. Not the dialog: the *loop*. Every other torrent's
    /// figures froze where they stood, every later command queued behind it
    /// unanswered, and nothing recovered on its own, because librqbit puts no
    /// timeout on resolving a magnet and neither did this.
    ///
    /// It looked alive, because the window still redrew. It was not. It was one
    /// paste away from being a picture of itself.
    pub fn begin_inspect(&mut self, source: &str) {
        self.pending = Some(Pending::fetching(source));

        let session = Arc::clone(&self.session);
        let sender = self.reads.clone();
        let source: Arc<str> = Arc::from(source);
        tokio::spawn(async move {
            // The loop is gone if the app is closing, and a read nobody is
            // waiting for is not a failure.
            let _ = sender.send(read_torrent(&session, source).await);
        });
    }

    /// The channel finished reads arrive on. Taken once, by the loop.
    pub const fn take_reads(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Read>> {
        self.reads_rx.take()
    }

    /// Put a finished read in front of the dialog.
    ///
    /// Ignored unless the dialog is still waiting on *this* source. Two pastes
    /// in a row leave two reads in flight and the first to answer is not
    /// necessarily the one being asked about — without this, a slow first
    /// magnet would land on top of a fast second one seconds after the dialog
    /// had already filled itself in.
    pub fn adopt_read(&mut self, read: Read) {
        if self.pending.as_ref().is_none_or(|p| p.source != read.source) {
            return;
        }
        self.pending_bytes = read.bytes;
        self.pending = Some(read.pending);
    }

    /// Accept what `inspect` found, into a folder of the caller's choosing.
    ///
    /// `folder` is a rename of the torrent's own subfolder and nothing more:
    /// librqbit takes the file names from the metadata, so the files inside
    /// cannot be renamed and the dialog does not pretend they can. An empty
    /// name, or one that is not a single ordinary path component, falls back to
    /// the torrent's own — the same guard the derived name gets, because a name
    /// typed by hand deserves it no less than one that came from a stranger.
    pub async fn confirm_add(
        &mut self,
        only_files: Option<Vec<usize>>,
        folder: Option<String>,
        destination: Option<String>,
    ) -> anyhow::Result<()> {
        let bytes = self.pending_bytes.take().context("nothing was read to add")?;
        let pending = self.pending.take().context("nothing was read to add")?;
        let source = pending.source.to_string();
        let named = folder.filter(|f| zerem_core::subfolder(f, pending.files.len()).is_some());
        // Where this one goes, which is not always where the next one will: a
        // shelf has a folder of its own and the session's default is untouched.
        let root = destination.map_or_else(|| self.output_dir.clone(), PathBuf::from);
        let sub = zerem_core::subfolder(named.as_deref().unwrap_or(&pending.name), pending.files.len());
        let folder = sub.map_or_else(|| root.clone(), |name| root.join(name)).to_string_lossy().into_owned();

        // From the bytes `inspect` already has, so a magnet is not fetched from
        // the swarm a second time.
        let handle = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(bytes),
                Some(AddTorrentOptions {
                    only_files,
                    overwrite: true,
                    output_folder: Some(folder),
                    paused: self.add_paused,
                    ..Default::default()
                }),
            )
            .await
            .with_context(|| format!("adding {source}"))?
            .into_handle()
            .context("the torrent was listed rather than added")?;

        let id = TorrentId(handle.id() as u32);
        self.entries.insert(id, Entry::new(handle));
        self.generation += 1;
        Ok(())
    }

    /// Where this torrent writes: the download folder, and inside it a folder of
    /// the torrent's own when it holds more than one file.
    ///
    /// librqbit does this itself — until it is told where to write, which takes
    /// the branch that skips the subfolder entirely. Zerem has to tell it,
    /// because the download folder changes while the app runs and librqbit
    /// fixes its own at construction with no setter. So the rule is ours, and
    /// it lives in [`zerem_core::folder`] where it can be tested.
    fn folder_for(&self, name: &str, files: usize) -> String {
        let root = zerem_core::subfolder(name, files)
            .map_or_else(|| self.output_dir.clone(), |sub| self.output_dir.join(sub));
        root.to_string_lossy().into_owned()
    }

    /// Throw away what `inspect` found.
    pub fn cancel_add(&mut self) {
        self.pending = None;
        self.pending_bytes = None;
    }

    /// Add without asking. Used for what arrives on the command line, where
    /// there is no dialog to answer.
    ///
    /// Read first, then added — the same two steps the dialog takes, for a
    /// reason that has nothing to do with dialogs: the folder a multi-file
    /// torrent goes in is its own name, and its name is in the metadata. For a
    /// magnet that means waiting on the swarm, which is what adding it was
    /// going to cost anyway.
    pub async fn add(&mut self, source: &str) -> anyhow::Result<()> {
        let read = AddTorrent::from_cli_argument(source)
            .context("that is not a magnet link, a URL, or a .torrent file")?;
        let listed = match self
            .session
            .add_torrent(read, Some(AddTorrentOptions { list_only: true, ..Default::default() }))
            .await
            .context("reading the torrent")?
        {
            AddTorrentResponse::ListOnly(listed) => listed,
            // Already in the list. Not a failure worth a red row: nothing is
            // added twice and the user is told.
            AddTorrentResponse::AlreadyManaged(..) | AddTorrentResponse::Added(..) => {
                anyhow::bail!("that torrent is already in the list")
            }
        };

        let name = listed.info.name().unwrap_or_default().to_string();
        let files = listed.info.iter_file_details().count();
        let folder = self.folder_for(&name, files);

        let handle = self
            .session
            .add_torrent(
                // From the bytes just read, so a magnet is not fetched from the
                // swarm a second time.
                AddTorrent::from_bytes(listed.torrent_bytes.to_vec()),
                Some(AddTorrentOptions {
                    // Not optional, whatever the default says. From librqbit's
                    // own docs: "Even when all the torrent pieces have been
                    // written, `overwrite` needs to be enabled in order to
                    // resume/seed the torrent." Without it every already-present
                    // torrent fails to add with "file exists".
                    overwrite: true,
                    // Passed per torrent, not taken from the session: this is
                    // what lets the download folder change without a restart.
                    output_folder: Some(folder),
                    paused: self.add_paused,
                    ..Default::default()
                }),
            )
            .await
            .context("adding the torrent")?
            .into_handle()
            .context("the torrent was listed rather than added")?;

        let id = TorrentId(handle.id() as u32);
        self.entries.insert(id, Entry::new(handle));
        // The row set moved, so every cached ordering above is stale.
        self.generation += 1;
        Ok(())
    }

    /// Apply global transfer caps, in bytes per second.
    ///
    /// librqbit's limiters are settable while the session runs, so this takes
    /// effect on the next piece rather than the next launch.
    pub fn set_limits(&self, down: Option<u32>, up: Option<u32>) {
        self.session.ratelimits.set_download_bps(NonZeroU32::new(down.unwrap_or(0)));
        self.session.ratelimits.set_upload_bps(NonZeroU32::new(up.unwrap_or(0)));
        tracing::info!(?down, ?up, "transfer limits changed");
    }

    /// Where torrents added from now on are written.
    pub fn set_output_dir(&mut self, dir: PathBuf) -> anyhow::Result<()> {
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        tracing::info!(dir = %dir.display(), "new torrents will be written here");
        self.output_dir = dir;
        Ok(())
    }

    /// Give every restored torrent a place in line, and work out which of them
    /// the queue stopped rather than the user.
    ///
    /// Order is by id, which is the order they were added — librqbit hands the
    /// ids out in sequence and keeps them across restarts. A jump to the head
    /// of the queue is deliberately not persisted: it is an instruction given
    /// in a moment, the same as pinning a file.
    fn adopt_queue(&mut self) {
        let mut ids: Vec<TorrentId> = self.entries.keys().copied().collect();
        ids.sort_unstable();
        for (place, id) in ids.into_iter().enumerate() {
            let Some(entry) = self.entries.get_mut(&id) else { continue };
            entry.position = place as i64;
            // A torrent that is stopped is the user's doing — unless this is
            // where we wrote down that it was ours.
            let paused = matches!(entry.handle.stats().state, librqbit::TorrentStatsState::Paused);
            entry.wanted_running = !paused || self.queued.holds(&entry.info_hash);
        }
        self.next_back = self.entries.len() as i64;
    }

    /// Put back selections a previous run narrowed and did not live to restore.
    ///
    /// The pins themselves are gone on purpose: "fetch this one first" is an
    /// instruction given in a moment. Coming back days later to a torrent still
    /// holding the rest of itself back would be the app remembering the wrong
    /// half of what happened.
    async fn restore_narrowed(&mut self) {
        if self.journal.is_empty() {
            return;
        }
        // Bound first because the loop mutates `self`, which the iterator's
        // borrow of `entries` would otherwise still be holding.
        let ids: Vec<TorrentId> = self.entries.keys().copied().collect();
        for id in ids {
            let Some(info_hash) = self.entries.get(&id).map(|e| e.info_hash.clone()) else {
                continue;
            };
            let Some(wanted) = self.journal.take(&info_hash) else { continue };
            let Some(entry) = self.entries.get_mut(&id) else { continue };
            let count = entry.resolve_files();
            if count == 0 {
                continue;
            }
            entry.wanted = zerem_core::flags(Some(&wanted), count);
            entry.first = vec![false; count];
            tracing::info!(id = id.0, files = wanted.len(), "restoring a narrowed selection");
            if let Err(e) = self.apply_choice(id).await {
                tracing::warn!(id = id.0, error = %format!("{e:#}"), "could not restore it");
            }
        }
        self.journal.flush();
    }

    /// Ask the session for whatever the ticks and the pins currently add up to.
    ///
    /// The only place `update_only_files` is called, so the journal and the
    /// session can never end up disagreeing about what was narrowed.
    async fn apply_choice(&mut self, id: TorrentId) -> anyhow::Result<()> {
        let Some(entry) = self.entries.get(&id) else { return Ok(()) };
        let count = entry.wanted.len();
        let complete = entry.complete(&entry.handle.stats());
        let narrowed = zerem_core::is_narrowed(&entry.wanted, &entry.first, &complete);
        // `None` is "no restriction", which librqbit's setter has no way to say
        // — it takes a set, so the whole torrent is spelled out.
        let target: Vec<usize> = zerem_core::to_fetch(&entry.wanted, &entry.first, &complete)
            .unwrap_or_else(|| (0..count).collect());
        let handle = entry.handle.clone();
        let info_hash = entry.info_hash.clone();

        // Already the answer. Saying so costs a comparison; not saying so costs
        // a chunk-tracker rebuild, and on a finished torrent a pause/unpause.
        // Sorted first because librqbit stores the set through a `HashSet` and
        // hands it back in whatever order that produced.
        let mut current = handle.only_files().unwrap_or_else(|| (0..count).collect());
        current.sort_unstable();
        if current == target {
            return Ok(());
        }

        // Written before the change, not after: the window this survives is a
        // process that dies between the two.
        let ticked: Vec<usize> =
            (0..count).filter(|&i| entry.wanted.get(i).copied().unwrap_or(false)).collect();
        self.journal.set(&info_hash, narrowed.then_some(ticked));

        self.session
            .update_only_files(&handle, &target.iter().copied().collect())
            .await
            .context("changing which files are downloaded")?;
        tracing::info!(id = id.0, picked = target.len(), of = count, narrowed, "file selection changed");
        Ok(())
    }

    /// Release what a finished pin was holding, and forget the pin.
    ///
    /// Costs a `stats()` call per *pinned* torrent, which is normally none of
    /// them — the whole list is never walked.
    pub async fn reconcile(&mut self) {
        let pinned: Vec<TorrentId> =
            self.entries.iter().filter(|(_, e)| e.first.iter().any(|&f| f)).map(|(id, _)| *id).collect();
        for id in pinned {
            if let Some(entry) = self.entries.get_mut(&id) {
                // A pin that has landed has done its job; leaving it set would
                // show a file as still being hurried after it arrived.
                let complete = entry.complete(&entry.handle.stats());
                for (pin, done) in entry.first.iter_mut().zip(&complete) {
                    *pin &= !*done;
                }
            }
            if let Err(e) = self.apply_choice(id).await {
                tracing::warn!(id = id.0, error = %format!("{e:#}"), "could not widen the selection");
            }
        }
    }

    /// Fetch a file, or stop fetching it, on a torrent already in the list.
    ///
    /// The current selection is read from our own record rather than from the
    /// caller, so a click made against a second-old panel cannot undo anything
    /// that happened since. `file` of `None` is every file at once.
    ///
    /// It cannot be read from the handle any more either: while a pin is live
    /// the session is fetching one file, which is not what the user ticked.
    ///
    /// The refusals are deliberate and both reach the status bar as a sentence:
    /// a change that would leave the torrent fetching nothing, and a torrent
    /// whose metadata has not arrived — there is nothing to choose from yet, and
    /// librqbit refuses it as well.
    pub async fn set_file_wanted(
        &mut self,
        id: TorrentId,
        files: Option<&[usize]>,
        wanted: bool,
    ) -> anyhow::Result<()> {
        let entry = self.entries.get_mut(&id).context("no such torrent")?;
        let count = entry.resolve_files();
        anyhow::ensure!(count > 0, "this torrent's file list has not arrived yet");

        entry.wanted = zerem_core::ticked(&entry.wanted, files, wanted).context("no such file")?;

        // Un-ticking a file drops its pin with it: a file nobody is fetching
        // cannot be the one being fetched first.
        for (pin, want) in entry.first.iter_mut().zip(&entry.wanted) {
            *pin &= *want;
        }
        self.apply_choice(id).await?;
        Ok(())
    }

    /// Fetch this file before the others, or stop doing that.
    ///
    /// While anything is pinned it is the *only* thing coming down. That is not
    /// a queue position like qBittorrent's — librqbit has no such thing to
    /// offer — and for the case people reach for priority in, it is the better
    /// bargain: the whole line goes to the file that was asked for.
    pub async fn set_file_first(&mut self, id: TorrentId, file: usize, first: bool) -> anyhow::Result<()> {
        let entry = self.entries.get_mut(&id).context("no such torrent")?;
        let count = entry.resolve_files();
        anyhow::ensure!(count > 0, "this torrent's file list has not arrived yet");
        anyhow::ensure!(
            !first || entry.wanted.get(file).copied().unwrap_or(false),
            "that file is not being downloaded"
        );
        *entry.first.get_mut(file).context("no such file")? = first;
        self.apply_choice(id).await
    }

    /// Start or stop a torrent, on the user's say-so.
    ///
    /// Start also means *now*: it goes to the head of the queue, which is the
    /// only thing an explicit click can mean while something else is holding
    /// the slots. Without that, pressing Start on a queued torrent would look
    /// like the app ignoring the click.
    ///
    /// Applied straight away rather than left to the queue, so the ordinary
    /// case — no limit set at all — behaves exactly as it did before there was
    /// a queue, and pays nothing for it.
    pub async fn set_running(&mut self, id: TorrentId, running: bool) -> anyhow::Result<()> {
        let front = self.next_front;
        let entry = self.entries.get_mut(&id).context("no such torrent")?;
        entry.wanted_running = running;
        if running {
            entry.position = front;
        }
        let handle = entry.handle.clone();

        if running {
            self.next_front -= 1;
            self.session.unpause(&handle).await.context("starting")?;
        } else {
            self.session.pause(&handle).await.context("pausing")?;
        }
        self.enforce_queue().await;
        Ok(())
    }

    /// Whether the next torrent added is added stopped.
    pub const fn set_add_paused(&mut self, paused: bool) {
        self.add_paused = paused;
    }

    /// How many torrents may download at once. Zero is no limit.
    pub async fn set_max_active(&mut self, limit: u32) {
        self.limit = limit;
        tracing::info!(limit, "how many download at once changed");
        self.enforce_queue().await;
    }

    /// Make what is running match what the queue says should be.
    ///
    /// Called every tick. The first line is what keeps that free for somebody
    /// who never set a limit: with no limit and nothing held back there is
    /// nothing to decide, and this is the pass that would otherwise ask every
    /// torrent for its stats once a second to reach that conclusion.
    ///
    /// Seeding is never queued — see [`zerem_core::queue`] for why.
    pub async fn enforce_queue(&mut self) {
        if self.limit == 0 && self.queued.is_empty() {
            return;
        }

        // One pass for the stats, because asking twice costs twice.
        let mut waiting = Vec::with_capacity(self.entries.len());
        let mut state_of = HashMap::with_capacity(self.entries.len());
        for (id, entry) in &self.entries {
            let stats = entry.handle.stats();
            waiting.push(Waiting {
                id: *id,
                position: entry.position,
                wanted: entry.wanted_running,
                complete: stats.finished,
            });
            let paused = matches!(stats.state, librqbit::TorrentStatsState::Paused);
            // A failed torrent is left alone in both directions: librqbit
            // refuses to pause one, and starting it is a retry the user asks
            // for rather than something a queue should do behind their back.
            let failed = matches!(stats.state, librqbit::TorrentStatsState::Error);
            state_of.insert(*id, (paused, failed));
        }

        let admitted: HashSet<TorrentId> = zerem_core::admit(&waiting, self.limit).into_iter().collect();

        let mut held = HashSet::new();
        for w in &waiting {
            let Some(&(paused, failed)) = state_of.get(&w.id) else { continue };
            let Some(handle) = self.entries.get(&w.id).map(|e| e.handle.clone()) else { continue };
            let should_run = admitted.contains(&w.id);

            if w.wanted && !should_run {
                if let Some(entry) = self.entries.get(&w.id) {
                    held.insert(entry.info_hash.to_string());
                }
            }
            if failed {
                continue;
            }
            if should_run && paused {
                if let Err(e) = self.session.unpause(&handle).await {
                    tracing::warn!(id = w.id.0, error = %e, "could not start a torrent's turn");
                }
            } else if !should_run && !paused && w.wanted {
                if let Err(e) = self.session.pause(&handle).await {
                    tracing::warn!(id = w.id.0, error = %e, "could not hold a torrent back");
                }
            }
        }
        self.queued.keep(held);
    }

    /// Remove a torrent, optionally sending its data to the recycle bin.
    ///
    /// librqbit is asked to delete *nothing*: its own `delete_files` unlinks,
    /// and a mis-click has to stay recoverable. The files are collected first,
    /// then handed to the recycle bin once the session has let go of them.
    pub async fn remove(&mut self, id: TorrentId, delete_data: bool) -> anyhow::Result<()> {
        let Some(entry) = self.entries.remove(&id) else {
            anyhow::bail!("no such torrent");
        };
        self.generation += 1;

        let doomed = if delete_data { collect_files(&entry.handle) } else { Vec::new() };
        let folder = entry.handle.output_folder().to_path_buf();
        drop(entry);

        self.session
            .delete(TorrentIdOrHash::Id(id.0 as usize), false)
            .await
            .context("removing the torrent")?;

        if !doomed.is_empty() {
            let count = doomed.len();
            trash_all(&folder, &doomed)?;
            tracing::info!(count, "moved files to the recycle bin");
        }
        Ok(())
    }

    /// Which torrent's insides to build into each snapshot. `None` stops.
    pub const fn watch_details(&mut self, id: Option<TorrentId>) {
        self.watching = id;
    }

    /// Drop the throughput history.
    ///
    /// Called when the view resumes after the window was hidden. Nothing was
    /// sampled while it was away, so keeping the old samples would splice two
    /// separate minutes together and label the result "the last sixty seconds".
    pub fn forget_history(&mut self) {
        self.history.clear();
    }

    /// Build the snapshot the UI draws.
    pub fn publish(&mut self) -> Arc<Snapshot> {
        self.seq += 1;

        let mut torrents: Vec<TorrentRow> = Vec::with_capacity(self.entries.len());
        let mut finished = Vec::new();
        for (id, entry) in &mut self.entries {
            let (name, name_key) = entry.resolve_name();
            let content = entry.resolve_content();
            // Bound first: `stats()` borrows the handle, and the rates it feeds
            // are a sibling field of the same entry.
            let stats = entry.handle.stats();
            let mut row = map::to_row(*id, &name, &name_key, &stats, &mut entry.trend);
            row.folder = entry.folder.clone();
            row.info_hash = entry.info_hash.clone();
            row.content = content;
            // Nothing was asked for. Not a fault the swarm caused, and the row
            // has to say it or the header tick looks like it did nothing.
            if !entry.wanted.is_empty() && !entry.wanted.iter().any(|w| *w) {
                row.stall = Some(zerem_core::Stall::NoFiles);
            }
            // Stopped because something else is ahead of it, not because
            // somebody stopped it. Showing both as "Paused" is how a queue
            // reads as an app that ignored the click.
            if row.state == State::Paused && entry.wanted_running && self.queued.holds(&entry.info_hash) {
                row.state = State::Queued;
            }
            // The moment it crossed, and only that moment. The first sighting
            // records without announcing, so a library of finished torrents
            // does not all shout at once when the app opens.
            let complete = row.size > 0 && row.is_complete();
            if entry.was_complete.replace(complete) == Some(false) && complete {
                finished.push(row.name.clone());
                self.arrived.push(*id);
            }
            torrents.push(row);
        }

        // The rows come out of a `HashMap`, so the order they arrive in is the
        // map`s and not anybody`s. Sorted by id, it becomes a pure function of
        // which torrents exist.
        //
        // That matters more than it looks. The window caches an *order* -- a
        // list of indices into this vector -- and rebuilds it only when the
        // generation changes. If two publishes with the same generation
        // disagreed about which row sits at index four, every cached index
        // would point at a different torrent and the table would draw one
        // torrent's figures under another's name. It cannot happen today
        // because every mutation of `entries` bumps the generation, but that is
        // four adjacent lines agreeing rather than a guarantee: a fifth
        // mutation that forgot would produce exactly that, silently. Sorting
        // costs microseconds on a list this size and takes the whole class of
        // failure off the table.
        torrents.sort_unstable_by_key(|row| row.id.0);

        // Only the watched one, and only while something is watching.
        let details = self
            .watching
            .and_then(|id| Some((id, self.entries.get(&id)?)))
            .map(|(id, entry)| map::to_details(id, &entry.handle, &entry.wanted, &entry.first));

        let notice = self.notice.as_mut().and_then(|n| {
            n.ticks_left = n.ticks_left.saturating_sub(1);
            (n.ticks_left > 0).then(|| n.text.clone())
        });
        if notice.is_none() {
            self.notice = None;
        }

        // Every one of the four, or the surface that reads the missing one is
        // simply dead: `pending` was absent here, and the add dialog therefore
        // never opened once — nothing could be added through the window at all.
        let snapshot = Snapshot::new(self.seq, self.generation, torrents)
            .with_notice(notice)
            .with_details(details)
            .with_finished(finished)
            .with_pending(self.pending.clone())
            .with_stream(self.streamer.as_ref().map(crate::stream::Streamer::at));

        // Recorded from the totals the snapshot just derived, so the footer
        // plots exactly the figures the status bar prints beside it.
        self.history.push(snapshot.stats.down_bps, snapshot.stats.up_bps);
        Arc::new(snapshot.with_history(self.history))
    }
}

/// Which protocol's listener the port is already taken on, if either.
///
/// Both are checked because they fail differently and matter differently: TCP
/// taken costs incoming peers, UDP taken costs uTP entirely. A bind that
/// succeeds here is released immediately and librqbit takes it a moment later —
/// so this can be raced, and is a diagnosis rather than a lock. Catching the
/// ordinary case (a second client already running) is what it is for.
fn port_in_use(port: u16) -> Option<&'static str> {
    use std::net::{Ipv6Addr, SocketAddr, TcpListener, UdpSocket};

    let addr = SocketAddr::from((Ipv6Addr::UNSPECIFIED, port));
    match (TcpListener::bind(addr), UdpSocket::bind(addr)) {
        (Err(_), Err(_)) => Some("TCP and UDP"),
        (Err(_), Ok(_)) => Some("TCP"),
        (Ok(_), Err(_)) => Some("UDP, so uTP is off"),
        (Ok(_), Ok(_)) => None,
    }
}

/// Absolute paths of everything this torrent wrote.
///
/// Taken from the metadata rather than guessed from the output folder: a
/// single-file torrent writes straight into it while a multi-file one makes a
/// subfolder, and getting that wrong means deleting the wrong thing.
fn collect_files(handle: &ManagedTorrent) -> Vec<PathBuf> {
    let root = handle.output_folder();
    handle
        .with_metadata(|meta| {
            meta.file_infos
                .iter()
                .map(|f| root.join(&f.relative_filename))
                .filter(|p| is_inside(root, p))
                .collect()
        })
        .unwrap_or_default()
}

/// Guard against a `relative_filename` that climbs out of the output folder.
/// The paths come from a torrent file, which is to say from a stranger.
fn is_inside(root: &Path, path: &Path) -> bool {
    path.components().all(|c| c != std::path::Component::ParentDir) && path.starts_with(root)
}

/// Move files to the recycle bin, then any now-empty directories they left.
fn trash_all(root: &Path, files: &[PathBuf]) -> anyhow::Result<()> {
    // Deepest first, so a subfolder is empty by the time it is considered.
    let mut dirs: Vec<PathBuf> =
        files.iter().filter_map(|f| f.parent()).filter(|d| *d != root).map(Path::to_path_buf).collect();
    dirs.sort_unstable();
    dirs.dedup();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));

    let existing: Vec<&PathBuf> = files.iter().filter(|f| f.exists()).collect();
    if !existing.is_empty() {
        trash::delete_all(&existing).context("moving the files to the recycle bin")?;
    }
    for dir in dirs {
        // Only if we emptied it — never a folder that still holds something the
        // user put there.
        if dir.read_dir().is_ok_and(|mut d| d.next().is_none()) {
            let _ = trash::delete(&dir);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_inside;
    use std::path::Path;

    #[test]
    fn a_path_that_climbs_out_of_the_output_folder_is_refused() {
        // Filenames come out of a torrent file, which is to say from a stranger.
        let root = Path::new("/downloads");
        assert!(is_inside(root, Path::new("/downloads/film/part1.mkv")));
        assert!(!is_inside(root, Path::new("/downloads/../etc/passwd")));
        assert!(!is_inside(root, Path::new("/etc/passwd")));
    }

    #[test]
    fn the_output_folder_itself_counts_as_inside() {
        let root = Path::new("/downloads");
        assert!(is_inside(root, Path::new("/downloads/single.iso")));
    }

    #[test]
    fn a_free_port_reports_no_conflict() {
        // Port 0 always binds — the OS picks a free one.
        assert_eq!(super::port_in_use(0), None);
    }

    #[test]
    fn a_taken_udp_port_is_named_as_costing_utp() {
        use std::net::{Ipv6Addr, SocketAddr, UdpSocket};

        // Hold a UDP port and nothing else, which is exactly the case that
        // silently turns uTP off for a whole run.
        let held = UdpSocket::bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)))
            .expect("bind an ephemeral UDP port");
        let port = held.local_addr().expect("its address").port();
        assert_eq!(super::port_in_use(port), Some("UDP, so uTP is off"));
    }
}

/// The stuck-magnet reproduction, run by hand.
///
/// Not part of the suite: it needs a swarm, a network and up to a minute, and a
/// test that fails because somebody's wifi dropped is a test people learn to
/// ignore. It exists because "paste a magnet and the dialog never fills" is a
/// report that cannot be chased any other way — it is exactly the two calls the
/// window makes, timed.
///
/// ```text
/// cargo test -p zerem-engine --lib inspecting_a_magnet -- --ignored --nocapture
/// ```
#[cfg(test)]
mod swarm {
    /// Sintel, the Blender Foundation's open film. A magnet with a live swarm
    /// and no question about who may distribute it.
    const SINTEL: &str = "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10\
        &tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce\
        &tr=udp%3A%2F%2Fexplodie.org%3A6969\
        &tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A6969%2Fannounce";

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "asks a real swarm for a real file list"]
    async fn inspecting_a_magnet_finishes() {
        let scratch = std::env::temp_dir().join(format!("zerem-swarm-{}", std::process::id()));
        let config = crate::EngineConfig {
            download_dir: scratch.join("downloads"),
            state_dir: scratch.join("state"),
            port: 0,
            ..crate::EngineConfig::default()
        };

        let mut session = super::TorrentSession::start(&config).await.expect("the session should start");

        // The loop's half of it, done by hand: take the channel, start the
        // read, wait on the answer. `begin_inspect` returns at once now — that
        // it does is the fix, so the test says it before it says anything else.
        let mut reads = session.take_reads().expect("a fresh session hands these over");

        let began = std::time::Instant::now();
        session.begin_inspect(SINTEL);
        assert!(began.elapsed() < std::time::Duration::from_millis(50), "it waited for the swarm");
        assert!(session.publish().pending.as_ref().is_some_and(|p| p.fetching), "it did not say so");

        let read = reads.recv().await.expect("the read task should answer");
        session.adopt_read(read);
        let took = began.elapsed();

        let snapshot = session.publish();
        let pending = snapshot.pending.as_ref().expect("something should be pending");
        println!("took {took:.1?}, {} files, error: {:?}", pending.files.len(), pending.error);
        assert!(pending.error.is_none(), "the swarm was asked and answered: {:?}", pending.error);
        assert!(!pending.fetching, "it is still saying it is fetching");
        assert!(!pending.files.is_empty(), "a file list with no files in it");

        let _ = std::fs::remove_dir_all(&scratch);
    }
}

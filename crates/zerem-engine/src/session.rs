//! The librqbit session, wrapped so the rest of the program never sees it.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use librqbit::api::TorrentIdOrHash;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session};
use zerem_core::{Content, History, Pending, PendingFile, TorrentId, TorrentRow};

use crate::config::EngineConfig;
use crate::map;
use crate::snapshot::Snapshot;

/// How many ticks a notice stays on screen before it clears itself.
const NOTICE_TICKS: u8 = 8;

/// One torrent, plus what librqbit makes expensive to ask for repeatedly.
struct Entry {
    handle: Arc<ManagedTorrent>,
    /// `None` until a magnet's metadata arrives. Cached as `Arc<str>` because
    /// `handle.name()` allocates a fresh `String` on every call, and the diff
    /// in the UI depends on the name being pointer-identical between ticks.
    name: Option<Arc<str>>,
    name_key: Option<Arc<str>>,
    /// Both fixed for the torrent's life, so they are resolved once. Asking the
    /// handle every tick would allocate a String per row per second for values
    /// that never change.
    folder: Arc<str>,
    info_hash: Arc<str>,
    /// What the torrent holds. `None` until the metadata arrives, and resolved
    /// exactly once after that: a file list does not change, and walking a few
    /// thousand of them per row per second would be the most expensive thing
    /// in the tick by a wide margin.
    content: Option<Content>,
    /// The smoothed download and upload figures. Filter state spans ticks, so
    /// it lives with the torrent rather than on the row it produces.
    rates: map::Rates,
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
            rates: map::Rates::default(),
        }
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
    session: Arc<Session>,
    entries: HashMap<TorrentId, Entry>,
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
    /// The last minute of session throughput, for the footer.
    history: History,
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

        let mut this = Self {
            session,
            entries,
            seq: 0,
            generation: 1,
            notice: None,
            output_dir: config.download_dir.clone(),
            watching: None,
            pending: None,
            pending_bytes: None,
            history: History::default(),
        };
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
        Ok(this)
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
    /// Say that a torrent is being read, before reaching for it.
    ///
    /// Split from the fetch so the engine can publish in between. A magnet's
    /// metadata comes from the swarm and takes seconds; a dialog that appears
    /// only once it has arrived leaves the click looking ignored, which is what
    /// makes someone click four more times.
    pub fn begin_inspect(&mut self, source: &str) {
        self.pending = Some(Pending::fetching(source));
    }

    /// Read it. [`Self::begin_inspect`] has already announced that this is
    /// happening.
    pub async fn finish_inspect(&mut self, source: &str) {
        let read = async {
            let add = AddTorrent::from_cli_argument(source)
                .context("that is not a magnet link, a URL, or a .torrent file")?;
            match self
                .session
                .add_torrent(add, Some(AddTorrentOptions { list_only: true, ..Default::default() }))
                .await
                .context("reading the torrent")?
            {
                AddTorrentResponse::ListOnly(listed) => Ok(listed),
                // Already in the list. Not an error worth a dialog — the user
                // gets told, and nothing is added twice.
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
                self.pending_bytes = Some(listed.torrent_bytes.to_vec());
                self.pending = Some(Pending {
                    source: Arc::from(source),
                    name: if name.is_empty() { Arc::from(source) } else { name },
                    files,
                    fetching: false,
                    error: None,
                });
            }
            Err(e) => {
                self.pending_bytes = None;
                self.pending = Some(Pending::failed(source, &format!("{e:#}")));
            }
        }
    }

    /// Accept what `inspect` found.
    pub async fn confirm_add(&mut self, only_files: Option<Vec<usize>>) -> anyhow::Result<()> {
        let bytes = self.pending_bytes.take().context("nothing was read to add")?;
        let source = self.pending.take().map_or_else(String::new, |p| p.source.to_string());

        // From the bytes `inspect` already has, so a magnet is not fetched from
        // the swarm a second time.
        let handle = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(bytes),
                Some(AddTorrentOptions {
                    only_files,
                    overwrite: true,
                    output_folder: Some(self.output_dir.to_string_lossy().into_owned()),
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

    /// Throw away what `inspect` found.
    pub fn cancel_add(&mut self) {
        self.pending = None;
        self.pending_bytes = None;
    }

    /// Add without asking. Used for what arrives on the command line, where
    /// there is no dialog to answer.
    pub async fn add(&mut self, source: &str) -> anyhow::Result<()> {
        let add = AddTorrent::from_cli_argument(source)
            .context("that is not a magnet link, a URL, or a .torrent file")?;

        let handle = self
            .session
            .add_torrent(
                add,
                Some(AddTorrentOptions {
                    // Not optional, whatever the default says. From librqbit's
                    // own docs: "Even when all the torrent pieces have been
                    // written, `overwrite` needs to be enabled in order to
                    // resume/seed the torrent." Without it every already-present
                    // torrent fails to add with "file exists".
                    overwrite: true,
                    // Passed per torrent, not taken from the session: this is
                    // what lets the download folder change without a restart.
                    output_folder: Some(self.output_dir.to_string_lossy().into_owned()),
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

    /// Fetch a file, or stop fetching it, on a torrent already in the list.
    ///
    /// The current selection is read from the handle rather than taken from the
    /// caller, so a click made against a second-old panel cannot undo anything
    /// that happened since. `file` of `None` is every file at once.
    ///
    /// The refusals are deliberate and both reach the status bar as a sentence:
    /// a change that would leave the torrent fetching nothing, and a torrent
    /// whose metadata has not arrived — there is nothing to choose from yet, and
    /// librqbit refuses it as well.
    pub async fn set_file_wanted(
        &self,
        id: TorrentId,
        file: Option<usize>,
        wanted: bool,
    ) -> anyhow::Result<()> {
        let handle = self.entries.get(&id).context("no such torrent")?.handle.clone();
        let count = handle.with_metadata(|meta| meta.file_infos.len()).unwrap_or(0);
        anyhow::ensure!(count > 0, "this torrent's file list has not arrived yet");

        let current = handle.only_files();
        let chosen = zerem_core::select_files(current.as_deref(), count, file, wanted)
            .context("at least one file has to be downloaded")?;

        // Already the answer. Saying so costs a comparison; not saying so costs
        // a chunk-tracker rebuild, and on a finished torrent a pause/unpause.
        if current.as_deref() == Some(chosen.as_slice()) {
            return Ok(());
        }

        let picked = chosen.len();
        self.session
            .update_only_files(&handle, &chosen.into_iter().collect())
            .await
            .context("changing which files are downloaded")?;
        tracing::info!(id = id.0, picked, of = count, "file selection changed");
        Ok(())
    }

    pub async fn set_running(&self, id: TorrentId, running: bool) -> anyhow::Result<()> {
        let handle = self.entries.get(&id).context("no such torrent")?.handle.clone();
        if running {
            self.session.unpause(&handle).await.context("starting")
        } else {
            self.session.pause(&handle).await.context("pausing")
        }
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
        for (id, entry) in &mut self.entries {
            let (name, name_key) = entry.resolve_name();
            let content = entry.resolve_content();
            // Bound first: `stats()` borrows the handle, and the rates it feeds
            // are a sibling field of the same entry.
            let stats = entry.handle.stats();
            let mut row = map::to_row(*id, &name, &name_key, &stats, &mut entry.rates);
            row.folder = entry.folder.clone();
            row.info_hash = entry.info_hash.clone();
            row.content = content;
            torrents.push(row);
        }

        // Only the watched one, and only while something is watching.
        let details = self
            .watching
            .and_then(|id| Some((id, self.entries.get(&id)?)))
            .map(|(id, entry)| map::to_details(id, &entry.handle));

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
            .with_pending(self.pending.clone());

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

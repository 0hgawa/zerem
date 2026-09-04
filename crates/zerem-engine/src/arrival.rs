//! What happens to a torrent the moment it finishes.
//!
//! Downloading to one folder and keeping in another: a fast disk takes the
//! writes, a large one keeps the result. Off unless somebody names a folder,
//! and this file is the sequence that runs when they have.
//!
//! # Why it is a sequence and not a call
//!
//! librqbit has no `move_storage`. libtorrent does — qBittorrent moves a
//! finished torrent by telling the library its files are somewhere else now,
//! and the library carries on seeding without reading a byte. Here the only
//! way to change where a torrent lives is to stop being that torrent and
//! become a new one pointing at the new place:
//!
//! 1. Remember everything needed to rebuild it: the `.torrent` bytes, which
//!    files were wanted, where they are and where they are going.
//! 2. Pause it, so nothing is writing while the files move.
//! 3. Move them. This is the slow part and the only one that can lose data, so
//!    it is the one that undoes itself — see [`crate::relocate`].
//! 4. Only once the move is *through*, drop the torrent from the session
//!    without touching the files, and add it again at the new folder.
//!
//! Order is the whole design. Every failure before step four leaves a torrent
//! that is still in the list, still knows where its files are, and has simply
//! not moved. Nothing is dropped until the data is already at the far end.
//!
//! # The cost, said plainly
//!
//! The torrent is re-checked. `overwrite` is what lets librqbit resume or seed
//! a torrent whose pieces are already written, and it verifies them to find
//! out. So a fifty-gigabyte torrent reads fifty gigabytes back after it moves,
//! and shows as Checking until it is done.
//!
//! That is a real price and there is no way around it from outside the
//! library. It is paid once, after the download has already finished, and
//! nobody is waiting on the content — which is why the feature is worth having
//! anyway, and why it is off until asked for.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use librqbit::{AddTorrent, AddTorrentOptions};
use zerem_core::TorrentId;

use crate::relocate;
use crate::session::TorrentSession;

/// Everything about a torrent that has to outlive it.
///
/// Gathered before anything is paused or moved, because after step four the
/// torrent this describes no longer exists.
struct Keepsake {
    bytes: Vec<u8>,
    only_files: Option<Vec<usize>>,
    plan: Vec<zerem_core::Step>,
    /// Where the rebuilt torrent is told to write.
    ///
    /// The destination root for a torrent with a folder of its own — librqbit
    /// adds the folder back itself — and the folder the files landed in for one
    /// that has none.
    output: String,
    was_running: bool,
}

impl TorrentSession {
    /// Move whatever finished on this tick, if there is anywhere to move it.
    pub async fn relocate_arrived(&mut self) {
        let Some(keep) = self.keep_dir.clone() else {
            // Nothing is configured, so nothing is owed. Cleared rather than
            // left to grow: a session that runs for a week with this switched
            // off would otherwise remember every torrent that ever finished.
            self.take_arrived();
            return;
        };
        for id in self.take_arrived() {
            if let Err(why) = self.relocate_one(id, &keep).await {
                tracing::warn!(id = id.0, "could not move a finished torrent: {why:#}");
                self.report(&zerem_core::text::move_failed(&why.to_string()));
            }
        }
    }

    async fn relocate_one(&mut self, id: TorrentId, keep: &Path) -> anyhow::Result<()> {
        let Some(keepsake) = self.gather(id, keep) else {
            // Nothing to do, and not a failure: the torrent may already be
            // where it belongs, or be gone, or have no files.
            return Ok(());
        };

        self.pause_for_move(id).await?;

        // Blocking, and minutes of it on a large torrent across volumes. The
        // engine's whole runtime would stop here otherwise, and with it every
        // other torrent in the session.
        let plan = keepsake.plan.clone();
        let carried = tokio::task::spawn_blocking(move || relocate::carry(&plan))
            .await
            .context("the move task was cancelled")?;

        if let Err(fault) = carried {
            // Nothing moved. Put it back the way it was and say so.
            if keepsake.was_running {
                let _ = self.set_running(id, true).await;
            }
            anyhow::bail!("{fault}");
        }

        self.rebuild(id, keepsake).await
    }

    /// Everything the rebuild will need, or `None` when there is nothing to do.
    fn gather(&self, id: TorrentId, keep: &Path) -> Option<Keepsake> {
        let entry = self.entries.get(&id)?;
        let from = PathBuf::from(entry.folder.as_ref());

        let (paths, bytes) = entry
            .handle
            .with_metadata(|meta| {
                let paths: Vec<String> = meta
                    .file_infos
                    .iter()
                    .map(|info| info.relative_filename.to_string_lossy().into_owned())
                    .collect();
                (paths, meta.torrent_bytes.to_vec())
            })
            .ok()?;

        // The same rule the download folder was built with, asked the same way,
        // so the two cannot drift: a torrent has a folder of its own exactly
        // when `subfolder` gave it one.
        let name = entry.handle.name().unwrap_or_default();
        let owns_folder = zerem_core::subfolder(&name, paths.len()).is_some();
        let plan = zerem_core::move_plan(&from, &paths, keep, owns_folder)?;

        // librqbit adds the torrent's own folder back when it is told a plain
        // output folder, so it is told the root — being told the folder as well
        // would nest it twice.
        let output = keep.to_string_lossy().into_owned();

        Some(Keepsake {
            bytes,
            only_files: (!entry.wanted.iter().all(|w| *w))
                .then(|| entry.wanted.iter().enumerate().filter(|(_, w)| **w).map(|(i, _)| i).collect()),
            plan,
            output,
            was_running: entry.wanted_running,
        })
    }

    /// Drop the torrent without touching its files, and add it back where the
    /// files now are.
    ///
    /// The two halves are one operation: between them the session does not hold
    /// this torrent at all, so anything that can fail has already failed.
    async fn rebuild(&mut self, id: TorrentId, keepsake: Keepsake) -> anyhow::Result<()> {
        self.forget(id).await?;

        let handle = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(keepsake.bytes),
                Some(AddTorrentOptions {
                    only_files: keepsake.only_files,
                    // Without it librqbit will not resume or seed a torrent
                    // whose pieces are already written — it refuses rather than
                    // verifies, which for a torrent that has just arrived is
                    // the wrong answer.
                    overwrite: true,
                    output_folder: Some(keepsake.output),
                    paused: !keepsake.was_running,
                    ..Default::default()
                }),
            )
            .await
            .context("adding the moved torrent back")?
            .into_handle()
            .context("the moved torrent was listed rather than added")?;

        self.adopt_moved(id, handle);
        Ok(())
    }
}

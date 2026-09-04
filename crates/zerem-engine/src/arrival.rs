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
//! # Step three does not happen on the engine's thread
//!
//! It cannot. A cross-volume copy of a fifty-gigabyte torrent is minutes of
//! disk, and the engine's loop is what publishes the snapshot the window draws
//! and what reads the commands the window sends. Awaiting the copy inside that
//! loop freezes the entire application for the length of the move: no speeds,
//! no buttons, nothing. The first version of this did exactly that.
//!
//! So the copy is spawned and *looked in on* by later ticks. The loop keeps
//! turning, every other torrent keeps running, and the rebuild happens on
//! whichever tick finds the task finished.
//!
//! One at a time, and not only because it is simpler: two large copies at once
//! turn a sequential read into a seeking one, and on a spinning disk that is
//! most of the throughput gone.
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
use tokio::task::JoinHandle;
use zerem_core::TorrentId;

use crate::relocate;
use crate::session::TorrentSession;

/// Everything about a torrent that has to outlive it.
///
/// Gathered before anything is paused or moved, because after step four the
/// torrent this describes no longer exists.
pub struct Keepsake {
    bytes: Vec<u8>,
    only_files: Option<Vec<usize>>,
    plan: Vec<zerem_core::Step>,
    /// Where the rebuilt torrent is told to write.
    ///
    /// The destination root, not the torrent's own folder inside it: librqbit
    /// puts that folder back itself, and naming it here would nest it twice.
    output: String,
    was_running: bool,
}

/// A move in flight: what is moving, and the thread moving it.
pub struct Move {
    id: TorrentId,
    keepsake: Keepsake,
    task: JoinHandle<Result<(), relocate::Fault>>,
}

impl TorrentSession {
    /// Look in on the move that is running, and start one if none is.
    ///
    /// Called every tick and free on almost all of them: nothing has finished,
    /// nothing is in flight, and this returns after two comparisons.
    pub async fn relocate_arrived(&mut self) {
        self.finish_move().await;
        if self.keep_dir.is_none() {
            // Nothing is configured, so nothing is owed. Cleared rather than
            // left to grow: a session running for a week with this switched off
            // would otherwise remember every torrent that ever finished.
            self.forget_arrived();
            return;
        }
        if self.moving.is_none() {
            self.start_move().await;
        }
    }

    /// Take the next torrent that finished and set its files moving.
    async fn start_move(&mut self) {
        let Some(keep) = self.keep_dir.clone() else { return };
        while let Some(id) = self.next_arrived() {
            let Some(keepsake) = self.gather(id, &keep) else {
                // Nothing to do, and not a failure: it may already be where it
                // belongs, or be gone, or have no files. Try the next one.
                continue;
            };
            if let Err(why) = self.pause_for_move(id).await {
                tracing::warn!(id = id.0, "could not pause before moving: {why:#}");
                continue;
            }
            let plan = keepsake.plan.clone();
            let task = tokio::task::spawn_blocking(move || relocate::carry(&plan));
            self.moving = Some(Move { id, keepsake, task });
            return;
        }
    }

    /// Finish the move in flight, if there is one and it is done.
    async fn finish_move(&mut self) {
        let Some(moving) = self.moving.take() else { return };
        if !moving.task.is_finished() {
            // Still copying. Put it back and let the loop carry on — this is
            // the whole reason the move is not awaited where it is started.
            self.moving = Some(moving);
            return;
        }

        let Move { id, keepsake, task } = moving;
        let carried = match task.await {
            Ok(carried) => carried,
            Err(why) => {
                tracing::warn!(id = id.0, "the move task did not finish: {why}");
                self.after_failure(id, &keepsake);
                return;
            }
        };

        if let Err(fault) = carried {
            tracing::warn!(id = id.0, "could not move a finished torrent: {fault}");
            self.report(&zerem_core::text::move_failed(&fault.to_string()));
            self.after_failure(id, &keepsake);
            return;
        }

        if let Err(why) = self.rebuild(id, keepsake).await {
            // The files are at the destination and the torrent could not be
            // added back to point at them. Loud, because this is the one state
            // the app cannot put right by itself.
            tracing::error!(id = id.0, "the files moved but the torrent did not: {why:#}");
            self.report(&zerem_core::text::move_failed(&why.to_string()));
        }
    }

    /// A move that did not happen. Nothing was touched, so only the pause has
    /// to be undone — and by wanting it running rather than starting it, so the
    /// queue decides when, as it does for everything else.
    fn after_failure(&mut self, id: TorrentId, keepsake: &Keepsake) {
        if keepsake.was_running {
            if let Some(entry) = self.entries.get_mut(&id) {
                entry.wanted_running = true;
            }
        }
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

        Some(Keepsake {
            bytes,
            only_files: (!entry.wanted.iter().all(|w| *w))
                .then(|| entry.wanted.iter().enumerate().filter(|(_, w)| **w).map(|(i, _)| i).collect()),
            plan,
            output: keep.to_string_lossy().into_owned(),
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

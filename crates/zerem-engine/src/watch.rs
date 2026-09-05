//! The folder the app keeps an eye on.
//!
//! Drop a `.torrent` into it — from a browser, a script, a network share — and
//! it is added without anybody opening the window. The oldest automation a
//! torrent client has, and the one that makes it a service rather than an
//! application.
//!
//! Which names are worth picking up and what to call them afterwards is
//! [`zerem_core::watch`], where it can be tested without a disk. What is here
//! is the reading, the adding and the renaming.
//!
//! # Every few seconds, not every tick
//!
//! The tick is a second, and a directory listing a second for a folder that is
//! empty almost always is a syscall spent on nothing. Four seconds is far
//! inside the time it takes anybody to notice a download has not started.

use std::path::{Path, PathBuf};

use zerem_core::Watched;

use crate::session::TorrentSession;

/// Ticks between sweeps.
const EVERY: u8 = 4;

impl TorrentSession {
    /// Add whatever has appeared in the watched folder, if there is one.
    pub async fn sweep_watch(&mut self) {
        self.since_sweep = self.since_sweep.wrapping_add(1);
        if self.since_sweep < EVERY {
            return;
        }
        self.since_sweep = 0;

        let Some(folder) = self.watch_dir.clone() else { return };
        for name in listing(&folder) {
            let path = folder.join(&name);
            // The path is the source: `add` reads a `.torrent` file, works out
            // the download folder and adds it — the same route the window
            // takes, minus the dialog. There is nothing to ask about a file
            // somebody dropped in a folder for exactly this.
            let outcome = match self.add(&path.to_string_lossy()).await {
                Ok(()) => Watched::Added,
                Err(why) => {
                    tracing::warn!(file = %name, "watched folder: {why:#}");
                    self.notify(zerem_core::text::watch_failed(&name));
                    Watched::Failed
                }
            };
            park(&folder, &name, outcome);
        }
    }
}

/// The torrent files waiting in a folder.
///
/// An unreadable folder is not an error worth saying: it is a path somebody
/// typed that no longer exists, and the setting is what tells them so.
fn listing(folder: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    zerem_core::waiting(&names).into_iter().map(str::to_owned).collect()
}

/// Rename a file to say what happened to it.
///
/// Renamed and never deleted: removing somebody's file is not this app's
/// decision. A name that already exists is left alone — the same torrent
/// dropped twice keeps the first record rather than clobbering it.
fn park(folder: &Path, name: &str, outcome: Watched) {
    let to = folder.join(zerem_core::parked(name, outcome));
    if to.exists() {
        // Nothing to write and nothing to say: the second copy is simply
        // removed from the way, under a name that is free.
        let _ = std::fs::remove_file(folder.join(name));
        return;
    }
    if let Err(e) = std::fs::rename(folder.join(name), &to) {
        // The one failure that matters, because it is the one that repeats: a
        // file that cannot be renamed is picked up again on the next sweep,
        // added again, and refused as already present, for ever.
        tracing::error!(file = name, error = %e, "could not park a watched torrent");
    }
}

/// Where the watcher looks, or nothing.
#[must_use]
pub fn folder_of(setting: &str) -> Option<PathBuf> {
    (!setting.trim().is_empty()).then(|| PathBuf::from(setting.trim()))
}

#[cfg(test)]
mod tests {
    use super::{folder_of, listing, park};
    use std::path::PathBuf;
    use zerem_core::Watched;

    /// A scratch folder that removes itself.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("zerem-watch-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("make a scratch folder");
            Self(path)
        }

        fn file(&self, name: &str) {
            std::fs::write(self.0.join(name), b"x").expect("write a file");
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_the_torrents_are_listed() {
        let scratch = Scratch::new("listing");
        scratch.file("a.torrent");
        scratch.file("notes.txt");
        scratch.file("b.torrent.added");

        assert_eq!(listing(&scratch.0), vec!["a.torrent"]);
    }

    #[test]
    fn a_folder_that_is_not_there_is_no_work_and_no_noise() {
        // A path somebody typed and then deleted. The setting is what tells
        // them; a sweep that shouted about it every four seconds would not.
        assert!(listing(&PathBuf::from("Z:/nowhere/at/all")).is_empty());
    }

    #[test]
    fn parking_renames_rather_than_deletes() {
        let scratch = Scratch::new("park");
        scratch.file("a.torrent");

        park(&scratch.0, "a.torrent", Watched::Added);
        assert!(!scratch.0.join("a.torrent").exists());
        assert!(scratch.0.join("a.torrent.added").exists());
    }

    #[test]
    fn a_failure_is_parked_too_so_it_is_not_retried_for_ever() {
        // The reason the outcome goes in the name at all: a broken file left
        // where it was is picked up on every sweep, for as long as the app runs.
        let scratch = Scratch::new("failed");
        scratch.file("bad.torrent");

        park(&scratch.0, "bad.torrent", Watched::Failed);
        assert!(scratch.0.join("bad.torrent.failed").exists());
        assert!(listing(&scratch.0).is_empty(), "it would be picked up again");
    }

    #[test]
    fn the_same_torrent_dropped_twice_keeps_the_first_record() {
        let scratch = Scratch::new("twice");
        scratch.file("a.torrent.added");
        scratch.file("a.torrent");

        park(&scratch.0, "a.torrent", Watched::Added);
        assert!(!scratch.0.join("a.torrent").exists(), "the second copy is out of the way");
        assert_eq!(std::fs::read(scratch.0.join("a.torrent.added")).expect("read"), b"x");
    }

    #[test]
    fn an_empty_setting_is_switched_off() {
        assert!(folder_of("").is_none());
        assert!(folder_of("   ").is_none());
        assert_eq!(folder_of(" D:/drop "), Some(PathBuf::from("D:/drop")), "and it is trimmed");
    }
}

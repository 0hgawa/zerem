//! Carrying a finished download to where it is kept.
//!
//! The plan is a list of pairs — where each file is and where it goes — and
//! nothing here knows what a torrent is. The rule the whole module is built
//! around: **if it cannot finish, it leaves everything exactly as it found
//! it**. A half-moved torrent is worse than one
//! that never moved, because the files are then in two places and neither is
//! the one the client is pointing at.
//!
//! # Two very different operations wearing one name
//!
//! On the same volume a move is a rename: instant, atomic per file, and it does
//! not read a byte. Across volumes it is a copy followed by a delete, it reads
//! and writes every byte, and it can fail halfway with the disk full.
//!
//! The usual case is the second one. Downloading to a fast disk and keeping on
//! a large one is the entire reason somebody turns this on, and those are not
//! the same volume. So the slow path is the one that gets the care: every file
//! is copied first, every copy is checked for length, and only when all of them
//! are through does anything get deleted. A failure at any point deletes what
//! was copied and returns, and the originals have not been touched.
//!
//! Verifying by length and not by hash is deliberate. Whatever moved these is
//! going to verify them properly afterwards — nothing that keeps checksums
//! trusts a copy on faith — so hashing here would be hashing everything twice
//! to answer a question that gets asked again anyway.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What went wrong, in the words the app will show.
#[derive(Debug)]
pub enum Fault {
    /// A folder at the destination could not be made.
    Folder(PathBuf, io::Error),
    /// One file would not copy. The plan is already undone by the time this is
    /// returned.
    Copy(PathBuf, io::Error),
    /// A copy finished but came out the wrong length, which is a disk that
    /// filled up without saying so.
    Short(PathBuf),
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Folder(path, why) => write!(f, "could not make {}: {why}", path.display()),
            Self::Copy(path, why) => write!(f, "could not copy {}: {why}", path.display()),
            Self::Short(path) => write!(f, "{} came out short — the disk may be full", path.display()),
        }
    }
}

/// Carry every file of a plan to its destination.
///
/// Blocking, and meant to be: it is minutes of disk on a large torrent. The
/// caller runs it off whatever thread must stay responsive.
///
/// On success the files are at their destinations and the originals are gone.
/// On failure nothing has changed that the caller has to clean up.
///
/// # Errors
///
/// Every [`Fault`] leaves the source files where they were.
pub fn carry(plan: &[(PathBuf, PathBuf)]) -> Result<(), Fault> {
    for folder in folders(plan) {
        fs::create_dir_all(&folder).map_err(|why| Fault::Folder(folder.clone(), why))?;
    }

    // A rename that works is the whole job — no byte is read and each file
    // arrives atomically. It is tried per file rather than once for the folder
    // because a plan can straddle volumes: a torrent whose files were put where
    // they are by hand.
    //
    // Which is why the ones that worked are remembered. Renaming three of four
    // files and then falling back to copying leaves the copy pass looking at a
    // folder it was not handed, and the first version of this did exactly that
    // — the test for it is the one that caught it. Undoing a rename is another
    // rename in the other direction, on a volume that has just proved it works.
    let mut renamed: Vec<&(PathBuf, PathBuf)> = Vec::with_capacity(plan.len());
    for step in plan {
        if fs::rename(&step.0, &step.1).is_err() {
            for (from, to) in renamed {
                let _ = fs::rename(to, from);
            }
            return copy_and_delete(plan);
        }
        renamed.push(step);
    }
    prune(plan);
    Ok(())
}

/// The cross-volume path: every byte copied and checked before anything goes.
fn copy_and_delete(plan: &[(PathBuf, PathBuf)]) -> Result<(), Fault> {
    copy_all(plan)?;
    for (from, _) in plan {
        // The copies are all through and checked. A source that will not delete
        // is a file somebody has open, and leaving it is better than failing a
        // move that has already succeeded — the data is at the destination.
        let _ = fs::remove_file(from);
    }
    prune(plan);
    Ok(())
}

/// Copy every file, or copy none.
fn copy_all(plan: &[(PathBuf, PathBuf)]) -> Result<(), Fault> {
    let mut done: Vec<&PathBuf> = Vec::with_capacity(plan.len());
    for step in plan {
        // A rename may already have moved this one, if the plan straddles
        // volumes and the earlier attempt got partway. Skip what is there.
        if !step.0.exists() && step.1.exists() {
            continue;
        }
        match copy_one(step) {
            Ok(()) => done.push(&step.1),
            Err(fault) => {
                for made in done {
                    let _ = fs::remove_file(made);
                }
                return Err(fault);
            }
        }
    }
    Ok(())
}

fn copy_one((from, to): &(PathBuf, PathBuf)) -> Result<(), Fault> {
    let written = fs::copy(from, to).map_err(|why| Fault::Copy(from.clone(), why))?;
    let expected = fs::metadata(from).map(|m| m.len()).map_err(|why| Fault::Copy(from.clone(), why))?;
    if written == expected {
        return Ok(());
    }
    // Windows will report a successful copy that ran out of room. The length is
    // the cheap way to catch it, and the file has to go: a short file at the
    // destination would be re-checked, found wrong, and re-downloaded.
    let _ = fs::remove_file(to);
    Err(Fault::Short(to.clone()))
}

/// Every folder that has to exist before a plan can run, parents first.
///
/// The order matters and getting it wrong fails halfway: a plan writes
/// `a/b/c.mkv` before anything has made `a/b`, and the failure reads as a
/// permissions problem.
fn folders(plan: &[(PathBuf, PathBuf)]) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> =
        plan.iter().filter_map(|(_, to)| to.parent().map(Path::to_path_buf)).collect();
    all.sort();
    all.dedup();
    all
}

/// Remove the folders a move emptied, deepest first.
///
/// Only ones that came out empty, and failure is ignored throughout: a folder
/// somebody else put something in is not this function's business, and the move
/// has already succeeded by the time it runs.
fn prune(plan: &[(PathBuf, PathBuf)]) {
    let mut folders: Vec<&Path> = plan.iter().filter_map(|(from, _)| from.parent()).collect();
    folders.sort_unstable();
    folders.dedup();
    // Deepest first, so `a/b` is gone before `a` is tried.
    folders.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for folder in folders {
        let _ = fs::remove_dir(folder);
    }
}

#[cfg(test)]
mod tests {
    use super::carry;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// A scratch folder that removes itself, so a failing test does not leave
    /// gigabytes of nothing behind on somebody's disk.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("zerem-relocate-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("make a scratch folder");
            Self(path)
        }

        fn file(&self, at: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(at);
            fs::create_dir_all(path.parent().expect("a parent")).expect("make a folder");
            fs::write(&path, bytes).expect("write a file");
            path
        }

        fn at(&self, path: &str) -> PathBuf {
            self.0.join(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn step(from: PathBuf, to: PathBuf) -> (PathBuf, PathBuf) {
        (from, to)
    }

    #[test]
    fn files_arrive_with_their_shape_and_their_contents() {
        let scratch = Scratch::new("plain");
        scratch.file("dl/Show/S1/E01.mkv", b"first");
        scratch.file("dl/Show/S1/E02.mkv", b"second");

        let plan = vec![
            step(scratch.at("dl/Show/S1/E01.mkv"), scratch.at("keep/Show/S1/E01.mkv")),
            step(scratch.at("dl/Show/S1/E02.mkv"), scratch.at("keep/Show/S1/E02.mkv")),
        ];
        carry(&plan).expect("a plain move");

        assert_eq!(fs::read(scratch.at("keep/Show/S1/E01.mkv")).expect("read"), b"first");
        assert_eq!(fs::read(scratch.at("keep/Show/S1/E02.mkv")).expect("read"), b"second");
    }

    #[test]
    fn the_originals_are_gone_and_so_are_the_folders_they_emptied() {
        // A move that leaves the tree behind leaves the download folder filling
        // up with empty season folders.
        let scratch = Scratch::new("prune");
        scratch.file("dl/Show/S1/E01.mkv", b"x");

        let plan = vec![step(scratch.at("dl/Show/S1/E01.mkv"), scratch.at("keep/Show/S1/E01.mkv"))];
        carry(&plan).expect("a plain move");

        assert!(!scratch.at("dl/Show/S1/E01.mkv").exists(), "the original is still there");
        assert!(!scratch.at("dl/Show/S1").exists(), "the folder it emptied is still there");
    }

    #[test]
    fn a_folder_somebody_else_is_using_is_left_alone() {
        // Pruning is a courtesy, not a claim on the folder.
        let scratch = Scratch::new("shared");
        scratch.file("dl/Show/E01.mkv", b"x");
        scratch.file("dl/Show/notes.txt", b"mine");

        let plan = vec![step(scratch.at("dl/Show/E01.mkv"), scratch.at("keep/E01.mkv"))];
        carry(&plan).expect("a plain move");

        assert!(scratch.at("dl/Show/notes.txt").exists(), "a file that was not in the plan was removed");
    }

    #[test]
    fn a_plan_that_cannot_finish_leaves_every_original_where_it_was() {
        // The rule the whole module exists for. The second file does not exist,
        // so the copy of it fails, and the first file must not have been moved
        // or half-copied.
        let scratch = Scratch::new("undo");
        scratch.file("dl/a.mkv", b"first");

        let plan = vec![
            step(scratch.at("dl/a.mkv"), scratch.at("keep/a.mkv")),
            step(scratch.at("dl/missing.mkv"), scratch.at("keep/missing.mkv")),
        ];
        let fault = carry(&plan).expect_err("the second file is not there");

        assert!(scratch.at("dl/a.mkv").exists(), "the original was moved anyway: {fault}");
        assert!(!scratch.at("keep/a.mkv").exists(), "a copy was left behind: {fault}");
    }

    #[test]
    fn an_empty_plan_is_not_an_error() {
        // Nothing to do is a real answer. The caller has already decided
        // whether a move was worth starting.
        carry(&[]).expect("nothing to do");
    }

    #[test]
    fn a_fault_says_which_file_and_why() {
        // It is shown to somebody who has to act on it, so a path that is not
        // in the message is a message nobody can use.
        let scratch = Scratch::new("message");
        let plan = vec![step(scratch.at("dl/gone.mkv"), scratch.at("keep/gone.mkv"))];
        let text = carry(&plan).expect_err("nothing to copy").to_string();
        assert!(text.contains("gone.mkv"), "no path in {text:?}");
    }

    #[test]
    fn a_file_already_at_the_destination_does_not_stop_the_rest() {
        // Which is the state a plan straddling two volumes lands in when the
        // rename pass got partway before hitting the boundary.
        let scratch = Scratch::new("partial");
        scratch.file("keep/a.mkv", b"already");
        scratch.file("dl/b.mkv", b"second");

        let plan = vec![
            step(scratch.at("dl/a.mkv"), scratch.at("keep/a.mkv")),
            step(scratch.at("dl/b.mkv"), scratch.at("keep/b.mkv")),
        ];
        carry(&plan).expect("the one already there is skipped");
        assert_eq!(fs::read(scratch.at("keep/b.mkv")).expect("read"), b"second");
    }

    #[test]
    fn nothing_is_written_outside_the_plan() {
        // A guard on the plan rather than on the move: every destination came
        // from `zerem_core::move_plan`, and this is what says the executor does
        // not invent one.
        let scratch = Scratch::new("bounds");
        scratch.file("dl/a.mkv", b"x");
        let plan = vec![step(scratch.at("dl/a.mkv"), scratch.at("keep/a.mkv"))];
        carry(&plan).expect("a plain move");

        let listed: Vec<PathBuf> = fs::read_dir(scratch.at("keep"))
            .expect("read the destination")
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .collect();
        assert_eq!(listed, vec![scratch.at("keep/a.mkv")]);
        assert!(Path::new(&scratch.at("keep")).is_dir());
    }
}

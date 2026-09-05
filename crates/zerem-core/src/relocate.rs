//! Where a finished torrent's files should end up, and whether moving them is
//! a sensible thing to attempt at all.
//!
//! Downloading to one folder and keeping in another is the reason this exists:
//! a fast disk takes the writes, a large one keeps the result. Which files go
//! where is arithmetic on paths and belongs here, where it can be tested
//! without a disk. Doing the moving is somebody else's job.
//!
//! # What this refuses, and why refusing is the feature
//!
//! Every answer below is `None` for a move that should not be started, and each
//! of those is a way somebody loses files:
//!
//! - Moving a folder into itself or into its own child, which is a copy walking
//!   into what it is writing.
//! - Moving to where the files already are. Not an error, but not work either,
//!   and writing a file over itself can truncate it.
//! - A torrent with no files, which is nothing to move and a sign that
//!   something else has already gone wrong.

use std::path::{Path, PathBuf};

/// One file's journey: where it is now and where it is going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// Every file of a torrent, and where each lands under `destination`.
///
/// `relative` is what the torrent metadata calls each file — the path under
/// `output`, separators and all. The shape under `destination` is the same
/// shape, because a move that reorganises a season pack is not a move.
///
/// `owns_folder` says whether `output` belongs to this torrent alone. A
/// multi-file torrent gets a folder of its own and that folder travels with it;
/// a single-file one sits loose in the shared download folder, and carrying
/// *that* folder's name along would file every lone download under "Downloads"
/// at the far end. It cannot be told from the path — both are just folders — so
/// it is asked for. The test for lone files is what found this.
///
/// `None` when the move should not be attempted; see the note at the top of the
/// file for the cases and what each of them costs.
#[must_use]
pub fn plan(output: &Path, relative: &[String], destination: &Path, owns_folder: bool) -> Option<Vec<Step>> {
    if relative.is_empty() {
        return None;
    }
    let (from_root, to_root) = (normalise(output), normalise(destination));
    if from_root == to_root {
        return None;
    }
    // Only recursive when the whole folder travels. A lone file going from
    // `D:/downloads` to `D:/downloads/done` is a different path for that one
    // file and perfectly safe.
    if owns_folder && to_root.starts_with(&from_root) {
        return None;
    }

    let carried = landing(&from_root, &to_root, owns_folder);
    Some(relative.iter().map(|path| Step { from: from_root.join(path), to: carried.join(path) }).collect())
}

/// The folder the files end up *in*, which is not always the destination.
///
/// A torrent with a folder of its own takes that folder along, so its files
/// land one level below where they were sent; one without simply lands there.
///
/// Its own function because two things need this answer and must not disagree:
/// the plan, to know where each file goes, and whatever re-registers the
/// torrent afterwards, to know where to look for them. Getting the second wrong
/// is a torrent that has moved perfectly and then downloads itself again — and
/// that is not hypothetical, it is what the first version of the move did.
#[must_use]
pub fn landing(output: &Path, destination: &Path, owns_folder: bool) -> PathBuf {
    let (from_root, to_root) = (normalise(output), normalise(destination));
    match from_root.file_name().filter(|_| owns_folder) {
        Some(name) => to_root.join(name),
        None => to_root,
    }
}

/// Reduce a path to the one spelling this file compares against.
///
/// Two spellings of one folder have to come out identical, or the check for
/// "moving somewhere it already is" waves through a move that copies every file
/// onto itself — and a file copied onto itself is a file truncated to nothing.
///
/// `components` does most of it: a trailing separator, a `.`, and forward
/// slashes on Windows all disappear. Two things it does *not* do, and both were
/// measured rather than assumed:
///
/// - `\?\D:\dl` and `D:\dl` are the same folder and compare unequal. Windows
///   hands out the verbatim form from `canonicalize`, and librqbit is free to
///   start doing that in any release.
/// - `d:\dl` and `D:\dl` are the same folder on a case-insensitive filesystem
///   and compare unequal too.
///
/// Neither is reachable through today's code paths, and that is not a reason to
/// leave them: what is on the other side of this check is somebody's finished
/// download.
fn normalise(path: &Path) -> PathBuf {
    use std::path::{Component, Prefix};

    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    out.push(format!("{}:", letter.to_ascii_uppercase() as char));
                }
                Prefix::VerbatimUNC(server, share) => {
                    let mut unc = std::ffi::OsString::from(r"\\");
                    unc.push(server);
                    unc.push(std::path::MAIN_SEPARATOR_STR);
                    unc.push(share);
                    out.push(unc);
                }
                _ => out.push(prefix.as_os_str()),
            },
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{landing, plan, Step};
    use std::path::{Path, PathBuf};

    fn at(text: &str) -> PathBuf {
        PathBuf::from(text)
    }

    #[test]
    fn every_file_keeps_its_shape_under_the_new_root() {
        // A move that reorganises a season pack is not a move.
        let steps = plan(
            Path::new("D:/downloads/Show S01"),
            &["Season 1/E01.mkv".into(), "Season 1/E02.mkv".into(), "readme.txt".into()],
            Path::new("E:/library"),
            true,
        )
        .expect("a plain move");

        assert_eq!(steps.len(), 3);
        assert_eq!(
            steps[0],
            Step {
                from: at("D:/downloads/Show S01/Season 1/E01.mkv"),
                to: at("E:/library/Show S01/Season 1/E01.mkv"),
            }
        );
        assert_eq!(steps[2].to, at("E:/library/Show S01/readme.txt"));
    }

    #[test]
    fn the_torrents_own_folder_comes_along() {
        // Without it every torrent moved to one destination pours its files
        // into a single heap, and two seasons that both hold `E01.mkv` fight
        // over the name.
        let steps = plan(Path::new("D:/dl/Album"), &["01.flac".into()], Path::new("E:/music"), true)
            .expect("a plain move");
        assert_eq!(steps[0].to, at("E:/music/Album/01.flac"));
    }

    #[test]
    fn a_lone_file_does_not_drag_the_shared_download_folder_along() {
        // A single-file torrent has no folder of its own, so its output folder
        // is the one every download shares. Carrying that name would file every
        // lone download under "Downloads" at the destination.
        let steps = plan(Path::new("D:/downloads"), &["film.mkv".into()], Path::new("E:/films"), false)
            .expect("a plain move");
        assert_eq!(steps[0].to, at("E:/films/film.mkv"));
    }

    #[test]
    fn a_folder_is_never_moved_into_itself() {
        // Copying `A` into `A/sub` walks into what it is writing.
        assert!(plan(Path::new("D:/dl"), &["a.mkv".into()], Path::new("D:/dl/done"), true).is_none());
        assert!(plan(Path::new("D:/dl"), &["a.mkv".into()], Path::new("D:/dl"), true).is_none());
    }

    #[test]
    fn a_lone_file_may_move_into_a_subfolder_of_where_it_sits() {
        // It is a different path for that one file and nothing recurses. Only a
        // whole folder travelling into its own child is the dangerous case.
        let steps =
            plan(Path::new("D:/downloads"), &["film.mkv".into()], Path::new("D:/downloads/done"), false)
                .expect("a lone file may");
        assert_eq!(steps[0].to, at("D:/downloads/done/film.mkv"));
    }

    #[test]
    fn a_trailing_separator_does_not_hide_that_it_is_the_same_place() {
        assert!(plan(Path::new("D:/dl/"), &["a.mkv".into()], Path::new("D:/dl"), true).is_none());
        assert!(plan(Path::new("D:/dl"), &["a.mkv".into()], Path::new("D:/./dl"), true).is_none());
    }

    #[test]
    #[cfg(windows)]
    fn two_different_drives_are_never_the_same_place() {
        // The case the whole feature exists for — a fast disk and a large one —
        // and the one where a wrong answer would be a folder moved into itself
        // across volumes.
        let steps = plan(Path::new(r"D:\downloads\Show"), &["a.mkv".into()], Path::new(r"E:\library"), true)
            .expect("different drives");
        assert_eq!(steps[0].to, at(r"E:\library\Show\a.mkv"));
    }

    #[test]
    #[cfg(windows)]
    fn a_backslash_path_reads_the_same_as_the_forward_slash_one() {
        // Windows gives back backslashes and the settings file may hold either,
        // since the folder picker and a hand edit do not agree. `components`
        // is what makes the two compare equal.
        assert!(plan(Path::new(r"D:\dl"), &["a.mkv".into()], Path::new("D:/dl"), true).is_none());
    }

    #[test]
    #[cfg(windows)]
    fn a_verbatim_prefix_does_not_look_like_a_different_folder() {
        // The dangerous spelling, and the one that was allowed before: two
        // *different* forms of the same folder. Windows hands the verbatim one
        // out of `canonicalize`, and a move onto itself copies every file over
        // itself — which truncates it.
        let verbatim = concat!(r"\\", r"?\", r"D:\dl");
        assert!(
            plan(Path::new(verbatim), &["a.mkv".into()], Path::new(r"D:\dl"), true).is_none(),
            "a folder was allowed to move onto itself"
        );
        assert!(
            plan(Path::new(r"D:\dl"), &["a.mkv".into()], Path::new(verbatim), true).is_none(),
            "and the same the other way round"
        );
    }

    #[test]
    #[cfg(windows)]
    fn the_case_of_a_drive_letter_is_not_a_different_drive() {
        assert!(plan(Path::new(r"d:\dl"), &["a.mkv".into()], Path::new(r"D:\dl"), true).is_none());
    }
    #[test]
    fn the_landing_folder_is_where_the_files_actually_end_up() {
        // The question the re-registration asks, and it has to give the same
        // answer as the plan. A torrent that moved perfectly and is then told
        // to look one level too high downloads itself again.
        let steps =
            plan(Path::new("D:/dl/Show"), &["a.mkv".into()], Path::new("E:/lib"), true).expect("a move");
        let landed = landing(Path::new("D:/dl/Show"), Path::new("E:/lib"), true);
        assert_eq!(landed, at("E:/lib/Show"));
        assert_eq!(steps[0].to, landed.join("a.mkv"));
    }

    #[test]
    fn a_lone_file_lands_in_the_destination_itself() {
        assert_eq!(landing(Path::new("D:/downloads"), Path::new("E:/films"), false), at("E:/films"));
    }

    #[test]
    fn a_torrent_with_no_files_is_not_a_move() {
        assert!(plan(Path::new("D:/dl/Thing"), &[], Path::new("E:/keep"), true).is_none());
    }
}

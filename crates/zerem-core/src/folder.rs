//! Where a torrent's files land under the download folder.
//!
//! librqbit does this itself — until you tell it where to write. Passing an
//! explicit `output_folder` takes the branch that skips the per-torrent
//! subfolder entirely, and Zerem has to pass one: the download folder is
//! changeable while the app runs, and librqbit fixes its own at construction
//! with no setter. So the rule is reimplemented here rather than inherited.
//!
//! It was found by reading a log line: a twelve-episode pack was writing
//! `Downloads\Zerem\[Judas] Chainsaw Man - S01E10.mkv`, straight into the
//! download folder with no folder of its own. Two packs would have interleaved
//! their files, and "remove, and the data with it" would have been deleting out
//! of a folder shared with everything else.

use std::path::{Component, Path};

/// The folder this torrent's files belong in, relative to the download folder.
///
/// `None` means "straight in", which is the right answer twice: for a
/// single-file torrent, where a folder holding one file is a folder nobody
/// wanted, and for a name that cannot safely be one.
///
/// The count is the rule, not the shape of the name: BitTorrent's own
/// distinction is that a multi-file torrent's `name` *is* a directory, while a
/// single-file torrent's `name` is the file. Same rule librqbit applies, so a
/// torrent added before this existed lands in the same place it already did.
#[must_use]
pub fn subfolder(name: &str, files: usize) -> Option<&str> {
    if files < 2 || name.is_empty() {
        return None;
    }
    // Exactly one ordinary component. A torrent's name comes from a stranger,
    // and `..\..\Windows` or `C:\` in it is not a folder name, it is an attempt
    // — refused by dropping the subfolder rather than by refusing the torrent,
    // because the files are then written somewhere harmless.
    let path = Path::new(name);
    let mut parts = path.components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(_)), None) => Some(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::subfolder;

    #[test]
    fn a_pack_gets_a_folder_of_its_own() {
        // The bug this exists for: without it, twelve episodes land loose in
        // the download folder beside everything else already there.
        assert_eq!(subfolder("[Judas] Chainsaw Man (Season 1)", 12), Some("[Judas] Chainsaw Man (Season 1)"));
    }

    #[test]
    fn a_single_file_goes_straight_in() {
        // A folder holding one file is a folder nobody asked for, and it is
        // what every other client does too.
        assert_eq!(subfolder("debian-13.6.0-amd64.iso", 1), None);
        assert_eq!(subfolder("anything", 0), None, "metadata not in yet");
    }

    #[test]
    fn a_name_that_climbs_out_is_refused() {
        // The name comes from a stranger. Refused by dropping the folder rather
        // than by refusing the torrent: the files then land somewhere harmless
        // instead of somewhere chosen by whoever made the torrent.
        assert_eq!(subfolder("../../Windows/System32", 5), None);
        assert_eq!(subfolder("..", 5), None);
        assert_eq!(subfolder("sub/dir", 5), None);
        assert_eq!(subfolder(r"sub\dir", 5), None);
    }

    #[test]
    fn an_absolute_name_is_refused_too() {
        assert_eq!(subfolder(r"C:\Windows", 5), None);
        assert_eq!(subfolder("/etc", 5), None);
    }

    #[test]
    fn a_name_with_no_name_in_it_is_refused() {
        assert_eq!(subfolder("", 5), None);
        assert_eq!(subfolder(".", 5), None, "the current directory is not a subfolder");
    }

    #[test]
    fn the_ordinary_awkward_characters_are_still_a_folder() {
        // Torrent names are full of brackets, dots and spaces. None of those
        // are path traversal, and refusing them would put most of a library
        // loose in the download folder.
        assert_eq!(subfolder("Some.Show.S01.1080p.WEB-DL", 10), Some("Some.Show.S01.1080p.WEB-DL"));
        assert_eq!(subfolder("Album — Artist (2019) [FLAC]", 14), Some("Album — Artist (2019) [FLAC]"));
    }
}

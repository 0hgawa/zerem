//! Splitting a torrent's file path into the part somebody reads and the part
//! that only says where it lives.
//!
//! A season pack lists `Featurettes\Trailer.mkv`, and the whole of what the eye
//! is looking for is `Trailer.mkv`. The folder still matters — two files called
//! `01.mkv` in different folders are different files — so it is kept and shown
//! quietly rather than dropped.
//!
//! Here rather than in the `.slint` for the rule this app has everywhere: the
//! window never formats anything. A split done per row per frame in the UI is a
//! split done sixty times a second for an answer that changed once.

/// Where a file lives, and what it is called.
///
/// The folder comes back with a chevron already in it, ready to draw, and empty
/// for a file that sits at the top of the torrent.
///
/// What counts as a separator is asked of the platform rather than assumed. A
/// backslash divides folders on Windows and is a perfectly legal character in a
/// filename on Linux — librqbit hands over a `PathBuf` joined with whatever the
/// platform uses, so a hard-coded backslash would cut a Linux filename in half.
/// `is_separator` knows both answers.
#[must_use]
pub fn split(path: &str) -> (String, &str) {
    path.rfind(std::path::is_separator).map_or_else(
        || (String::new(), path),
        |at| {
            let folder: Vec<&str> = path[..at].split(std::path::is_separator).collect();
            (folder.join(" › ") + " › ", &path[at + 1..])
        },
    )
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn a_file_at_the_top_has_no_folder() {
        assert_eq!(split("film.mkv"), (String::new(), "film.mkv"));
    }

    #[test]
    fn a_nested_file_keeps_both_halves() {
        assert_eq!(split("Featurettes/Trailer.mkv"), ("Featurettes › ".to_owned(), "Trailer.mkv"));
    }

    #[test]
    #[cfg(windows)]
    fn a_windows_separator_reads_the_same_as_a_unix_one() {
        // A backslash printed raw in a list of names looks like an escape
        // somebody forgot to handle. Windows only, and that is the point: on
        // Linux a backslash is a character in a filename, not a folder
        // boundary, and cutting there would rename the file on screen.
        assert_eq!(split("Featurettes\\Trailer.mkv"), ("Featurettes › ".to_owned(), "Trailer.mkv"));
    }

    #[test]
    fn every_level_shows_and_none_of_them_is_a_slash() {
        assert_eq!(
            split("Season 1/Extras/Deleted/a.mkv"),
            ("Season 1 › Extras › Deleted › ".to_owned(), "a.mkv")
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn a_backslash_is_part_of_the_name_where_the_platform_says_so() {
        // The other half of the same claim. Cutting here would rename the file
        // on screen, and the name on screen is what somebody matches against
        // what they came looking for.
        assert_eq!(split("odd\\name.mkv"), (String::new(), "odd\\name.mkv"));
    }

    #[test]
    fn a_path_that_ends_in_a_separator_has_no_name_rather_than_a_wrong_one() {
        // Not a real torrent entry, and the answer still has to be something
        // that draws: an empty name beside its folder, not a panic.
        assert_eq!(split("Extras/"), ("Extras › ".to_owned(), ""));
    }

    #[test]
    fn nothing_at_all_is_nothing_at_all() {
        assert_eq!(split(""), (String::new(), ""));
    }
}

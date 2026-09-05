//! Which files in a watched folder are torrents waiting to be added.
//!
//! A folder the app keeps an eye on: drop a `.torrent` into it — from a browser,
//! a script, a network share — and it is added without anybody opening the
//! window. It is the oldest automation a torrent client has and the one that
//! makes it a service rather than an application.
//!
//! The arithmetic is here so it can be tested without a disk. What is *not*
//! here is the reading and the renaming, which is the engine's.
//!
//! # Why the file is renamed rather than deleted
//!
//! Because deleting somebody's file is not this app's decision to make, and
//! because a name is where the outcome is written. A `.torrent` that was added
//! becomes `.torrent.added`; one that could not be becomes `.torrent.failed`.
//!
//! The failed name matters more than it looks. Leaving a broken file where it
//! was means picking it up again on the next scan, failing again, and saying so
//! again — a folder with one corrupt file in it would produce a notice every
//! few seconds for as long as the app runs.

/// What the watcher does with a file once it has tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Added,
    Failed,
}

/// The suffix a torrent file has to end in to be picked up.
const TORRENT: &str = ".torrent";

/// Whether this name is a torrent the watcher should try to add.
///
/// Exactly `.torrent`, which is also what keeps a half-written file out: a
/// browser downloads to `something.torrent.crdownload` and renames only once
/// the bytes are all there, so a name this accepts is a file that is finished.
#[must_use]
pub fn is_waiting(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(TORRENT) && lower.len() > TORRENT.len()
}

/// What to call it once the attempt is over.
#[must_use]
pub fn parked(name: &str, outcome: Outcome) -> String {
    match outcome {
        Outcome::Added => format!("{name}.added"),
        Outcome::Failed => format!("{name}.failed"),
    }
}

/// Everything in a listing that is waiting, in the order given.
///
/// Takes the listing rather than a path: the caller has already read the
/// directory, and a function that reads one cannot be tested without making
/// one.
#[must_use]
pub fn waiting(listing: &[String]) -> Vec<&str> {
    listing.iter().filter(|name| is_waiting(name)).map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::{is_waiting, parked, waiting, Outcome};

    #[test]
    fn a_torrent_file_is_what_gets_picked_up() {
        assert!(is_waiting("ubuntu-24.04.torrent"));
        assert!(is_waiting("SOMETHING.TORRENT"), "the case of an extension is not a different file");
    }

    #[test]
    fn a_file_still_being_written_is_left_alone() {
        // A browser downloads to a temporary name and renames only once the
        // bytes are all there, so requiring the exact ending is what keeps a
        // half-written torrent out of the session.
        assert!(!is_waiting("ubuntu.torrent.crdownload"));
        assert!(!is_waiting("ubuntu.torrent.part"));
        assert!(!is_waiting("ubuntu.torrent.tmp"));
    }

    #[test]
    fn what_has_already_been_dealt_with_is_not_picked_up_again() {
        // The whole reason the outcome goes in the name.
        assert!(!is_waiting("ubuntu.torrent.added"));
        assert!(!is_waiting("broken.torrent.failed"));
    }

    #[test]
    fn nothing_else_in_the_folder_is_touched() {
        assert!(!is_waiting("notes.txt"));
        assert!(!is_waiting("a folder"));
        // Not a torrent called nothing: `.torrent` on its own is an extension
        // with no file in front of it.
        assert!(!is_waiting(".torrent"));
        assert!(!is_waiting(""));
    }

    #[test]
    fn the_new_name_says_what_happened() {
        assert_eq!(parked("a.torrent", Outcome::Added), "a.torrent.added");
        assert_eq!(parked("a.torrent", Outcome::Failed), "a.torrent.failed");
    }

    #[test]
    fn a_parked_name_is_never_waiting_again() {
        // The two halves have to agree, or a failure is retried every scan and
        // a folder with one corrupt file in it complains for ever.
        for outcome in [Outcome::Added, Outcome::Failed] {
            assert!(!is_waiting(&parked("a.torrent", outcome)));
        }
    }

    #[test]
    fn a_listing_comes_back_in_the_order_it_was_given() {
        // Which is the order the disk gave them. Nothing here sorts: two
        // torrents dropped together have no order worth inventing.
        let listing: Vec<String> = ["b.torrent", "notes.txt", "a.torrent", "c.torrent.added"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(waiting(&listing), vec!["b.torrent", "a.torrent"]);
    }

    #[test]
    fn an_empty_folder_is_no_work() {
        assert!(waiting(&[]).is_empty());
    }
}

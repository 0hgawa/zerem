//! Engine failures, said in words instead of numbers.
//!
//! What reaches the state column is an `anyhow` chain with an `io::Error` at
//! the bottom, and that error's `Display` ends in `(os error 112)`. The number
//! is a fact about the kernel; it is not a fact about the download, and it is
//! not something anybody can act on. Zerem's rule is that a state always says
//! what to do about itself, so the ones worth recognising are recognised.
//!
//! The codes are per-platform on purpose. `ENOSPC` is 28 and `ERROR_DISK_FULL`
//! is 112 — the same failure with two numbers — so a single table would have
//! to be wrong on one of the two systems. Matching on the message text instead
//! was the alternative and is worse: Windows localises those, so it would work
//! on an English install and silently stop working on a Portuguese one.
//!
//! Anything unrecognised is passed through untouched. A wrong translation is
//! worse than a raw string, because the raw string can at least be searched
//! for.

/// A sentence for this failure, if it is one we know.
///
/// `None` means "say what the engine said". Deliberately not a guess: the point
/// is to replace the numbers that mean something specific, not to paraphrase
/// everything into vagueness.
#[must_use]
pub fn explain(raw: &str) -> Option<&'static str> {
    reason(os_code(raw)?).map(crate::text::tr)
}

/// The `(os error N)` that `io::Error` prints at the end of its `Display`.
fn os_code(raw: &str) -> Option<u32> {
    let tail = raw.rsplit_once("os error ")?.1;
    let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(windows)]
const fn reason(code: u32) -> Option<&'static str> {
    match code {
        // ERROR_DISK_FULL, ERROR_HANDLE_DISK_FULL.
        112 | 39 => Some("Not enough space on the disk"),
        // ERROR_ACCESS_DENIED.
        5 => Some("No permission to write in the download folder"),
        // ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND.
        2 | 3 => Some("The download folder is not there any more"),
        // ERROR_SHARING_VIOLATION — the classic: a player still holding the file.
        32 => Some("Another program has one of the files open"),
        // ERROR_WRITE_PROTECT.
        19 => Some("The download folder is read-only"),
        // ERROR_NOT_READY — a removable drive that was unplugged.
        21 => Some("That drive is not available"),
        _ => None,
    }
}

#[cfg(not(windows))]
const fn reason(code: u32) -> Option<&'static str> {
    match code {
        // ENOSPC, EDQUOT — out of room, and out of allowance, read the same to
        // whoever has to fix it.
        28 | 122 => Some("Not enough space on the disk"),
        // EACCES, EPERM.
        13 | 1 => Some("No permission to write in the download folder"),
        // ENOENT.
        2 => Some("The download folder is not there any more"),
        // EROFS.
        30 => Some("The download folder is read-only"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{explain, os_code};

    #[test]
    fn the_code_is_read_off_the_end_of_the_chain() {
        // What actually arrives: anyhow context, then the io::Error.
        let raw = "writing to file: There is not enough space on the disk. (os error 112)";
        assert_eq!(os_code(raw), Some(112));
    }

    #[test]
    fn the_last_code_wins_when_a_chain_carries_more_than_one() {
        // The innermost error is the cause; the outer ones are context.
        assert_eq!(os_code("outer (os error 2): inner (os error 112)"), Some(112));
    }

    #[test]
    fn a_message_with_no_code_is_left_alone() {
        // Passed through untouched. A wrong translation is worse than a raw
        // string, which can at least be searched for.
        assert_eq!(os_code("the tracker refused the announce"), None);
        assert_eq!(explain("the tracker refused the announce"), None);
    }

    #[test]
    fn a_code_nobody_taught_it_is_left_alone_too() {
        assert_eq!(explain("something odd (os error 4242)"), None);
    }

    #[test]
    fn running_out_of_room_says_so_rather_than_a_number() {
        // The failure this whole module exists for: it is the one a user can
        // actually fix, and "(os error 112)" tells them nothing about how.
        let raw = if cfg!(windows) { "(os error 112)" } else { "(os error 28)" };
        assert_eq!(explain(raw), Some("Not enough space on the disk"));
    }

    #[test]
    fn a_missing_folder_says_which_thing_is_missing() {
        let raw = if cfg!(windows) { "(os error 3)" } else { "(os error 2)" };
        assert_eq!(explain(raw), Some("The download folder is not there any more"));
    }

    #[test]
    fn a_permission_failure_names_the_folder_not_the_file() {
        // The fix is always the folder — the file it happened to be writing is
        // one of hundreds and means nothing to anybody.
        let raw = if cfg!(windows) { "(os error 5)" } else { "(os error 13)" };
        assert_eq!(explain(raw), Some("No permission to write in the download folder"));
    }
}

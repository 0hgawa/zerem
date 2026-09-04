//! How much room is left where something is about to be written.
//!
//! One question, asked before the write rather than diagnosed after it. A
//! transfer that dies at 94 % because the volume filled is recoverable in
//! principle and miserable in practice: the time is gone, the partial file is
//! still there, and nothing said a word until it happened.

use std::path::{Path, PathBuf};

/// Bytes available to this user on the volume holding `path`.
///
/// `None` means the question could not be answered — an unreachable volume, or
/// a platform this has not been written for. Callers are expected to fall
/// silent on `None` rather than guess: a warning built on an unknown is worse
/// than no warning, because it teaches people to dismiss the real one.
///
/// "Available to this user", not "free on the volume": on a disk with a quota
/// those differ, and the smaller one is the number that decides whether the
/// write succeeds.
#[must_use]
pub fn free_space(path: &Path) -> Option<u64> {
    imp::free_space(&nearest_existing(path)?)
}

/// The closest ancestor of `path` that exists.
///
/// A destination folder is often not created until the first file lands in it,
/// and the OS cannot report free space for a directory that is not there. The
/// volume is the same either way, so the answer is the same — walking up is how
/// to get it asked.
fn nearest_existing(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| p.is_dir()).map(Path::to_path_buf)
}

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    pub fn free_space(dir: &Path) -> Option<u64> {
        let mut available = 0u64;
        // Safe: `available` is a live local for the whole call, and the two
        // totals we do not want are passed as the API's own "not interested".
        // `&raw mut` rather than `&mut`, which clippy reads as an implicit
        // borrow-to-pointer: the API wants a pointer and saying so is clearer
        // than letting one be coerced.
        let result =
            unsafe { GetDiskFreeSpaceExW(&HSTRING::from(dir), Some(&raw mut available), None, None) };
        match result {
            Ok(()) => Some(available),
            Err(e) => {
                tracing::debug!(dir = %dir.display(), error = %e, "could not read free space");
                None
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;

    /// Not implemented here yet.
    ///
    /// The Unix answer is `statvfs`, which means libc — a dependency this crate
    /// does not have and will not acquire for one call before the Linux build
    /// needs it. `None` is honest meanwhile: the caller says nothing rather
    /// than something wrong.
    pub const fn free_space(_dir: &Path) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{free_space, nearest_existing};
    use std::path::Path;

    #[test]
    fn a_folder_that_does_not_exist_yet_answers_for_its_volume() {
        // The case this is for: someone picks a destination that will be
        // created on the first write. Asking about it directly fails, and the
        // volume is the same either way.
        let scratch = std::env::temp_dir();
        let planned = scratch.join("zerem-not-created-yet").join("nor-this");
        assert_eq!(nearest_existing(&planned).as_deref(), Some(scratch.as_path()));
    }

    #[test]
    fn a_path_with_no_existing_ancestor_is_unknown_rather_than_zero() {
        // Zero would read as "the disk is full" and warn about everything.
        // Relative and invented, so nothing above it exists on any platform.
        // An absolute path would walk up to a root, which always does.
        let nowhere = Path::new("zerem-no-such-place-3f9a/deeper");
        assert_eq!(nearest_existing(nowhere), None);
        assert_eq!(free_space(nowhere), None);
    }

    #[test]
    #[cfg(windows)]
    fn the_volume_holding_the_scratch_folder_reports_something() {
        let free = free_space(&std::env::temp_dir()).expect("a figure for the temp volume");
        assert!(free > 0, "a volume with room for the test files reported none");
    }
}

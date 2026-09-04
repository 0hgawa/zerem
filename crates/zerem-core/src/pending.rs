//! A torrent that has been read but not yet accepted.
//!
//! The add dialog is the worst-designed screen in the whole genre: every client
//! crams a file tree into a small modal and hides the destination behind it,
//! and it is where the user makes the only two decisions that matter — where it
//! goes, and what comes down. So adding is two steps here rather than one:
//! inspect, then confirm.

use std::sync::Arc;

/// One file offered by a torrent that has not been added yet.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PendingFile {
    pub path: Arc<str>,
    pub size: u64,
    /// Whether it will be downloaded. Everything is on to start with, which is
    /// what someone who just clicks "Add" expects.
    pub wanted: bool,
}

/// What the dialog is asking about.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pending {
    /// What was handed in — a magnet, a URL, or a path. Kept so the confirmation
    /// can name it if the metadata never arrives.
    pub source: Arc<str>,
    pub name: Arc<str>,
    pub files: Vec<PendingFile>,
    /// Set while a magnet's metadata is still being fetched from the swarm,
    /// which can take a while and has to look like waiting rather than like
    /// nothing happening.
    pub fetching: bool,
    /// Why it could not be read, if it could not.
    pub error: Option<Arc<str>>,
}

impl Pending {
    #[must_use]
    pub fn fetching(source: &str) -> Self {
        Self {
            source: Arc::from(source),
            name: Arc::from(source),
            files: Vec::new(),
            fetching: true,
            error: None,
        }
    }

    #[must_use]
    pub fn failed(source: &str, error: &str) -> Self {
        Self {
            source: Arc::from(source),
            name: Arc::from(source),
            files: Vec::new(),
            fetching: false,
            error: Some(Arc::from(error)),
        }
    }

    /// Total of the files that are actually wanted — which is what the user is
    /// deciding about, not the torrent's full size.
    #[must_use]
    pub fn selected_size(&self) -> u64 {
        self.files.iter().filter(|f| f.wanted).map(|f| f.size).sum()
    }

    #[must_use]
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    #[must_use]
    pub fn selected_count(&self) -> usize {
        self.files.iter().filter(|f| f.wanted).count()
    }

    /// Indices to hand the engine. `None` means all of them, which is what
    /// librqbit wants rather than a list naming every file.
    #[must_use]
    pub fn only_files(&self) -> Option<Vec<usize>> {
        if self.files.iter().all(|f| f.wanted) {
            return None;
        }
        Some(self.files.iter().enumerate().filter(|(_, f)| f.wanted).map(|(i, _)| i).collect())
    }

    /// Whether there is anything to add. Confirming with nothing ticked would
    /// produce a torrent that downloads nothing, so the button says no.
    #[must_use]
    pub fn is_addable(&self) -> bool {
        self.error.is_none() && !self.fetching && self.selected_count() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::{Pending, PendingFile};
    use std::sync::Arc;

    fn pending(sizes: &[(u64, bool)]) -> Pending {
        Pending {
            source: Arc::from("magnet:?xt=urn:btih:abc"),
            name: Arc::from("Something"),
            files: sizes
                .iter()
                .enumerate()
                .map(|(i, &(size, wanted))| PendingFile {
                    path: Arc::from(format!("f{i}.bin").as_str()),
                    size,
                    wanted,
                })
                .collect(),
            fetching: false,
            error: None,
        }
    }

    #[test]
    fn everything_wanted_means_no_file_list_for_the_engine() {
        // librqbit wants `None`, not a list naming every file.
        assert_eq!(pending(&[(1, true), (2, true)]).only_files(), None);
    }

    #[test]
    fn a_partial_choice_names_the_indices_it_kept() {
        let p = pending(&[(1, true), (2, false), (3, true)]);
        assert_eq!(p.only_files(), Some(vec![0, 2]));
    }

    #[test]
    fn the_size_shown_is_what_was_chosen_not_what_exists() {
        // The number the user is deciding about is the one they will download.
        let p = pending(&[(100, true), (900, false)]);
        assert_eq!(p.selected_size(), 100);
        assert_eq!(p.total_size(), 1000);
    }

    #[test]
    fn nothing_ticked_cannot_be_added() {
        // It would produce a torrent that downloads nothing.
        assert!(!pending(&[(1, false), (2, false)]).is_addable());
        assert!(pending(&[(1, false), (2, true)]).is_addable());
    }

    #[test]
    fn neither_a_fetch_in_progress_nor_a_failure_is_addable() {
        assert!(!Pending::fetching("magnet:?xt=urn:btih:abc").is_addable());
        assert!(!Pending::failed("nonsense", "not a magnet link").is_addable());
    }
}

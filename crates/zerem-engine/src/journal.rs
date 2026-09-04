//! What to put back if the process does not get to put it back itself.
//!
//! Fetching one file first works by narrowing what the session is fetching to
//! that file alone. The narrowed set is what librqbit persists, so a process
//! killed while a file is pinned would come back having quietly forgotten which
//! files the user had actually ticked — and the user would find eleven of
//! twelve switched off with no idea why.
//!
//! So the selection is written down before it is narrowed and removed once it
//! is restored. Almost always the file is absent or empty; it exists for the
//! minutes a pin is live.
//!
//! A pin itself is deliberately *not* restored. "Fetch this one first" is an
//! instruction given in a moment, not a setting — coming back to a torrent
//! still holding the rest of itself hostage, days later, would be the app
//! remembering the wrong half.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// One line per torrent: its infohash, then the file indices that were ticked.
///
/// Hand-rolled rather than JSON, and not to save a dependency: the whole format
/// is two fields, it has to survive being read after a crash wrote half of it,
/// and a line that does not parse is skipped rather than taking the rest of the
/// file down with it.
#[derive(Default, Debug)]
pub struct Journal {
    path: PathBuf,
    entries: HashMap<String, Vec<usize>>,
}

impl Journal {
    #[must_use]
    pub fn open(state_dir: &Path) -> Self {
        let path = state_dir.join("narrowed.txt");
        let entries = std::fs::read_to_string(&path).map(|text| parse(&text)).unwrap_or_default();
        if !entries.is_empty() {
            tracing::info!(count = entries.len(), "restoring file selections a previous run narrowed");
        }
        Self { path, entries }
    }

    /// What the previous run had ticked, if it was interrupted mid-pin.
    #[must_use]
    pub fn take(&mut self, info_hash: &str) -> Option<Vec<usize>> {
        self.entries.remove(info_hash)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Record a selection about to be narrowed, or forget one restored.
    pub fn set(&mut self, info_hash: &str, wanted: Option<Vec<usize>>) {
        match wanted {
            Some(wanted) => self.entries.insert(info_hash.to_owned(), wanted),
            None => self.entries.remove(info_hash),
        };
        self.write();
    }

    /// Persist whatever is held, after a run of [`Self::take`] has emptied it.
    pub fn flush(&self) {
        self.write();
    }

    /// Written whole every time, because it is at most a handful of short lines
    /// and a partial update is the failure this exists to survive.
    fn write(&self) {
        if self.entries.is_empty() {
            // Absent rather than empty: the common state is no pins at all, and
            // a file that is there says something happened.
            let _ = std::fs::remove_file(&self.path);
            return;
        }
        let mut text = String::new();
        for (hash, wanted) in &self.entries {
            let _ = write!(text, "{hash}");
            for i in wanted {
                let _ = write!(text, " {i}");
            }
            text.push('\n');
        }
        if let Err(e) = write_atomically(&self.path, &text) {
            tracing::warn!(error = %e, "could not record the narrowed selection");
        }
    }
}

/// Same rule the settings file follows: a torn write here would be a selection
/// nobody can get back.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;

    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

fn parse(text: &str) -> HashMap<String, Vec<usize>> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let hash = fields.next()?;
            // A torrent with nothing ticked cannot happen and would mean
            // fetching nothing, so an entry that says so is a torn line.
            let wanted: Vec<usize> = fields.filter_map(|f| f.parse().ok()).collect();
            (!wanted.is_empty()).then(|| (hash.to_owned(), wanted))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse, Journal};

    #[test]
    fn a_selection_survives_being_written_and_read() {
        let dir = std::env::temp_dir().join("zerem-journal-roundtrip");
        std::fs::create_dir_all(&dir).expect("scratch");
        let _ = std::fs::remove_file(dir.join("narrowed.txt"));

        let mut journal = Journal::open(&dir);
        journal.set("abc123", Some(vec![0, 3, 7]));

        let mut reopened = Journal::open(&dir);
        assert_eq!(reopened.take("abc123"), Some(vec![0, 3, 7]));
        assert_eq!(reopened.take("abc123"), None, "taking it is what consumes it");
    }

    #[test]
    fn restoring_the_last_pin_leaves_no_file_behind() {
        let dir = std::env::temp_dir().join("zerem-journal-empty");
        std::fs::create_dir_all(&dir).expect("scratch");

        let mut journal = Journal::open(&dir);
        journal.set("abc123", Some(vec![1]));
        journal.set("abc123", None);

        assert!(Journal::open(&dir).is_empty());
        assert!(!dir.join("narrowed.txt").exists(), "no pins is no file");
    }

    #[test]
    fn a_torn_line_is_skipped_rather_than_taking_the_file_down() {
        // The whole reason this is two fields and not JSON: it is read after
        // exactly the kind of exit that can leave half a line behind.
        let entries = parse("aaa 0 1 2\nbbb\n\nccc 4\nddd not-a-number\n");
        assert_eq!(entries.get("aaa"), Some(&vec![0, 1, 2]));
        assert_eq!(entries.get("ccc"), Some(&vec![4]));
        assert_eq!(entries.get("bbb"), None, "a hash with no selection says nothing");
        assert_eq!(entries.get("ddd"), None);
    }
}

/// A plain set of infohashes, written one per line.
///
/// The queue pauses torrents on the user's behalf, which means librqbit's own
/// paused flag stops being the answer to "did somebody stop this?" — the same
/// lesson `only_files` taught above. So what the queue paused is written down,
/// and a run that comes back finds its own work rather than mistaking it for
/// the user's.
///
/// Absent almost always: it holds a line only while more torrents are wanted
/// than the limit allows.
#[derive(Default, Debug)]
pub struct Roster {
    path: PathBuf,
    hashes: HashSet<String>,
}

impl Roster {
    #[must_use]
    pub fn open(state_dir: &Path, name: &str) -> Self {
        let path = state_dir.join(name);
        let hashes = std::fs::read_to_string(&path)
            .map(|text| text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default();
        Self { path, hashes }
    }

    #[must_use]
    pub fn holds(&self, info_hash: &str) -> bool {
        self.hashes.contains(info_hash)
    }

    /// Whether it is holding anything, which is what lets the caller skip the
    /// whole pass when there is no limit and nothing to undo.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Replace the whole set, writing only when it actually changed.
    ///
    /// Called every tick, which is why it compares first: the steady state is
    /// no change at all, and rewriting a file once a second for nothing is the
    /// kind of thing that shows up in somebody's disk activity graph.
    pub fn keep(&mut self, hashes: HashSet<String>) {
        if hashes == self.hashes {
            return;
        }
        self.hashes = hashes;
        if self.hashes.is_empty() {
            let _ = std::fs::remove_file(&self.path);
            return;
        }
        let mut text = String::new();
        for hash in &self.hashes {
            let _ = writeln!(text, "{hash}");
        }
        if let Err(e) = write_atomically(&self.path, &text) {
            tracing::warn!(error = %e, "could not record what the queue paused");
        }
    }
}

#[cfg(test)]
mod roster_tests {
    use super::Roster;

    #[test]
    fn what_the_queue_paused_survives_the_process_that_paused_it() {
        let dir = std::env::temp_dir().join("zerem-roster");
        std::fs::create_dir_all(&dir).expect("scratch");
        let _ = std::fs::remove_file(dir.join("queued.txt"));

        let mut roster = Roster::open(&dir, "queued.txt");
        roster.keep(["aaa".to_owned(), "bbb".to_owned()].into_iter().collect());

        let reopened = Roster::open(&dir, "queued.txt");
        assert!(reopened.holds("aaa"));
        assert!(reopened.holds("bbb"));
        assert!(!reopened.holds("ccc"));
    }

    #[test]
    fn an_empty_queue_leaves_no_file_behind() {
        // The steady state for most people is no queue at all, and a file that
        // is there says something is being held back.
        let dir = std::env::temp_dir().join("zerem-roster-empty");
        std::fs::create_dir_all(&dir).expect("scratch");

        let mut roster = Roster::open(&dir, "queued.txt");
        roster.keep(std::iter::once("aaa".to_owned()).collect());
        roster.keep(std::collections::HashSet::new());

        assert!(!dir.join("queued.txt").exists());
        assert!(!Roster::open(&dir, "queued.txt").holds("aaa"));
    }
}

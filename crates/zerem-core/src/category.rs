//! Which shelf a torrent goes on, and where that shelf is.
//!
//! A category is a name and a folder, and the folder is the whole point: the
//! reason to put a torrent on "Series" is so it lands where series live without
//! anybody navigating there again.
//!
//! There is no category manager, and that is deliberate rather than unfinished.
//! One is made by typing a name in the add dialog and choosing where that
//! download goes; from then on the name means that folder. A separate screen
//! for creating something whose whole definition is two fields already on
//! screen would be a second place to do the same thing.
//!
//! The model is a map of names to folders and nothing more. A struct would have
//! wanted `serde` to reach the settings file, and this crate's dependency list
//! is deliberately empty — a shelf is two strings, and two strings do not need
//! a type to be two strings.

use std::collections::BTreeMap;

/// Name as typed → the folder it writes to.
///
/// `BTreeMap` so the rail lists them in an order somebody can predict and
/// adding one does not shuffle the others under the cursor, and so the settings
/// file is stable between saves.
pub type Shelves = BTreeMap<String, String>;

/// Fold a name for comparison.
///
/// Names are matched case- and space-insensitively so that "TV Shows",
/// "tv shows" and "TV  Shows" are one shelf rather than three. What is *shown*
/// is always the name as first typed — folding decides sameness, never display.
#[must_use]
pub fn key(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Whether a name can be a category at all.
///
/// Empty is not a category, it is the absence of one — a real answer, and one
/// stored as no assignment rather than as a shelf called "".
#[must_use]
pub fn is_named(name: &str) -> bool {
    !key(name).is_empty()
}

/// The name as it is stored, for a name as it was typed.
#[must_use]
pub fn canonical<'a>(shelves: &'a Shelves, name: &str) -> Option<&'a str> {
    let wanted = key(name);
    shelves.keys().find(|stored| key(stored) == wanted).map(String::as_str)
}

/// The folder a category writes to, if the app has been told.
#[must_use]
pub fn folder_of<'a>(shelves: &'a Shelves, name: &str) -> Option<&'a str> {
    canonical(shelves, name).and_then(|stored| shelves.get(stored)).map(String::as_str)
}

/// Remember a category, or move an existing one.
///
/// Re-typing a name with a different folder moves the shelf rather than making
/// a second one: two shelves called "Series" pointing at different places is a
/// category system nobody can predict. The spelling first used is kept, so the
/// rail does not rename itself under somebody who typed it differently once.
pub fn remember(shelves: &mut Shelves, name: &str, folder: &str) {
    if !is_named(name) {
        return;
    }
    let stored = canonical(shelves, name).map_or_else(|| name.trim().to_owned(), str::to_owned);
    shelves.insert(stored, folder.to_owned());
}

/// Drop a category and say whether it was there.
///
/// The torrents on it are not touched: their files are where they are, and a
/// shelf being removed is not a reason to move anything on disk.
pub fn forget(shelves: &mut Shelves, name: &str) -> bool {
    canonical(shelves, name).map(str::to_owned).is_some_and(|stored| shelves.remove(&stored).is_some())
}

#[cfg(test)]
mod tests {
    use super::{canonical, folder_of, forget, is_named, key, remember, Shelves};

    fn shelves(pairs: &[(&str, &str)]) -> Shelves {
        pairs.iter().map(|(n, f)| ((*n).to_owned(), (*f).to_owned())).collect()
    }

    #[test]
    fn one_shelf_however_it_is_typed() {
        // Somebody who types it slightly differently on the second torrent has
        // not made a second category, they have made a typo.
        assert_eq!(key("TV Shows"), key("tv shows"));
        assert_eq!(key("TV  Shows"), key("tv shows"));
        assert_eq!(key("  Series "), "series");
    }

    #[test]
    fn nothing_typed_is_not_a_shelf_called_nothing() {
        assert!(!is_named(""));
        assert!(!is_named("   "));
        assert!(is_named("Series"));
    }

    #[test]
    fn the_name_is_kept_as_typed_even_though_matching_ignores_it() {
        // A rail that shows "tv shows" when somebody typed "TV Shows" is an app
        // correcting them for no reason.
        let mut shelf = Shelves::new();
        remember(&mut shelf, "TV Shows", "D:/TV");
        assert_eq!(canonical(&shelf, "tv  shows"), Some("TV Shows"));
        assert_eq!(folder_of(&shelf, "tv shows"), Some("D:/TV"));
    }

    #[test]
    fn retyping_a_name_moves_the_shelf_rather_than_making_a_second() {
        // Two shelves called "Series" pointing at different places is a category
        // system nobody can predict.
        let mut shelf = shelves(&[("Series", "D:/old")]);
        remember(&mut shelf, "series", "E:/new");
        assert_eq!(shelf.len(), 1);
        assert_eq!(folder_of(&shelf, "Series"), Some("E:/new"));
        assert_eq!(canonical(&shelf, "SERIES"), Some("Series"), "the first spelling is kept");
    }

    #[test]
    fn an_unnamed_shelf_is_never_stored() {
        let mut shelf = Shelves::new();
        remember(&mut shelf, "  ", "D:/anywhere");
        assert!(shelf.is_empty());
    }

    #[test]
    fn shelves_come_out_in_an_order_somebody_can_predict() {
        // And a new one does not shuffle the others under the cursor.
        let mut shelf = Shelves::new();
        for name in ["Series", "Anime", "Films"] {
            remember(&mut shelf, name, "D:/x");
        }
        let names: Vec<&str> = shelf.keys().map(String::as_str).collect();
        assert_eq!(names, vec!["Anime", "Films", "Series"]);
    }

    #[test]
    fn forgetting_says_whether_there_was_anything_to_forget() {
        let mut shelf = shelves(&[("Series", "D:/tv")]);
        assert!(forget(&mut shelf, "SERIES"));
        assert!(shelf.is_empty());
        assert!(!forget(&mut shelf, "Series"), "and again is not an error, it is a no");
    }

    #[test]
    fn a_name_nobody_has_used_has_no_folder() {
        assert_eq!(folder_of(&shelves(&[("Series", "D:/tv")]), "Films"), None);
    }
}

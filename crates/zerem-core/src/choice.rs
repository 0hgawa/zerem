//! What the user wants fetched, and what they want fetched *first*.
//!
//! librqbit has one lever for this and only one: `only_files`, the set it is
//! fetching. There is no priority in its public API — the ordering it uses is
//! `pub(crate)`, sorted by filename, and carries a `// TODO: make it
//! configurable`. A three-position control in which two positions do the same
//! thing would be a lie about what the engine does, which is why this app has
//! never had one.
//!
//! So "first" is built out of the lever that exists: while a file is first, it
//! is the only thing being fetched. Everything else stops and resumes the
//! moment it lands. That is a different bargain from qBittorrent's queue —
//! stricter, and for the case people actually reach for priority in (get this
//! episode *now*) it is the better one, because the whole line goes to it.
//!
//! Narrowing is safe: librqbit's `update_only_files` replaces which pieces are
//! *selected* and leaves which pieces are *had* untouched, so widening again
//! returns to exactly the progress that was there. Verified in its
//! `chunk_tracker`, because getting this wrong would silently discard
//! gigabytes.

/// A per-file flag list from librqbit's `only_files`.
///
/// `None` is its "no restriction", which means every file — not none of them.
/// Read the other way round, a torrent downloading normally would be drawn
/// with everything switched off.
///
/// A lookup rather than a search: this feeds loops over file counts that do
/// reach four figures, and `contains` inside one of those is quadratic.
#[must_use]
pub fn flags(only: Option<&[usize]>, count: usize) -> Vec<bool> {
    only.map_or_else(
        || vec![true; count],
        |list| {
            let mut flags = vec![false; count];
            for &i in list.iter().filter(|&&i| i < count) {
                flags[i] = true;
            }
            flags
        },
    )
}

/// The tick list that switching files on or off would produce, or `None` when
/// the change names a file the torrent does not have.
///
/// `file` names one, or is `None` for every file at once — which is what makes
/// a torrent of four thousand files editable by hand, and what the header tick
/// above the list is.
///
/// Nothing ticked is allowed, and that reverses an earlier rule. It was refused
/// because a torrent fetching nothing looked like a state nobody reaches for on
/// purpose — but it made the header tick a control that only worked one way,
/// which reads as a control that does not work. Fetching nothing is a real
/// answer: keep what is on disk, share it, take no more. The row says so
/// instead of the click being swallowed.
///
/// The add dialog still refuses it, and that is not the same rule: there, zero
/// files means not adding the torrent, and the button for that says Cancel.
#[must_use]
pub fn ticked(current: &[bool], file: Option<usize>, wanted: bool) -> Option<Vec<bool>> {
    let mut next = current.to_vec();
    match file {
        Some(i) => *next.get_mut(i)? = wanted,
        None => next.fill(wanted),
    }
    Some(next)
}

/// Which files to ask the engine for.
///
/// `first` narrows; it never widens. A file nobody ticked cannot be fetched by
/// pinning it, and a pin on a file that is already complete is not a reason to
/// stop everything else — both are filtered here rather than guarded against at
/// each of the several places that could set them.
///
/// Returns `None` for "no restriction", which is what librqbit means by a
/// missing `only_files` — and is not the same as an empty list, which means
/// fetch nothing.
#[must_use]
pub fn to_fetch(wanted: &[bool], first: &[bool], complete: &[bool]) -> Option<Vec<usize>> {
    let pinned: Vec<usize> = (0..wanted.len())
        .filter(|&i| wanted[i] && first.get(i).copied().unwrap_or(false))
        .filter(|&i| !complete.get(i).copied().unwrap_or(false))
        .collect();
    if !pinned.is_empty() {
        return Some(pinned);
    }

    let chosen: Vec<usize> = (0..wanted.len()).filter(|&i| wanted[i]).collect();
    // Every file: the engine wants `None` for that, not a list naming them all.
    if chosen.len() == wanted.len() {
        return None;
    }
    Some(chosen)
}

/// Whether anything is still being held back, which is what decides between
/// saying so and saying nothing.
///
/// The same filtering as [`to_fetch`], asked as a question: a pin that survives
/// on a finished file is not holding anything back.
#[must_use]
pub fn is_narrowed(wanted: &[bool], first: &[bool], complete: &[bool]) -> bool {
    (0..wanted.len()).any(|i| {
        wanted[i] && first.get(i).copied().unwrap_or(false) && !complete.get(i).copied().unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::{flags, is_narrowed, ticked, to_fetch};

    #[test]
    fn switching_one_file_leaves_the_rest_alone() {
        assert_eq!(ticked(&[true; 3], Some(1), false), Some(vec![true, false, true]));
        assert_eq!(ticked(&[true, false, false], Some(2), true), Some(vec![true, false, true]));
    }

    #[test]
    fn no_file_named_is_every_file_at_once() {
        // The only thing that makes a torrent of four thousand files editable.
        assert_eq!(ticked(&[false, true, false], None, true), Some(vec![true; 3]));
    }

    #[test]
    fn switching_everything_off_is_allowed() {
        // It was refused once, and that made the header tick a control that
        // only worked one way — which reads as one that does not work.
        // Fetching nothing is a real answer: keep what is on disk, share it,
        // take no more. The row says so rather than the click vanishing.
        assert_eq!(ticked(&[false, true, false], Some(1), false), Some(vec![false; 3]));
        assert_eq!(ticked(&[true; 3], None, false), Some(vec![false; 3]));
    }

    #[test]
    fn switching_a_file_to_what_it_already_is_changes_nothing() {
        assert_eq!(ticked(&[true, false, true], Some(0), true), Some(vec![true, false, true]));
    }

    #[test]
    fn an_index_past_the_end_is_refused_rather_than_panicking() {
        assert_eq!(ticked(&[true; 3], Some(3), true), None);
    }

    #[test]
    fn no_restriction_means_every_file_not_none_of_them() {
        // Read the wrong way round, a torrent downloading normally would be
        // drawn with every file switched off.
        assert_eq!(flags(None, 3), vec![true, true, true]);
        assert_eq!(flags(Some(&[0, 2]), 3), vec![true, false, true]);
    }

    #[test]
    fn an_index_past_the_end_is_ignored_rather_than_panicking() {
        // The list comes back from the engine, and a stale one outliving a
        // metadata change should not take the window down.
        assert_eq!(flags(Some(&[0, 9]), 2), vec![true, false]);
    }

    #[test]
    fn no_pin_and_everything_ticked_is_no_restriction_at_all() {
        // `None`, not a list naming all four: that is what librqbit reads as
        // "fetch the whole torrent", and a list is a chunk-tracker rebuild.
        assert_eq!(to_fetch(&[true; 4], &[false; 4], &[false; 4]), None);
    }

    #[test]
    fn without_a_pin_the_ticks_are_the_answer() {
        let wanted = [true, false, true, false];
        assert_eq!(to_fetch(&wanted, &[false; 4], &[false; 4]), Some(vec![0, 2]));
    }

    #[test]
    fn a_pin_makes_that_file_the_only_thing_being_fetched() {
        // The whole point: everything else stops so the line goes to this one.
        let first = [false, false, true, false];
        assert_eq!(to_fetch(&[true; 4], &first, &[false; 4]), Some(vec![2]));
        assert!(is_narrowed(&[true; 4], &first, &[false; 4]));
    }

    #[test]
    fn several_pins_are_fetched_together() {
        let first = [true, false, true, false];
        assert_eq!(to_fetch(&[true; 4], &first, &[false; 4]), Some(vec![0, 2]));
    }

    #[test]
    fn a_pin_cannot_fetch_a_file_that_was_never_ticked() {
        // Otherwise pinning would quietly undo a skip, and the two controls
        // would be arguing about the same file.
        let wanted = [true, false, true, true];
        let first = [false, true, false, false];
        assert_eq!(to_fetch(&wanted, &first, &[false; 4]), Some(vec![0, 2, 3]));
        assert!(!is_narrowed(&wanted, &first, &[false; 4]));
    }

    #[test]
    fn a_pin_on_a_finished_file_holds_nothing_back() {
        // This is what ends the narrowing: the file lands, the pin stops
        // counting, and the rest of the torrent is asked for again on the very
        // next tick without anybody pressing anything.
        let first = [false, false, true, false];
        let complete = [false, false, true, false];
        assert_eq!(to_fetch(&[true; 4], &first, &complete), None);
        assert!(!is_narrowed(&[true; 4], &first, &complete));
    }

    #[test]
    fn the_last_pin_landing_is_what_releases_the_rest() {
        let first = [true, false, true, false];
        assert_eq!(to_fetch(&[true; 4], &first, &[true, false, false, false]), Some(vec![2]));
        assert_eq!(to_fetch(&[true; 4], &first, &[true, false, true, false]), None);
    }

    #[test]
    fn a_torrent_with_no_files_yet_asks_for_no_restriction() {
        // A magnet before its metadata. Returning an empty list here would tell
        // the engine to fetch nothing at all.
        assert_eq!(to_fetch(&[], &[], &[]), None);
        assert!(!is_narrowed(&[], &[], &[]));
    }
}

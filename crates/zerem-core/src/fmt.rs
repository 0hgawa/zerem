//! Display formatting.
//!
//! The only place a number becomes a string. The `.slint` never formats
//! anything: a format call written inside a `for` in the UI runs once per row
//! per frame, which is the cost this exists to avoid.
//!
//! Promoted from the Phase 0 spike, where it was measured at ~1.1 µs per row:
//! formatting, not comparing, is what a tick actually costs.

/// Unit ladder. Binary maths with the labels torrent clients conventionally
/// show, which is what users compare against other clients.
const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

/// Human byte count with *stable* precision.
///
/// The decimal count is chosen from the mantissa, not fixed globally, so a
/// value reads as `1.44 GB` / `14.4 GB` / `144 GB` and keeps roughly the same
/// width as it grows. That width stability is why the column does not visibly
/// twitch once a second.
#[must_use]
pub fn bytes(n: u64) -> String {
    if n == 0 {
        return "0 B".into();
    }
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{n} B");
    }
    // Three significant figures: 2 decimals below ten, 1 below a hundred, none
    // above. Summing the two predicates says exactly that in one line.
    let decimals = usize::from(v < 10.0) + usize::from(v < 100.0);
    format!("{v:.decimals$} {}", UNITS[unit])
}

/// Transfer rate. Zero renders as an em dash rather than `0 B/s`: an idle row
/// should read as empty, not as a measurement that happens to be zero.
#[must_use]
pub fn speed(bps: u64) -> String {
    if bps == 0 {
        "—".into()
    } else {
        format!("{}/s", bytes(bps))
    }
}

/// Remaining time, coarsened as it grows.
///
/// Past a day the answer is never precise enough to be worth the digits, and a
/// seconds field ticking down inside a two-day estimate is noise that also
/// forces a repaint of the row every single tick.
#[must_use]
pub fn eta(secs: Option<u32>) -> String {
    let Some(s) = secs else { return "∞".into() };
    match s {
        0 => "—".into(),
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s if s < 86_400 => format!("{}h {}m", s / 3600, (s % 3600) / 60),
        s => format!("{}d {}h", s / 86_400, (s % 86_400) / 3600),
    }
}

/// Share ratio, carried as hundredths so the domain value is exact and its
/// `PartialEq` cannot be tripped by float noise.
#[must_use]
pub fn ratio(x100: u32) -> String {
    format!("{}.{:02}", x100 / 100, x100 % 100)
}

/// Peers as `connected (total)` — the shape every client uses, so it needs no
/// column header explanation.
#[must_use]
pub fn peers(connected: u32, total: u32) -> String {
    format!("{connected} ({total})")
}

/// Completion percentage. Integer on purpose: a decimal place here changes
/// every tick on a fast torrent and repaints the row for a digit nobody reads.
#[must_use]
pub fn percent(done: u64, size: u64) -> String {
    if size == 0 {
        return "0%".into();
    }
    format!("{}%", (done * 100 / size).min(100))
}

/// Transferred against total, with the percentage — the whole of what used to
/// be a Size column and a Progress column.
///
/// A finished torrent shows its size and nothing else. Most of a long-lived
/// list is finished, and "1.92 GB / 1.92 GB · 100%" is three ways of saying one
/// thing in a column that then reads as noise all the way down.
#[must_use]
pub fn progress(done: u64, size: u64) -> String {
    if done >= size {
        return bytes(size);
    }
    format!("{} / {} · {}", bytes(done), bytes(size), percent(done, size))
}

/// What to say when a download will not fit where it is going, and nothing
/// when it will.
///
/// The shortfall, not the two totals: it is the one actionable number — how
/// much has to be freed, or how much has to come off the list of ticks. What
/// the torrent needs is already on the summary line right below it.
///
/// Exactly filling the volume is not warned about. The rule has to be crisp
/// enough to state, and "it fits" is the rule.
#[must_use]
pub fn shortfall(needed: u64, free: u64) -> Option<String> {
    let short = needed.checked_sub(free).filter(|&short| short > 0)?;
    Some(format!("Not enough room in this folder — {} short", bytes(short)))
}

/// What the line above the file list says while something is being fetched
/// first.
///
/// Replaces the count rather than joining it. "3 of 12 files" stops being the
/// interesting fact the moment the other nine have stopped, and two lines
/// disagreeing about the same list is one line too many.
#[must_use]
pub fn fetching_first(pinned: usize, waiting: usize) -> String {
    let files = if pinned == 1 { "file" } else { "files" };
    if waiting == 0 {
        return format!("{pinned} {files} first");
    }
    format!("{pinned} {files} first · {waiting} waiting")
}

/// How much of the list is showing, while a filter is on. Replaces the plain
/// total rather than joining it: two counts side by side is one too many.
#[must_use]
pub fn matched(shown: usize, total: usize) -> String {
    format!("{shown} of {total}")
}

#[cfg(test)]
mod tests {
    use super::{bytes, eta, fetching_first, matched, percent, progress, ratio, shortfall, speed};

    #[test]
    fn a_pinned_file_says_what_it_is_costing_the_rest() {
        // The number that matters is not how many are pinned, it is how many
        // stopped so that it could go first.
        assert_eq!(fetching_first(1, 11), "1 file first · 11 waiting");
        assert_eq!(fetching_first(3, 9), "3 files first · 9 waiting");
    }

    #[test]
    fn nothing_waiting_is_not_mentioned() {
        // Pinning every file left is not holding anything back, and saying
        // "0 waiting" would suggest it was.
        assert_eq!(fetching_first(2, 0), "2 files first");
    }

    #[test]
    fn byte_precision_keeps_a_stable_width() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(512), "512 B");
        // Three significant figures across the whole range is what stops the
        // column from twitching as a value grows.
        assert_eq!(bytes(1024), "1.00 KB");
        assert_eq!(bytes(1024 * 1024 + 471_859), "1.45 MB");
        assert_eq!(bytes(15 * 1024 * 1024), "15.0 MB");
        assert_eq!(bytes(150 * 1024 * 1024), "150 MB");
    }

    #[test]
    fn idle_rates_read_as_empty_not_as_zero() {
        assert_eq!(speed(0), "—");
        assert_eq!(speed(2048), "2.00 KB/s");
    }

    #[test]
    fn eta_coarsens_as_it_grows() {
        assert_eq!(eta(None), "∞");
        assert_eq!(eta(Some(0)), "—");
        assert_eq!(eta(Some(45)), "45s");
        assert_eq!(eta(Some(90)), "1m 30s");
        assert_eq!(eta(Some(3700)), "1h 1m");
        assert_eq!(eta(Some(90_000)), "1d 1h");
    }

    #[test]
    fn ratio_is_exact_because_it_is_an_integer() {
        assert_eq!(ratio(0), "0.00");
        assert_eq!(ratio(105), "1.05");
        assert_eq!(ratio(1234), "12.34");
    }

    #[test]
    fn percent_never_exceeds_one_hundred() {
        assert_eq!(percent(0, 0), "0%");
        assert_eq!(percent(50, 200), "25%");
        // Over-reported progress must not render as 101%.
        assert_eq!(percent(300, 200), "100%");
    }

    #[test]
    fn progress_carries_both_the_absolute_and_the_relative() {
        // The absolute figure is the one people compare against a disk and
        // against another client; the percentage is the one they glance at.
        assert_eq!(progress(515_000_000, 2_061_584_302), "491 MB / 1.92 GB · 24%");
    }

    #[test]
    fn a_finished_torrent_states_its_size_once() {
        assert_eq!(progress(2_061_584_302, 2_061_584_302), "1.92 GB");
        // Over-reported, which librqbit does at the end of a re-check.
        assert_eq!(progress(3_000, 2_000), "1.95 KB");
    }

    #[test]
    fn a_magnet_without_metadata_reads_as_empty_not_as_broken() {
        // `total_bytes: 0` until the swarm answers, which would divide by zero
        // anywhere the percentage was computed instead.
        assert_eq!(progress(0, 0), "0 B");
    }

    #[test]
    fn room_is_only_mentioned_when_there_is_not_enough() {
        assert_eq!(shortfall(1_000, 5_000), None);
        // Exactly filling the volume fits, and the rule has to be crisp enough
        // to state.
        assert_eq!(shortfall(5_000, 5_000), None);
    }

    #[test]
    fn the_shortfall_is_the_number_that_can_be_acted_on() {
        // Not "needs 3.72 GB, 1.21 GB free" — the figure someone needs is how
        // much to free or to untick, and the total is on the line below.
        assert_eq!(
            shortfall(4_000_000_000, 1_300_000_000).as_deref(),
            Some("Not enough room in this folder — 2.51 GB short")
        );
    }

    #[test]
    fn an_empty_volume_is_reported_as_the_whole_amount() {
        // Free space of zero is a real answer, not a missing one — the caller
        // passes `None` through without asking when it does not know.
        assert!(shortfall(1_024, 0).is_some());
    }

    #[test]
    fn a_filtered_count_says_what_it_is_out_of() {
        assert_eq!(matched(12, 300), "12 of 300");
        assert_eq!(matched(0, 300), "0 of 300");
    }
}

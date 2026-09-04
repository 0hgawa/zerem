//! Display formatting.
//!
//! The only place a number becomes a string. The `.slint` never formats
//! anything: a format call written inside a `for` in the UI runs once per row
//! per frame, which is exactly the cost this spike exists to avoid.
//!
//! Survives the spike — this is `zerem-core` material.

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

#[cfg(test)]
mod tests {
    use super::{bytes, eta, percent, ratio, speed};

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
}

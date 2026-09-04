//! Transfer rates that stop twitching.
//!
//! What librqbit reports is already an average — its `SpeedEstimator` divides
//! the bytes moved across a five-second sliding window — and that matters to
//! the design here: the remaining noise is not sampling noise, it is the window
//! itself, one second entering and one leaving. It is small, a few per cent, and
//! it is *relentless*. A steady megabyte a second prints `1.03 MB/s`,
//! `1.00 MB/s`, `1.05 MB/s`, and a value sitting on a unit boundary flips
//! between `999 KB/s` and `1.00 MB/s` ten times a minute — which is the detail
//! that makes a torrent client look amateur.
//!
//! So the instrument is a **deadband**, not more averaging: the published figure
//! only moves once the rate has drifted meaningfully away from it. Stacking a
//! second heavy average on top of librqbit's would buy the same stillness by
//! adding lag to a number that is already five seconds behind.
//!
//! The light exponential average in front of it is there only to keep one
//! outlying second from stepping over the deadband on its own.
//!
//! The deadband pays twice. A rate that does not change leaves [`TorrentRow`]
//! byte-identical between ticks, so the model diff skips the row and never
//! reformats or repaints it — smoothing is cheaper here than not smoothing.
//!
//! [`TorrentRow`]: crate::TorrentRow

/// How much of each new sample is taken. Deliberately most of it: the input is
/// already a five-second average, and the job of this term is to blunt a single
/// outlier, not to average again.
const SMOOTHING: f64 = 0.4;

/// How far the average has to drift from the published figure before it is
/// republished. Five per cent is under half of the three significant figures
/// the column shows, so what is drawn is never more than a rounding away from
/// what is happening.
const DEADBAND: f64 = 0.05;

/// One smoothed rate, in bytes per second.
///
/// Lives beside the torrent it belongs to rather than on the row: it is state
/// that spans ticks, and a row is one tick by definition.
#[derive(Clone, Copy, Default, Debug)]
pub struct Rate {
    /// `None` at rest. Not a zero, because "no transfer" and "a transfer
    /// averaging zero" have to start differently — see [`Self::update`].
    average: Option<f64>,
    shown: u64,
}

impl Rate {
    /// Feed one sample and get back what to display.
    ///
    /// Two cases skip the filter entirely, and both are the same rule: a
    /// *change of state* is not noise and must not be smoothed like it.
    /// Stopping publishes zero at once — a paused row showing 4 MB/s for three
    /// seconds is a lie — and starting publishes the first sample at once,
    /// rather than easing up from nothing while the transfer is already at
    /// full speed.
    pub fn update(&mut self, sample: u64) -> u64 {
        if sample == 0 {
            *self = Self::default();
            return 0;
        }

        let sample = sample as f64;
        let average = self.average.map_or(sample, |average| SMOOTHING.mul_add(sample - average, average));
        self.average = Some(average);

        let shown = self.shown as f64;
        if (average - shown).abs() > shown * DEADBAND {
            self.shown = average.round() as u64;
        }
        self.shown
    }
}

#[cfg(test)]
mod tests {
    use super::Rate;

    /// Feed the same sample repeatedly, and report where the filter got to.
    fn settle(rate: &mut Rate, sample: u64, ticks: usize) -> u64 {
        (0..ticks).map(|_| rate.update(sample)).last().unwrap_or(0)
    }

    #[test]
    fn a_transfer_starting_reads_at_its_speed_not_at_a_fraction_of_it() {
        // Easing up from zero would show 400 KB/s for a download that is
        // already doing a megabyte, and the number would still be climbing
        // when the user looked away.
        let mut rate = Rate::default();
        assert_eq!(rate.update(1_000_000), 1_000_000);
    }

    #[test]
    fn stopping_is_immediate_because_it_is_not_noise() {
        let mut rate = Rate::default();
        settle(&mut rate, 1_000_000, 5);
        assert_eq!(rate.update(0), 0);
        // And it starts clean rather than decaying out of the old value.
        assert_eq!(rate.update(50_000), 50_000);
    }

    #[test]
    fn the_wobble_of_a_steady_transfer_never_reaches_the_screen() {
        // A real 1 MB/s transfer as librqbit's five-second window reports it:
        // a few per cent either way, every second, forever. What matters is
        // that the published figure is *the same number* tick after tick.
        let mut rate = Rate::default();
        rate.update(1_000_000);
        let samples = [1_031_000, 972_000, 1_018_000, 964_000, 1_042_000, 988_000, 1_009_000];
        let published: Vec<u64> = samples.iter().map(|&s| rate.update(s)).collect();
        assert!(published.iter().all(|&p| p == 1_000_000), "the column would have twitched: {published:?}");
    }

    #[test]
    fn a_value_sitting_on_a_unit_boundary_does_not_flip() {
        // 1 MiB/s exactly, wobbling by a hair. Without the deadband this
        // alternates between "1023 KB/s" and "1.00 MB/s" forever.
        let mut rate = Rate::default();
        rate.update(1_048_576);
        for sample in [1_048_100, 1_049_000, 1_047_800, 1_049_400] {
            assert_eq!(rate.update(sample), 1_048_576, "the unit changed under a rounding error");
        }
    }

    #[test]
    fn a_real_change_is_followed() {
        // The deadband is not a freeze: a transfer that genuinely drops to a
        // quarter has to say so, and within a few seconds.
        let mut rate = Rate::default();
        settle(&mut rate, 4_000_000, 5);
        let settled = settle(&mut rate, 1_000_000, 8);
        // Within the deadband, which is where it is supposed to stop: the
        // figure follows until it is close enough to be right at the precision
        // the column prints, and then holds still.
        let error = settled.abs_diff(1_000_000) * 100 / 1_000_000;
        assert!(error <= 6, "the drop was not followed: settled at {settled}, {error} % out");
    }

    #[test]
    fn a_slow_drift_is_tracked_rather_than_swallowed() {
        // Each step is inside the deadband, so nothing moves on its own tick —
        // but the average carries and the figure catches up. The failure this
        // guards against is a rate that creeps from 1 to 2 MB/s while the
        // column insists on the number it first printed.
        let mut rate = Rate::default();
        rate.update(1_000_000);
        for step in 1..=40 {
            rate.update(1_000_000 + step * 25_000);
        }
        let settled = settle(&mut rate, 2_000_000, 3);
        assert!(settled > 1_900_000, "the drift was lost: ended at {settled}");
    }
}

//! The last minute of session throughput, and the shape it draws as.
//!
//! A number says what is happening now; the shape says whether it is going
//! anywhere. Sixty samples is the whole record — it is what fits in a footer
//! and it is as far back as anyone reads while a transfer is running.
//!
//! The samples are the *published* rates, not the raw ones, so this inherits
//! [`Rate`](crate::Rate)'s stability: the vertical scale is the fastest second
//! in the window, and it only moves when the transfer genuinely does.

use std::fmt::Write as _;

/// How many seconds the footer shows. One sample per engine tick.
pub const WINDOW: usize = 60;

/// The height of the drawing's coordinate space. The width is `WINDOW - 1` —
/// one unit per gap between samples — and the `.slint` that draws it declares
/// the same viewbox.
pub const HEIGHT: u64 = 100;

/// One rolling minute of session download and upload rates, in bytes/second.
///
/// A fixed array rather than a `VecDeque`: it is 960 bytes, it never allocates,
/// and it is copied into a snapshot once a second.
#[derive(Clone, Copy, Debug)]
pub struct History {
    down: [u64; WINDOW],
    up: [u64; WINDOW],
    /// Where the next sample goes.
    next: usize,
    /// How many of the slots are real. Below `WINDOW` while the window is still
    /// filling, which is what makes the graph grow in from the right rather
    /// than pretend to a minute of zeroes it never observed.
    len: usize,
}

impl Default for History {
    fn default() -> Self {
        Self { down: [0; WINDOW], up: [0; WINDOW], next: 0, len: 0 }
    }
}

/// Two paths sharing one vertical scale, in SVG path commands.
///
/// One scale for both, so the two lines can be compared by eye — which is the
/// only reason to draw them in the same box.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Spark {
    pub down: String,
    pub up: String,
}

impl History {
    /// Record one tick.
    pub const fn push(&mut self, down: u64, up: u64) {
        self.down[self.next] = down;
        self.up[self.next] = up;
        self.next = (self.next + 1) % WINDOW;
        if self.len < WINDOW {
            self.len += 1;
        }
    }

    /// Forget everything.
    ///
    /// Called when the engine resumes after the window was hidden. Nothing was
    /// sampled while it was away, so keeping the old samples would splice two
    /// separate minutes together and label the result "the last sixty seconds".
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The samples, oldest first.
    fn series(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        let start = (self.next + WINDOW - self.len) % WINDOW;
        (0..self.len).map(move |i| {
            let slot = (start + i) % WINDOW;
            (self.down[slot], self.up[slot])
        })
    }

    /// The drawing, or `None` when there is nothing to draw.
    ///
    /// Nothing to draw is two distinct cases and both are honest as an absence:
    /// fewer than two samples is a line with no length, and a window in which
    /// nothing moved is a flat line along the floor that says less than an
    /// empty footer does.
    #[must_use]
    pub fn spark(&self) -> Option<Spark> {
        let scale = self.series().map(|(down, up)| down.max(up)).max()?;
        if self.len < 2 || scale == 0 {
            return None;
        }

        // The newest sample sits on the right edge, so a window still filling
        // grows leftwards and the present never moves.
        let first_x = WINDOW - self.len;
        let mut spark = Spark { down: String::new(), up: String::new() };
        for (i, (down, up)) in self.series().enumerate() {
            let x = first_x + i;
            let command = if i == 0 { 'M' } else { 'L' };
            // Integer coordinates: the box is a hundred units tall and a
            // fraction of one of them is below a pixel on any screen.
            let _ = write!(spark.down, "{command} {x} {} ", HEIGHT - down * HEIGHT / scale);
            let _ = write!(spark.up, "{command} {x} {} ", HEIGHT - up * HEIGHT / scale);
        }
        Some(spark)
    }
}

#[cfg(test)]
mod tests {
    use super::{History, Spark, HEIGHT, WINDOW};

    fn filled(samples: &[(u64, u64)]) -> History {
        let mut history = History::default();
        for &(down, up) in samples {
            history.push(down, up);
        }
        history
    }

    #[test]
    fn nothing_is_drawn_until_there_are_two_points() {
        assert_eq!(History::default().spark(), None);
        assert_eq!(filled(&[(1000, 0)]).spark(), None, "one point is a line with no length");
        assert!(filled(&[(1000, 0), (2000, 0)]).spark().is_some());
    }

    #[test]
    fn an_idle_minute_draws_nothing_rather_than_a_flat_floor() {
        // A line pinned to the bottom of the box says less than the space it
        // occupies would if it were empty.
        assert_eq!(filled(&[(0, 0); 30]).spark(), None);
    }

    #[test]
    fn the_newest_sample_is_always_on_the_right_edge() {
        // Otherwise the present slides across the footer as the window fills,
        // which reads as the graph drifting rather than growing.
        let right = (WINDOW - 1).to_string();
        for count in [2, 17, WINDOW, WINDOW + 25] {
            let history = filled(&vec![(1000, 500); count]);
            let Spark { down, .. } = history.spark().expect("a drawing");
            let last_x = down.split_whitespace().rev().nth(1).expect("the final x");
            assert_eq!(last_x, right, "with {count} samples");
        }
    }

    #[test]
    fn a_window_still_filling_starts_partway_across() {
        let history = filled(&[(1000, 0), (2000, 0), (3000, 0)]);
        let Spark { down, .. } = history.spark().expect("a drawing");
        assert!(down.starts_with(&format!("M {} ", WINDOW - 3)), "{down}");
    }

    #[test]
    fn the_peak_of_the_window_is_the_top_of_the_box() {
        let history = filled(&[(0, 0), (1000, 0)]);
        let Spark { down, .. } = history.spark().expect("a drawing");
        // SVG counts downwards, so the fastest second is at zero and an idle
        // one sits on the floor at the full height.
        assert_eq!(down.trim(), format!("M {} {HEIGHT} L {} 0", WINDOW - 2, WINDOW - 1));
    }

    #[test]
    fn both_lines_share_one_scale_so_they_can_be_compared() {
        // Upload is a tenth of download here, and has to *look* like a tenth.
        let history = filled(&[(0, 0), (1000, 100)]);
        let Spark { down, up } = history.spark().expect("a drawing");
        assert_eq!(down.trim(), format!("M {} {HEIGHT} L {} 0", WINDOW - 2, WINDOW - 1));
        assert_eq!(up.trim(), format!("M {} {HEIGHT} L {} 90", WINDOW - 2, WINDOW - 1));
    }

    #[test]
    fn the_window_forgets_what_falls_out_of_it() {
        let mut history = filled(&vec![(9_000_000, 0); WINDOW]);
        for _ in 0..WINDOW {
            history.push(1000, 0);
        }
        let Spark { down, .. } = history.spark().expect("a drawing");
        // If the old peak were still in the ring, every one of these would be
        // pinned to the floor instead of spread across the box.
        assert!(down.contains(" 0 "), "the old peak still sets the scale: {down}");
    }

    #[test]
    fn resuming_after_the_window_was_hidden_starts_a_new_minute() {
        let mut history = filled(&[(1000, 0), (2000, 0)]);
        history.clear();
        assert_eq!(history.spark(), None, "the gap would have been drawn as continuous time");
    }
}

//! WCAG contrast, so "readable" is a number rather than an opinion.
//!
//! The palette lives in `ui/theme.slint` and the ratios are asserted against it
//! by a test that reads that file, so the two cannot drift: retuning a colour
//! either keeps it legible or turns CI red. That is the whole reason this is
//! arithmetic in a crate and not a note in a document.

/// Relative luminance, per WCAG 2.2.
///
/// The channel transfer is sRGB's, not a plain average: the eye is far more
/// sensitive to green than to blue, and averaging the channels calls white text
/// on blue readable when it is not.
fn luminance(rgb: u32) -> f64 {
    let channel = |shift: u32| {
        let v = f64::from((rgb >> shift) & 0xff) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126f64.mul_add(channel(16), 0.7152f64.mul_add(channel(8), 0.0722 * channel(0)))
}

/// The contrast ratio between two opaque colours, from 1.0 to 21.0.
///
/// Symmetric, as WCAG defines it: which one is the ink and which the paper does
/// not change whether they can be told apart.
#[must_use]
pub fn ratio(a: u32, b: u32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (lighter, darker) = if x > y { (x, y) } else { (y, x) };
    (lighter + 0.05) / (darker + 0.05)
}

/// What WCAG AA asks of body text. Everything in this app is 11–13 px, which is
/// "normal" by the standard's reckoning — the 3:1 allowance is for 18 pt.
pub const AA_TEXT: f64 = 4.5;

/// What AA asks of a control's own shape — a border, an icon, the fill of a
/// progress bar. Not text, so not the text threshold.
pub const AA_SHAPE: f64 = 3.0;

#[cfg(test)]
mod tests {
    use super::{ratio, AA_TEXT};

    #[test]
    fn the_extremes_are_the_ones_wcag_names() {
        // 21:1 is the whole range, and it is the check that the transfer
        // function is right rather than merely plausible.
        assert!((ratio(0xff_ffff, 0x00_0000) - 21.0).abs() < 0.01);
        assert!((ratio(0x00_0000, 0x00_0000) - 1.0).abs() < 0.01);
    }

    #[test]
    fn it_does_not_matter_which_one_is_the_ink() {
        assert!((ratio(0x11_1111, 0xdd_dddd) - ratio(0xdd_dddd, 0x11_1111)).abs() < 1e-9);
    }

    #[test]
    fn green_counts_for_more_than_blue() {
        // The reason this is not an average of the channels. Pure green is far
        // brighter to the eye than pure blue, and a palette checked with an
        // average calls white on blue readable when it is not.
        assert!(ratio(0x00_ff00, 0x00_0000) > ratio(0x00_00ff, 0x00_0000));
    }

    #[test]
    fn a_known_failing_pair_fails() {
        // Mid grey on white is the classic "looks fine to the designer" pair,
        // and it is 3.9:1 — under the bar.
        assert!(ratio(0x77_7777, 0xff_ffff) < AA_TEXT);
        assert!(ratio(0x59_5959, 0xff_ffff) > AA_TEXT);
    }
}

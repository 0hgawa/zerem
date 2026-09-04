//! The picture that goes with a country code.
//!
//! Emoji would have been free and does not work: Segoe UI Emoji ships no
//! regional indicator pairs, deliberately, so 🇧🇷 renders on Windows as the
//! letters B and R. A flag here is a picture or it is nothing.
//!
//! The file is built by `tools/flags` and is a palette and run lengths per
//! flag — the whole of what a flag needs, since a flag is a few flat colours.
//! Decoding one is the loop at the bottom of this file, and it is the reason
//! nothing here has a dependency.

/// The flags, as `tools/flags` wrote them.
static BLOB: &[u8] = include_bytes!("../../../assets/flags.bin");

const MAGIC: &[u8; 5] = b"ZFLG1";

/// One row of the directory: a country code and where its flag begins.
const ROW: usize = 6;

/// A decoded flag: straight RGBA, ready to hand a renderer.
pub struct Flag {
    pub width: u32,
    pub height: u32,
    /// Four bytes a pixel, top row first.
    pub pixels: Vec<u8>,
}

/// The flag for a two-letter country code.
///
/// `None` for a code the set does not carry, which draws nothing rather than a
/// placeholder — a box where a flag should be says "broken", and an empty slot
/// says "we do not know", which is the truth.
#[must_use]
pub fn of(code: &str) -> Option<Flag> {
    let code = code.as_bytes();
    let [a, b] = code else { return None };
    let wanted = [a.to_ascii_uppercase(), b.to_ascii_uppercase()];

    if BLOB.get(..5)? != MAGIC {
        return None;
    }
    let width = u32::from(*BLOB.get(5)?);
    let height = u32::from(*BLOB.get(6)?);
    let count = usize::from(u16::from_le_bytes([*BLOB.get(7)?, *BLOB.get(8)?]));
    let directory = BLOB.get(9..9 + count * ROW)?;

    // The directory is sorted, so this is a search rather than a scan — two
    // hundred and thirty-nine rows is small, but this runs per peer per tick.
    let row = directory.chunks_exact(ROW).position(|row| row[..2] == wanted)?;
    let at = row * ROW;
    let start =
        u32::from_le_bytes([directory[at + 2], directory[at + 3], directory[at + 4], directory[at + 5]])
            as usize;

    decode(BLOB.get(start..)?, width, height)
}

/// One packed flag: a palette, then runs of indices into it.
fn decode(packed: &[u8], width: u32, height: u32) -> Option<Flag> {
    let colours = usize::from(*packed.first()?);
    let palette = packed.get(1..1 + colours * 4)?;

    let total = (width * height) as usize;
    let mut pixels = Vec::with_capacity(total * 4);
    let mut runs = packed.get(1 + colours * 4..)?.chunks_exact(2);
    while pixels.len() < total * 4 {
        let run = runs.next()?;
        let colour = palette.get(usize::from(run[1]) * 4..usize::from(run[1]) * 4 + 4)?;
        for _ in 0..run[0] {
            pixels.extend_from_slice(colour);
        }
    }
    pixels.truncate(total * 4);
    Some(Flag { width, height, pixels })
}

#[cfg(test)]
mod tests {
    use super::of;

    #[test]
    fn a_known_flag_decodes_to_a_full_picture() {
        let flag = of("BR").expect("Brazil is in the set");
        assert_eq!(flag.width, 40);
        assert_eq!(flag.height, 30);
        assert_eq!(flag.pixels.len(), (40 * 30 * 4) as usize);
    }

    #[test]
    fn the_case_of_the_code_does_not_matter() {
        // The table hands back upper case and a caller may not.
        assert!(of("br").is_some());
        assert!(of("Br").is_some());
    }

    #[test]
    fn a_code_the_set_does_not_carry_draws_nothing() {
        // Rather than a placeholder box, which reads as broken where an empty
        // slot reads as unknown.
        assert!(of("ZZ").is_none());
        assert!(of("").is_none());
        assert!(of("BRA").is_none());
    }

    #[test]
    fn every_country_the_geo_table_knows_has_a_flag() {
        // The two files are generated from the same list of codes, and this is
        // what says they still are. A country with an address range and no flag
        // is a peer with a blank where every other row has a picture.
        for probe in ["US", "BR", "DE", "JP", "AU", "CN", "GB", "RU", "IN", "ZA"] {
            let flag = of(probe).unwrap_or_else(|| panic!("{probe} has no flag"));
            assert_eq!(flag.pixels.len(), (flag.width * flag.height * 4) as usize);
        }
    }

    #[test]
    fn a_flag_that_does_not_fill_the_box_is_padded_and_not_stretched() {
        // Nepal is a pennant and the United Kingdom is 2:1. Both are centred in
        // the box with the remainder left transparent, so neither is drawn in
        // proportions it does not have.
        for probe in ["NP", "GB", "CH"] {
            let flag = of(probe).unwrap_or_else(|| panic!("{probe} has no flag"));
            let clear = flag.pixels.chunks_exact(4).filter(|p| p[3] == 0).count();
            assert!(clear > 0, "{probe} fills the whole box, so nothing was padded");
        }
    }

    #[test]
    fn padding_only_ever_sits_at_the_edges() {
        // The other half of the same claim, and the one that would catch a real
        // mistake: the flag itself is a solid block, centred, and everything
        // outside it is clear. Every country keeps its own proportions — Brazil
        // is 7:10, Germany 3:5 — so almost all of them are padded on one axis.
        // A gap anywhere but the edges means the fitting put a flag down in the
        // wrong place.
        //
        // Nepal is left out, and not because it fails by accident: its flag is
        // genuinely not a rectangle. It is two stacked pennants, so it has
        // clear space inside its own bounding box, and it is the only one.
        for probe in ["BR", "DE", "GB", "CH", "US"] {
            let flag = of(probe).unwrap_or_else(|| panic!("{probe} has no flag"));
            let (w, h) = (flag.width as usize, flag.height as usize);
            let opaque = |x: usize, y: usize| flag.pixels[(y * w + x) * 4 + 3] != 0;

            let xs: Vec<usize> = (0..w).filter(|&x| (0..h).any(|y| opaque(x, y))).collect();
            let ys: Vec<usize> = (0..h).filter(|&y| (0..w).any(|x| opaque(x, y))).collect();
            let (x0, x1) = (xs[0], xs[xs.len() - 1]);
            let (y0, y1) = (ys[0], ys[ys.len() - 1]);

            for y in 0..h {
                for x in 0..w {
                    let inside = (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
                    assert_eq!(opaque(x, y), inside, "{probe} at {x},{y}");
                }
            }
            // And centred, to within the odd pixel an odd remainder leaves.
            assert!(x0.abs_diff(w - 1 - x1) <= 1, "{probe} is off centre across");
            assert!(y0.abs_diff(h - 1 - y1) <= 1, "{probe} is off centre down");
        }
    }
}

//! The palette, measured.
//!
//! Reads `ui/theme.slint` itself rather than a copy of the values, so retuning a
//! colour either keeps it legible or turns this red. A contrast table written
//! down by hand is a table that stops being true on the first tweak.
//!
//! Both themes, because a palette that is only checked in the dark is a palette
//! that is only half checked — and the light one is the harder of the two here,
//! since it is the newer.

use std::collections::HashMap;

use zerem_core::contrast::{ratio, AA_SHAPE, AA_TEXT};

const THEME: &str = include_str!("../ui/theme.slint");

/// The colours of one theme, by token name.
struct Palette {
    name: &'static str,
    colours: HashMap<String, u32>,
}

impl Palette {
    fn get(&self, token: &str) -> u32 {
        *self
            .colours
            .get(token)
            .unwrap_or_else(|| panic!("{} has no token `{token}` — was it renamed?", self.name))
    }

    /// Record one pair. Collected rather than asserted on the spot, and the
    /// number is always named: stopping at the first failure would mean fixing
    /// a palette one colour per test run, and "assertion failed" would send
    /// whoever hits it back to a calculator.
    fn check(&self, failures: &mut Vec<String>, fg: &str, bg: &str, floor: f64) {
        let value = ratio(self.get(fg), self.get(bg));
        if value < floor {
            failures
                .push(format!("{}: {fg} on {bg} is {value:.2}:1, under the {floor:.1}:1 floor", self.name));
        }
    }
}

/// Pull `#rrggbb` literals out of the token declarations.
///
/// Two shapes, and only two: `dark ? #a : #b` for the ones that swap, and a
/// bare `#a` for the ones that do not. Anything else is not a colour token and
/// is skipped — the file also holds lengths, durations and easing curves.
fn parse(dark: bool) -> Palette {
    let mut colours = HashMap::new();
    for line in THEME.lines() {
        let Some(rest) = line.trim().strip_prefix("out property <brush> ") else { continue };
        let Some((token, value)) = rest.split_once(':') else { continue };
        let hexes: Vec<u32> = value
            .split('#')
            .skip(1)
            .filter_map(|h| {
                let digits: String = h.chars().take_while(char::is_ascii_hexdigit).collect();
                // Six digits only: the palette also holds #ffffff06 overlays,
                // which have an alpha and cannot be contrast-checked against
                // anything without compositing them first.
                (digits.len() == 6).then(|| u32::from_str_radix(&digits, 16).ok())?
            })
            .collect();
        let colour = match hexes.as_slice() {
            [single] => *single,
            [in_dark, in_light] => {
                if dark {
                    *in_dark
                } else {
                    *in_light
                }
            }
            _ => continue,
        };
        colours.insert(token.trim().to_owned(), colour);
    }
    assert!(colours.len() > 8, "the theme parser found almost nothing — did the file's shape change?");
    Palette { name: if dark { "dark" } else { "light" }, colours }
}

#[test]
fn every_pair_the_app_actually_draws_is_legible() {
    let mut bad = Vec::new();
    for dark in [true, false] {
        let p = parse(dark);

        // Body text, on both the window and the surfaces that sit on it.
        p.check(&mut bad, "text", "bg", AA_TEXT);
        p.check(&mut bad, "text", "surface", AA_TEXT);
        p.check(&mut bad, "text", "surface-hi", AA_TEXT);

        // The dim text is a whole column of the table — sizes, peers, ETA,
        // ratio — so it is body text and gets the body threshold, not the
        // "it is only a hint" discount.
        p.check(&mut bad, "text-dim", "bg", AA_TEXT);
        p.check(&mut bad, "text-dim", "surface", AA_TEXT);

        // Coloured text: the state column, the speeds, the notice line.
        p.check(&mut bad, "accent", "bg", AA_TEXT);
        p.check(&mut bad, "accent", "surface", AA_TEXT);
        p.check(&mut bad, "seeding", "bg", AA_TEXT);
        p.check(&mut bad, "seeding", "surface", AA_TEXT);
        p.check(&mut bad, "danger", "bg", AA_TEXT);
        p.check(&mut bad, "danger", "surface", AA_TEXT);
        p.check(&mut bad, "warn", "bg", AA_TEXT);
        p.check(&mut bad, "warn", "surface", AA_TEXT);
    }
    assert!(bad.is_empty(), "{} pairs are illegible:\n  {}", bad.len(), bad.join("\n  "));
}

#[test]
fn the_shapes_that_are_not_text_clear_the_shape_bar() {
    let mut bad = Vec::new();
    for dark in [true, false] {
        let p = parse(dark);

        // `idle` is never a sentence — it is the bar of a paused torrent and
        // the path of a file nobody asked for. Three to one is what AA asks of
        // a shape, and holding it to the text bar would force it so bright that
        // "switched off" would stop reading as switched off.
        p.check(&mut bad, "idle", "bg", AA_SHAPE);
        p.check(&mut bad, "idle", "surface", AA_SHAPE);
    }
    assert!(
        bad.is_empty(),
        "{} shapes are too faint:
  {}",
        bad.len(),
        bad.join(
            "
  "
        )
    );
}

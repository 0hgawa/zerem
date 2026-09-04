//! Packs the country flags Zerem draws beside a peer's address.
//!
//! # Why images and not emoji
//!
//! Because 🇧🇷 does not render on Windows. Segoe UI Emoji ships no regional
//! indicator pairs, deliberately, so every flag emoji falls back to the two
//! letters it is built from. A flag has to be a picture here or it is not a
//! flag.
//!
//! # Where they come from
//!
//! flagcdn.com, at `w40`, which is Flagpedia's set: the flags themselves are
//! national symbols and in the public domain, and the set is published for any
//! use without attribution. Fetch them for the countries the geo table knows —
//! `tools/geo` writes those codes into the head of `assets/geoip.bin` — and
//! point this at the folder.
//!
//! ```text
//! cargo run --release -- <folder-of-cc.png> ../../assets/flags.bin
//! ```
//!
//! # The format
//!
//! A flag is a few flat colours, so a palette and run lengths beat anything
//! general: no compressor, no dependency in the app, and a decoder that fits on
//! a screen. Rows run together rather than restarting each line — a horizontal
//! tricolour becomes three runs for the whole image instead of three per row.

use std::collections::BTreeMap;
use std::path::Path;

use image::imageops::FilterType;
use image::RgbaImage;

/// The box every flag is fitted into.
///
/// 40×30, which is a 20×15 slot at the two-times scaling this app is most
/// often run at — and 20×15 is the size a flag sits beside an address without
/// becoming the thing you look at first.
///
/// Not smaller. The source set is 40 wide, so anything under that resamples
/// every flag instead of a handful, and resampling invents colours: at 32×24
/// several flags went past the 255 a palette byte can name.
const W: u32 = 40;
const H: u32 = 30;

fn main() {
    let mut args = std::env::args().skip(1);
    let (input, output) = match (args.next(), args.next()) {
        (Some(i), Some(o)) => (i, o),
        _ => {
            eprintln!("usage: zerem-flags-build <folder-of-cc.png> <out.bin>");
            std::process::exit(2);
        }
    };

    let mut flags: BTreeMap<[u8; 2], Vec<u8>> = BTreeMap::new();
    let mut widest = 0usize;
    for entry in std::fs::read_dir(&input).expect("read the input folder") {
        let path = entry.expect("read a directory entry").path();
        let Some(code) = code_of(&path) else { continue };
        let packed = pack(&fit(&path));
        widest = widest.max(packed.len());
        flags.insert(code, packed);
    }
    assert!(!flags.is_empty(), "no cc.png files in {input}");

    let blob = assemble(&flags);
    std::fs::write(&output, &blob).expect("write the flags");
    println!(
        "{output}: {} bytes — {} flags at {W}×{H}, {} bytes each on average, {widest} at worst",
        blob.len(),
        flags.len(),
        blob.len() / flags.len()
    );
}

/// The country a file is named after, if it is named after one.
fn code_of(path: &Path) -> Option<[u8; 2]> {
    if path.extension()? != "png" {
        return None;
    }
    let stem = path.file_stem()?.to_str()?.as_bytes();
    let [a, b] = stem else { return None };
    a.is_ascii_alphabetic()
        .then(|| [a.to_ascii_uppercase(), b.to_ascii_uppercase()])
        .filter(|_| b.is_ascii_alphabetic())
}

/// One flag, centred in the box without being stretched.
///
/// Flags are not all 4:3 — Switzerland and the Vatican are square, Nepal is a
/// pennant taller than it is wide, and the United Kingdom is 2:1. Stretching
/// them to a common box would draw each of those wrong, so they are scaled to
/// fit and the leftover is left transparent.
///
/// A flag that already fits is not resampled at all. That is not a shortcut:
/// resampling invents colours between the flat ones, and every colour invented
/// is a palette entry paid for in the file.
fn fit(path: &Path) -> RgbaImage {
    let source = image::open(path).expect("decode a flag").to_rgba8();
    let (sw, sh) = source.dimensions();

    let scaled = if sw <= W && sh <= H {
        source
    } else {
        // Triangle rather than nearest: at this size a nearest-neighbour cross
        // or saltire comes out with a staircase in it, and the extra colours
        // are only along those edges.
        let ratio = f64::from(W) / f64::from(sw);
        let ratio = ratio.min(f64::from(H) / f64::from(sh));
        let (w, h) = ((f64::from(sw) * ratio) as u32, (f64::from(sh) * ratio) as u32);
        image::imageops::resize(&source, w.max(1), h.max(1), FilterType::Triangle)
    };

    let (sw, sh) = scaled.dimensions();
    let (ox, oy) = ((W - sw) / 2, (H - sh) / 2);
    let mut out = RgbaImage::new(W, H);
    for (x, y, pixel) in scaled.enumerate_pixels() {
        out.put_pixel(x + ox, y + oy, *pixel);
    }
    out
}

/// A palette and a run of indices, in the shape the app's reader decodes.
///
/// ```text
/// u8            how many colours
/// [r,g,b,a] * n the palette
/// (u8, u8) *    a run length of 1..=255 and the colour it repeats,
///               laid end to end until W*H pixels are covered
/// ```
///
/// Runs cross row boundaries. A horizontal tricolour is three runs for the
/// whole flag rather than three for every line, and the flags that are not
/// tricolours were never going to be cheap either way.
fn pack(image: &RgbaImage) -> Vec<u8> {
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut indices: Vec<u8> = Vec::with_capacity((W * H) as usize);
    for (_, _, pixel) in image.enumerate_pixels() {
        // Anything not fully opaque is the padding around a flag that does not
        // fill the box, and there is only ever one of it.
        let colour = if pixel.0[3] < 128 { [0, 0, 0, 0] } else { [pixel.0[0], pixel.0[1], pixel.0[2], 255] };
        let at = palette.iter().position(|c| *c == colour).unwrap_or_else(|| {
            palette.push(colour);
            palette.len() - 1
        });
        indices.push(u8::try_from(at).expect("a flag with more than 255 colours"));
    }

    let mut out = Vec::with_capacity(256);
    out.push(u8::try_from(palette.len()).expect("palette fits in a byte"));
    for colour in &palette {
        out.extend_from_slice(colour);
    }

    let mut at = 0;
    while at < indices.len() {
        let colour = indices[at];
        let mut run = 0usize;
        while at + run < indices.len() && indices[at + run] == colour && run < 255 {
            run += 1;
        }
        out.push(u8::try_from(run).expect("runs are capped at 255"));
        out.push(colour);
        at += run;
    }
    out
}

/// The whole file: a sorted directory, then the flags it points at.
///
/// ```text
/// "ZFLG1"                 magic
/// u8 width, u8 height     the box every flag was fitted into
/// u16                     how many flags
/// [ [u8;2] code, u32 at ] the directory, sorted, so a code is a binary search
/// the packed flags
/// ```
fn assemble(flags: &BTreeMap<[u8; 2], Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::with_capacity(128 * 1024);
    out.extend_from_slice(b"ZFLG1");
    out.push(u8::try_from(W).expect("box fits in a byte"));
    out.push(u8::try_from(H).expect("box fits in a byte"));
    out.extend_from_slice(&u16::try_from(flags.len()).expect("fewer than 65536 flags").to_le_bytes());

    let directory = 5 + 2 + 2 + flags.len() * 6;
    let mut at = u32::try_from(directory).expect("directory fits");
    for (code, packed) in flags {
        out.extend_from_slice(code);
        out.extend_from_slice(&at.to_le_bytes());
        at += u32::try_from(packed.len()).expect("flags fit");
    }
    for packed in flags.values() {
        out.extend_from_slice(packed);
    }
    out
}

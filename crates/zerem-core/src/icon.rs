//! The app mark, drawn rather than shipped as a file.
//!
//! One source for both places it appears: the tray rasterises it at 32 px at
//! runtime, and the build script writes the same shape into the `.ico` that
//! becomes the executable's icon. A committed binary asset would be a second
//! copy of this that nothing keeps in step.
//!
//! Pure maths into a `Vec<u8>`, which is why it lives in core.
//!
//! # Transcribed, not embedded
//!
//! The mark is a Fluent cloud, and it arrived as an SVG. Rasterising that at
//! run time would want an SVG engine in a crate whose dependency list is empty
//! on purpose, and baking it at build time would mean choosing one pixel size
//! in advance for something asked for at seven of them. Neither is necessary:
//! the whole drawing is two shapes and five gradients, and every one of those
//! is an expression. The numbers below are the SVG's own, in its own sixteen
//! units, so a change to the artwork is a change to the same figures.

/// How much bigger the artwork is drawn than the SVG asks for.
///
/// The source is a sixteen-pixel *interface* icon: it carries the margin that
/// keeps it in line with text on a toolbar. An application icon is not in line
/// with anything — it sits in a taskbar beside other application icons, which
/// fill their canvas — and a mark left fourteen units wide inside sixteen reads
/// as a smaller app rather than a tidier one.
///
/// Fourteen units to sixteen is 1.143, and this stops a hair short of it: the
/// edge is a curve, and a curve needs half a pixel either side of itself to be
/// drawn as one rather than as a staircase.
const FIT: f32 = 1.13;

/// Where the artwork's own middle is.
///
/// The cloud spans two to twelve vertically, so its middle is at seven where
/// the canvas's is at eight. Left alone it hangs a unit high, which in a row of
/// taskbar icons is the sort of thing nobody names and everybody sees.
const ART_MIDDLE_Y: f32 = 7.0;

/// The body: a capsule with a flat foot, from the two `A3.25` arcs and the line
/// between them. Its ends, the line they sit on, then the radius.
const BODY: [f32; 4] = [4.25, 11.75, 8.75, 3.25];

/// The head: the `a4 4` arc across the top. Its middle is half a unit below
/// where that arc begins and ends, which is what a four-unit radius over a
/// 7.94-unit chord works out to.
const HEAD: [f32; 3] = [8.0, 5.996, 4.0];

/// The fill, and the two ends of the line it runs along.
const SKY: [f32; 3] = [15.0, 175.0, 255.0]; // #0fafff
const DEEP: [f32; 3] = [54.0, 122.0, 242.0]; // #367af2
const FILL_LINE: [f32; 4] = [1.5, 3.875, 7.948, 13.254];

/// The light on the near lobe, and the light down the head. Both run from white
/// to nothing, and both go on at three tenths.
const LOBE_LIGHT: [f32; 4] = [1.0, 6.613, 5.382, 10.492];
const HEAD_LIGHT: [f32; 4] = [5.412, 2.45, 6.47, 7.965];
const LIGHT_STRENGTH: f32 = 0.3;

/// The glow inside the head: solid to just past four tenths of the way out,
/// then gone by the rim.
const GLOW: [f32; 3] = [44.0, 135.0, 245.0]; // #2c87f5
const GLOW_SOLID_TO: f32 = 0.412;

/// The wash over the whole mark: nothing until halfway out, then magenta, at a
/// half. It is what keeps the thing from being one flat blue, and it is why the
/// far corner is warmer than the near one.
const WASH: [f32; 3] = [221.0, 60.0, 226.0]; // #dd3ce2
const WASH_STARTS_AT: f32 = 0.5;
const WASH_STRENGTH: f32 = 0.5;

/// The arrow, which is not the SVG's — a cloud on its own is what every storage
/// service uses, and this is a torrent client. The arrow is what makes it say
/// what the app does rather than where the file came from.
///
/// Where it goes is decided by the shape it sits on. Centred left to right, and
/// low rather than middling: the cloud carries most of its weight along the
/// body, and an arrow centred in the *canvas* would sit half in the head, where
/// there is less to sit on. Top, then where the head widens out, then the point.
const ARROW: [f32; 3] = [5.4, 8.5, 10.9];

/// Half the stem, and half the head where it is widest.
///
/// The stem is what decides whether this reads at sixteen pixels: 0.85 either
/// side is very close to two whole pixels there, and one pixel of white on blue
/// is a scratch rather than a stroke.
const STEM: f32 = 0.85;
const BARB: f32 = 2.3;

/// The two radial gradients, as the inverse of the transform that places each.
///
/// A radial gradient here is drawn on the unit circle at the origin and then
/// carried into the artwork by an affine transform, so "how far out is this
/// pixel" is asked by taking the pixel *back* through that transform and
/// measuring where it lands. Four numbers for the inverted linear part, then
/// the two the forward transform shifts by.
const GLOW_PLACE: [f32; 6] = [0.188_828, -0.079_864, 0.089_341, 0.211_235, 4.342, 8.55];
const WASH_PLACE: [f32; 6] = [0.035_506, 0.072_966, -0.010_232, 0.004_979, 7.418_17, 1.376_2];

/// The mark, at whatever size is asked for.
///
/// Straight RGBA, row-major from the top — the layout every raster consumer
/// wants and the one the ICO writer converts from.
#[must_use]
pub fn rgba(size: u32) -> Vec<u8> {
    // Pixels to one artwork unit, which is what turns a distance into a
    // coverage and so does every bit of the antialiasing below.
    let per_unit = size as f32 / 16.0 * FIT;
    let mut out = vec![0_u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let (ux, uy) = place(x, y, size);
            let cover = coverage(cloud(ux, uy), per_unit);
            if cover <= 0.0 {
                continue;
            }
            let [r, g, b] = shade(ux, uy, per_unit);
            // The arrow last and over everything, because it is the part that
            // has to be legible at sixteen pixels and nothing may tint it.
            let [r, g, b] = mix([r, g, b], [255.0; 3], coverage(arrow(ux, uy), per_unit));
            let i = ((y * size + x) * 4) as usize;
            out[i] = r as u8;
            out[i + 1] = g as u8;
            out[i + 2] = b as u8;
            out[i + 3] = (cover * 255.0) as u8;
        }
    }
    out
}

/// A pixel's middle, in the artwork's own sixteen units.
fn place(x: u32, y: u32, size: u32) -> (f32, f32) {
    let n = size as f32 / 16.0;
    let (cx, cy) = ((x as f32 + 0.5) / n, (y as f32 + 0.5) / n);
    ((cx - 8.0) / FIT + 8.0, (cy - 8.0) / FIT + ART_MIDDLE_Y)
}

/// How far outside the cloud a point is, in artwork units — negative inside.
///
/// The outline is the union of the two shapes, which for distances is whichever
/// is nearer. That is all of it: everything else in the drawing is colour.
fn cloud(x: f32, y: f32) -> f32 {
    body(x, y).min(head(x, y))
}

fn body(x: f32, y: f32) -> f32 {
    let [left, right, line, radius] = BODY;
    (x - x.clamp(left, right)).hypot(y - line) - radius
}

fn head(x: f32, y: f32) -> f32 {
    let [cx, cy, radius] = HEAD;
    (x - cx).hypot(y - cy) - radius
}

/// How far outside the arrow a point is — negative inside.
///
/// A stem and a head, and whichever is nearer, the same way the cloud is put
/// together. Both are given as distances rather than as a yes or no so the
/// edges come out smooth: a white shape this small with hard edges is the one
/// thing in the drawing that would look drawn by hand.
fn arrow(x: f32, y: f32) -> f32 {
    let [top, shoulder, tip] = ARROW;
    stem(x, y, top, shoulder).min(head_of_arrow(x, y, shoulder, tip))
}

/// The upright, as a box.
fn stem(x: f32, y: f32, top: f32, shoulder: f32) -> f32 {
    let half = (shoulder - top) / 2.0;
    let (dx, dy) = ((x - 8.0).abs() - STEM, (y - (top + half)).abs() - half);
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0)
}

/// The point, as a triangle: three edges, and outside is whichever it is
/// furthest past.
fn head_of_arrow(x: f32, y: f32, shoulder: f32, tip: f32) -> f32 {
    let (left, right, point) = ((8.0 - BARB, shoulder), (8.0 + BARB, shoulder), (8.0, tip));
    let past = |a: (f32, f32), b: (f32, f32)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        -dy.mul_add(-(x - a.0), dx * (y - a.1)) / dx.hypot(dy)
    };
    past(left, right).max(past(right, point)).max(past(point, left))
}

/// A distance turned into how much of a pixel is covered.
///
/// Half a pixel of feather either side of the edge, which is what makes a curve
/// a curve at sixteen pixels rather than a staircase.
fn coverage(distance: f32, per_unit: f32) -> f32 {
    distance.mul_add(-per_unit, 0.5).clamp(0.0, 1.0)
}

/// The colour at a point inside the cloud.
///
/// Laid on in the order the SVG paints them, because each is over what came
/// before it and that order is the picture.
fn shade(x: f32, y: f32, per_unit: f32) -> [f32; 3] {
    let [ax, ay, bx, by] = FILL_LINE;
    let mut colour = mix(SKY, DEEP, along(x, y, ax, ay, bx, by));

    // The near lobe catches the light. Its edge is faded rather than cut: the
    // gradient has run out by the time it gets there, so the seam is invisible
    // — but a hard mask would put a staircase where the fade is.
    let [left, _, line, radius] = BODY;
    let lobe = coverage((x - left).hypot(y - line) - radius, per_unit);
    let [ax, ay, bx, by] = LOBE_LIGHT;
    colour = over(colour, [255.0; 3], LIGHT_STRENGTH * (1.0 - along(x, y, ax, ay, bx, by)) * lobe);

    // The head, twice: white down its front, and a brighter blue through the
    // middle of it. The second is what draws the seam between head and body,
    // and that seam is what stops the mark reading as one flat blob.
    let inside_head = coverage(head(x, y), per_unit);
    let [ax, ay, bx, by] = HEAD_LIGHT;
    colour = over(colour, [255.0; 3], LIGHT_STRENGTH * (1.0 - along(x, y, ax, ay, bx, by)) * inside_head);
    let reach = ((1.0 - radial(x, y, GLOW_PLACE)) / (1.0 - GLOW_SOLID_TO)).clamp(0.0, 1.0);
    colour = over(colour, GLOW, reach * inside_head);

    // And the magenta over all of it.
    let out = (radial(x, y, WASH_PLACE) - WASH_STARTS_AT) / (1.0 - WASH_STARTS_AT);
    over(colour, WASH, WASH_STRENGTH * out.clamp(0.0, 1.0))
}

/// How far along a line a point has got: nothing at one end, one at the other,
/// clamped so a gradient never runs past its own stops.
fn along(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    ((y - ay).mul_add(dy, (x - ax) * dx) / dy.mul_add(dy, dx * dx)).clamp(0.0, 1.0)
}

/// How far out a point is on a radial gradient, once it has been carried back
/// through the transform that placed the gradient.
fn radial(x: f32, y: f32, place: [f32; 6]) -> f32 {
    let [a, b, c, d, tx, ty] = place;
    let (qx, qy) = (x - tx, y - ty);
    a.mul_add(qx, b * qy).hypot(c.mul_add(qx, d * qy))
}

fn mix(from: [f32; 3], to: [f32; 3], t: f32) -> [f32; 3] {
    [
        (to[0] - from[0]).mul_add(t, from[0]),
        (to[1] - from[1]).mul_add(t, from[1]),
        (to[2] - from[2]).mul_add(t, from[2]),
    ]
}

/// One colour over another at `alpha`. Both are opaque, so this is the mix and
/// nothing more — the shape's own transparency is carried separately, in the
/// coverage, and folding the two together here would darken every edge.
fn over(under: [f32; 3], on: [f32; 3], alpha: f32) -> [f32; 3] {
    mix(under, on, alpha.clamp(0.0, 1.0))
}

/// A Windows `.ico` holding these sizes.
///
/// Uncompressed BMP frames rather than PNG ones: PNG would mean an encoder as a
/// build dependency, and at these sizes the whole file is a few tens of KiB
/// either way. Both forms are valid ICO, and every Windows version since XP
/// reads BMP frames.
#[must_use]
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let frames: Vec<Vec<u8>> = sizes.iter().map(|&s| dib(s)).collect();

    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0]); // reserved
    out.extend_from_slice(&[1, 0]); // type: icon
    out.extend_from_slice(&u16::try_from(sizes.len()).unwrap_or(u16::MAX).to_le_bytes());

    // The directory comes first, so every frame's offset counts from past it.
    let mut offset = 6 + 16 * frames.len();
    for (&size, frame) in sizes.iter().zip(&frames) {
        // 0 means 256 in this field, which is why it is a single byte.
        let dim = u8::try_from(size).unwrap_or(0);
        out.extend_from_slice(&[dim, dim, 0, 0]); // width, height, palette, reserved
        out.extend_from_slice(&1_u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32_u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&u32::try_from(frame.len()).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&u32::try_from(offset).unwrap_or(0).to_le_bytes());
        offset += frame.len();
    }
    for frame in frames {
        out.extend_from_slice(&frame);
    }
    out
}

/// One frame: a `BITMAPINFOHEADER`, BGRA bottom-up, then the AND mask.
///
/// The mask is unused for 32-bit frames but the format still requires it, and a
/// missing one is where hand-rolled ICO writers usually go wrong.
fn dib(size: u32) -> Vec<u8> {
    let rgba = rgba(size);
    let mut out = Vec::with_capacity(40 + (size * size * 4) as usize);

    out.extend_from_slice(&40_u32.to_le_bytes()); // header size
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&(size * 2).to_le_bytes()); // colour + mask, as ICO wants
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&32_u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]); // compression, sizes, resolutions, palette

    // Bottom-up, and BGRA rather than RGBA.
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }

    // AND mask: one bit per pixel, rows padded to four bytes. All zero, because
    // the alpha channel already carries the shape.
    let row = (size.div_ceil(32) * 4) as usize;
    out.extend(std::iter::repeat_n(0_u8, row * size as usize));
    out
}

#[cfg(test)]
mod tests {
    use super::{arrow, cloud, ico, place, rgba, ARROW, ART_MIDDLE_Y};

    /// One pixel of a `size`-square rendering, as RGBA.
    fn pixel(px: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    #[test]
    fn the_corners_are_empty_and_the_middle_is_not() {
        let px = rgba(32);
        assert_eq!(pixel(&px, 32, 0, 0)[3], 0, "the very corner is outside the cloud");
        assert_eq!(pixel(&px, 32, 31, 0)[3], 0);
        assert_eq!(pixel(&px, 32, 16, 16)[3], 255, "the middle is solid");
    }

    #[test]
    fn it_is_a_cloud_and_not_a_circle() {
        // The two things that make the outline: a head standing above the body,
        // and shoulders the head does not reach. Asked of the distance itself,
        // so it is the shape under test rather than the colour over it.
        assert!(cloud(8.0, 2.5) < 0.0, "the head stands above the body");
        assert!(cloud(1.5, 4.0) > 0.0, "and nothing stands up there beside it");
        assert!(cloud(1.5, 8.75) < 0.0, "the body reaches out past the head");
        assert!(cloud(14.5, 8.75) < 0.0);
    }

    #[test]
    fn the_foot_is_flat() {
        // A cloud sitting on a line, not a blob. Just inside the foot and just
        // below it, across the whole width.
        for x in [5.0_f32, 8.0, 11.0] {
            assert!(cloud(x, 11.8) < 0.0, "inside the foot at {x}");
            assert!(cloud(x, 12.3) > 0.0, "below the foot at {x}");
        }
    }

    #[test]
    fn the_artwork_sits_in_the_middle_of_the_canvas() {
        // The source is an interface icon and hangs a unit high in its own box.
        // Left there it would sit above the line of every other icon in a
        // taskbar — the one thing here that is deliberately not the SVG's.
        const SIZE: u32 = 64;
        let px = rgba(SIZE);
        let drawn = |y: u32| (0..SIZE).any(|x| pixel(&px, SIZE, x, y)[3] > 0);
        let top = (0..SIZE).find(|&y| drawn(y)).expect("something is drawn");
        let bottom = (0..SIZE).rev().find(|&y| drawn(y)).expect("something is drawn");
        let below = SIZE - 1 - bottom;
        assert!(top.abs_diff(below) <= 2, "{top} above and {below} below");
    }

    #[test]
    fn the_arrow_points_down() {
        // Wide where the head is, a point at the end. That asymmetry is the
        // whole of what makes it an arrow rather than a cross, and getting the
        // taper backwards would draw one pointing up -- an upload client.
        let [_, shoulder, tip] = ARROW;
        let across = |y: f32| (0..320_u16).filter(|&n| arrow(f32::from(n) / 20.0, y) < 0.0).count();
        let wide = across(shoulder + 0.1);
        let narrow = across(tip - 0.3);
        assert!(wide > narrow * 3, "{wide} across the head against {narrow} near the point");
    }

    #[test]
    fn the_arrow_never_hangs_off_the_cloud() {
        // It is drawn over the mark and takes the mark's own transparency, so
        // any part of it outside the shape is simply not drawn -- an arrow that
        // reached past the foot would come out cut off rather than wrong, which
        // is the kind of thing that survives a glance and ships.
        // A grid over the whole of where the arrow can be, with room to spare
        // on every side of it.
        let mut checked = 0;
        for row in 0..120_u16 {
            for col in 0..120_u16 {
                let x = f32::from(col).mul_add(0.075, 4.0);
                let y = f32::from(row).mul_add(0.06, 4.5);
                if arrow(x, y) < 0.0 {
                    assert!(cloud(x, y) < 0.0, "the arrow is outside the cloud at {x}, {y}");
                    checked += 1;
                }
            }
        }
        // The grid has to have found the arrow at all, or this passes by
        // testing nothing -- which is how it passed the first time I ran it.
        assert!(checked > 2000, "only {checked} points of arrow were tested");
    }

    #[test]
    fn the_arrow_is_centred() {
        // On the mark's middle, which is also the canvas's. Off by even a
        // little and it reads as a mistake at every size above the taskbar.
        let [top, shoulder, tip] = ARROW;
        for y in [top + 0.2, shoulder - 0.1, shoulder + 0.5, tip - 0.4] {
            for dx in [0.3_f32, 0.9, 1.6] {
                let (left, right) = (arrow(8.0 - dx, y), arrow(8.0 + dx, y));
                assert!((left - right).abs() < 0.001, "at {y}, {dx} out: {left} against {right}");
            }
        }
    }

    #[test]
    fn the_light_comes_from_the_near_corner() {
        // The fill runs from a bright sky blue down to a deeper one, so the far
        // end of that line has to be the darker. A gradient transcribed with
        // its ends swapped would still draw a perfectly good cloud.
        let px = rgba(64);
        let near = pixel(&px, 64, 20, 20);
        let far = pixel(&px, 64, 40, 44);
        assert!(near[2] > far[2], "the near corner is the bluer: {near:?} against {far:?}");
    }

    #[test]
    fn the_far_corner_is_the_warm_one() {
        // The magenta starts halfway out and only ever reaches one corner. It
        // is the whole reason the mark is not one flat blue, and a transform
        // inverted the wrong way would put it in the other corner or nowhere.
        let px = rgba(64);
        let cool = pixel(&px, 64, 18, 26);
        let warm = pixel(&px, 64, 46, 42);
        assert!(warm[0] > cool[0] + 20, "the far corner is redder: {warm:?} against {cool:?}");
    }

    #[test]
    fn a_pixel_lands_where_the_artwork_says() {
        // The one place a size becomes artwork units. Wrong here and every
        // number above is measuring the wrong point.
        //
        // Asked as a symmetry rather than as one pixel, because on an even grid
        // there is no pixel *at* the middle — 16 of 32 is the first of the
        // right half, and its own centre is a quarter unit past. The two either
        // side of the line have to average out to it.
        let (left, above) = place(15, 15, 32);
        let (right, below) = place(16, 16, 32);
        assert!((f32::midpoint(left, right) - 8.0).abs() < 0.001, "{left} and {right} straddle the middle");
        assert!(
            (f32::midpoint(above, below) - ART_MIDDLE_Y).abs() < 0.001,
            "{above} and {below} straddle the artwork's"
        );
    }

    #[test]
    fn the_ico_header_describes_what_follows() {
        let file = ico(&[16, 32, 48, 256]);
        assert_eq!(&file[0..4], &[0, 0, 1, 0], "reserved, then type 1 = icon");
        assert_eq!(u16::from_le_bytes([file[4], file[5]]), 4, "four frames");

        // Every directory entry has to point inside the file, or Windows shows
        // nothing and says nothing.
        for i in 0..4 {
            let entry = 6 + i * 16;
            let len = u32::from_le_bytes(file[entry + 8..entry + 12].try_into().expect("4 bytes"));
            let at = u32::from_le_bytes(file[entry + 12..entry + 16].try_into().expect("4 bytes"));
            assert!(at as usize + len as usize <= file.len(), "frame {i} runs past the end");
        }
    }

    #[test]
    fn a_256_pixel_frame_is_recorded_as_zero() {
        // The width and height fields are one byte, so 256 is written as 0.
        let file = ico(&[256]);
        assert_eq!(file[6], 0);
        assert_eq!(file[7], 0);
    }
}

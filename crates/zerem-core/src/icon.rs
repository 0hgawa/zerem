//! The app mark, drawn rather than shipped as a file.
//!
//! One source for both places it appears: the tray rasterises it at 32 px at
//! runtime, and the build script writes the same shape into the `.ico` that
//! becomes the executable's icon. A committed binary asset would be a second
//! copy of this that nothing keeps in step.
//!
//! Pure maths into a `Vec<u8>`, which is why it lives in core.
//!
//! # A tile with a glyph on it, not a glyph
//!
//! This used to *be* the shape — a cloud, with the gradients inside it and
//! nothing around it. It is now what every other application on this desktop
//! is: a rounded square carrying the colour, with a white mark on top. The two
//! are not variations of each other. The first has no ground and the second is
//! mostly ground, and a mark drawn the first way sits in a taskbar looking like
//! it lost its background.
//!
//! # Why a bolt
//!
//! `zerem` is a current — of water, and of electricity. The bolt is the same
//! word read the other way, which is why it is not a departure from the name.
//! It is also the only shape considered that survives sixteen pixels without
//! being redrawn, because it is one filled form and nothing else: no thin
//! stroke to close up, no second element competing for the same pixels.

/// Every measurement here is a fraction of the tile's side, so one set of
/// numbers describes the mark at sixteen pixels and at two hundred and
/// fifty-six. The names are the CSS the design was settled in.
const CORNER: f32 = 0.22;

/// The fill, at 147° — down and to the right.
const SKY: [f32; 3] = [15.0, 175.0, 255.0]; // #0FAFFF
const MID: [f32; 3] = [43.0, 124.0, 246.0]; // #2B7CF6
const DEEP: [f32; 3] = [64.0, 84.0, 228.0]; // #4054E4
const FILL_ANGLE: f32 = 147.0;
const FILL_MIDPOINT: f32 = 0.52;

/// The light in the near corner: an ellipse of 90 by 80 per cent, centred a
/// fifth of the way in and a little down from the top.
const SHEEN: [f32; 4] = [0.22, 0.08, 0.90, 0.80];
const SHEEN_STRENGTH: f32 = 0.30;
const SHEEN_REACH: f32 = 0.62;

/// And the magenta in the far one, which is what stops the tile being a flat
/// blue square. Wider than the tile on purpose: only its edge is ever seen.
const WASH: [f32; 3] = [221.0, 60.0, 226.0]; // #DD3CE2
const WASH_AT: [f32; 4] = [0.88, 0.92, 1.20, 1.10];
const WASH_STRENGTH: f32 = 0.42;
const WASH_REACH: f32 = 0.58;

/// The hairline along the top edge. One pixel, whatever the size — it is a lit
/// edge, and a lit edge that scaled with the icon would stop being one.
const EDGE_STRENGTH: f32 = 0.34;

/// How much of the tile the glyph takes up.
const GLYPH: f32 = 0.62;

/// The bolt, in the four-hundred-unit space it was drawn in. Seven points, and
/// the two that matter are the end of the upper arm and the start of the lower:
/// that pair is the notch that makes it a bolt rather than a chevron.
const BOLT: [[f32; 2]; 7] = [
    [157.055, 0.0],
    [90.798, 196.319],
    [164.417, 196.319],
    [88.344, 400.0],
    [289.571, 159.509],
    [218.405, 159.509],
    [311.656, 0.0],
];

/// The side of the space `BOLT` is measured in.
const BOLT_SPACE: f32 = 400.0;

/// The mark, at whatever size is asked for.
///
/// Straight RGBA, row-major from the top — the layout every raster consumer
/// wants and the one the ICO writer converts from.
#[must_use]
pub fn rgba(size: u32) -> Vec<u8> {
    let across = size as f32;
    let mut out = vec![0_u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            // The middle of the pixel, as a fraction of the side. Everything
            // below is in these units, which is what lets one set of numbers
            // describe every size.
            let (u, v) = ((x as f32 + 0.5) / across, (y as f32 + 0.5) / across);

            let edge = tile(u, v);
            let cover = coverage(edge, across);
            if cover <= 0.0 {
                continue;
            }

            let mut colour = ground(u, v);
            // The lit top edge: one pixel deep, inside the tile, and only along
            // the top of it.
            let depth = -edge * across;
            if depth < 1.0 && v < 0.5 {
                colour = over(colour, [255.0; 3], EDGE_STRENGTH * (1.0 - depth.max(0.0)));
            }
            // The glyph last and over everything, because it is the part that
            // has to be legible at sixteen pixels and nothing may tint it.
            colour = over(colour, [255.0; 3], coverage(glyph(u, v), across));

            let i = ((y * size + x) * 4) as usize;
            out[i] = colour[0] as u8;
            out[i + 1] = colour[1] as u8;
            out[i + 2] = colour[2] as u8;
            out[i + 3] = (cover * 255.0) as u8;
        }
    }
    out
}

/// How far outside the tile a point is — negative inside.
///
/// A square with rounded corners, filling the canvas. The radius is the one
/// Windows 11 gives an application icon, and the one this app already asks the
/// desktop for on its own window.
fn tile(u: f32, v: f32) -> f32 {
    let (dx, dy) = ((u - 0.5).abs() - (0.5 - CORNER), (v - 0.5).abs() - (0.5 - CORNER));
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - CORNER
}

/// How far outside the bolt a point is, in fractions of the side.
fn glyph(u: f32, v: f32) -> f32 {
    // Into the bolt's own space: the glyph is centred and takes `GLYPH` of the
    // tile, so its whole four hundred units map onto that fraction.
    let to_bolt = |c: f32| ((c - 0.5) / GLYPH).mul_add(BOLT_SPACE, BOLT_SPACE / 2.0);
    polygon(to_bolt(u), to_bolt(v), &BOLT) * GLYPH / BOLT_SPACE
}

/// How far outside a polygon a point is — negative inside.
///
/// Nearest edge for the distance, a crossing count for the sign. Written out
/// rather than reduced to a convex test because the shape is concave: the notch
/// that makes a bolt a bolt is exactly what a convex test would fill in.
fn polygon(x: f32, y: f32, points: &[[f32; 2]]) -> f32 {
    let n = points.len();
    let mut nearest = f32::MAX;
    let mut inside = false;

    for i in 0..n {
        let (here, prev) = (points[i], points[(i + n - 1) % n]);
        let (ex, ey) = (prev[0] - here[0], prev[1] - here[1]);
        let (wx, wy) = (x - here[0], y - here[1]);
        let along = (ey.mul_add(wy, ex * wx) / ey.mul_add(ey, ex * ex)).clamp(0.0, 1.0);
        let (ox, oy) = (ex.mul_add(-along, wx), ey.mul_add(-along, wy));
        nearest = nearest.min(oy.mul_add(oy, ox * ox));

        // A horizontal ray, counting the edges it crosses. All three or none:
        // the "none" half is the one that is easy to leave out, and leaving it
        // out inverts the answer on every edge the ray meets from below --
        // which reads as a shape that is inside-out rather than as a shape that
        // is wrong, so it looks plausible until something is measured.
        let (below, above) = (y >= here[1], y < prev[1]);
        let leftward = ex * wy > ey * wx;
        if (below && above && leftward) || !(below || above || leftward) {
            inside = !inside;
        }
    }

    if inside {
        -nearest.sqrt()
    } else {
        nearest.sqrt()
    }
}

/// A distance turned into how much of a pixel is covered.
///
/// Half a pixel of feather either side of the edge, which is what makes a curve
/// a curve at sixteen pixels rather than a staircase.
fn coverage(distance: f32, side: f32) -> f32 {
    distance.mul_add(-side, 0.5).clamp(0.0, 1.0)
}

/// The colour of the tile under the glyph.
fn ground(u: f32, v: f32) -> [f32; 3] {
    // The fill first. Three stops, which is one mix on whichever side of the
    // midpoint the pixel falls.
    let t = along(u, v, FILL_ANGLE);
    let base = if t < FILL_MIDPOINT {
        mix(SKY, MID, t / FILL_MIDPOINT)
    } else {
        mix(MID, DEEP, (t - FILL_MIDPOINT) / (1.0 - FILL_MIDPOINT))
    };

    // Then the two lights, in the order they are painted.
    let near = reach(u, v, SHEEN, SHEEN_REACH);
    let far = reach(u, v, WASH_AT, WASH_REACH);
    over(over(base, [255.0; 3], SHEEN_STRENGTH * near), WASH, WASH_STRENGTH * far)
}

/// How far along a linear gradient a point has got.
///
/// Zero degrees points up and the angle turns clockwise, which is the
/// convention the design was written in and not the one maths uses. Dividing by
/// the line's own length is what makes the stops reach the corners of the box
/// rather than stopping at its edges.
fn along(u: f32, v: f32, degrees: f32) -> f32 {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let span = sin.abs() + cos.abs();
    let reach = (u - 0.5).mul_add(sin, -((v - 0.5) * cos));
    (reach / span + 0.5).clamp(0.0, 1.0)
}

/// How far out a point is on an elliptical radial gradient: one where it runs
/// out, zero at the middle, and clamped at both ends.
fn reach(u: f32, v: f32, at: [f32; 4], stop: f32) -> f32 {
    let [cx, cy, rx, ry] = at;
    let out = ((u - cx) / rx).hypot((v - cy) / ry);
    ((stop - out) / stop).clamp(0.0, 1.0)
}

fn mix(from: [f32; 3], to: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (to[0] - from[0]).mul_add(t, from[0]),
        (to[1] - from[1]).mul_add(t, from[1]),
        (to[2] - from[2]).mul_add(t, from[2]),
    ]
}

/// One colour over another at `alpha`. Both are opaque, so this is the mix and
/// nothing more — the tile's own transparency is carried separately, in the
/// coverage, and folding the two together here would darken every edge.
fn over(under: [f32; 3], on: [f32; 3], alpha: f32) -> [f32; 3] {
    mix(under, on, alpha)
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
    use super::{glyph, ico, polygon, rgba, tile, BOLT, BOLT_SPACE};

    /// One pixel of a `size`-square rendering, as RGBA.
    fn pixel(px: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    #[test]
    fn the_corners_are_rounded_and_the_rest_is_solid() {
        // A tile, not a square: the very corner is outside it and everything a
        // little way in is not.
        let px = rgba(64);
        assert_eq!(pixel(&px, 64, 0, 0)[3], 0, "the corner was not rounded off");
        assert_eq!(pixel(&px, 64, 63, 63)[3], 0);
        assert_eq!(pixel(&px, 64, 32, 1)[3], 255, "the top edge should be solid");
        assert_eq!(pixel(&px, 64, 1, 32)[3], 255, "and so should the side");
    }

    #[test]
    fn the_tile_reaches_the_edges() {
        // The mark used to be a shape floating on nothing, which in a taskbar
        // reads as an icon that lost its background. It fills the canvas now,
        // and that is the whole difference between the two designs.
        assert!(tile(0.5, 0.001) < 0.0, "there is a gap above the tile");
        assert!(tile(0.001, 0.5) < 0.0, "there is a gap beside it");
    }

    #[test]
    fn the_bolt_is_white_and_the_tile_is_not() {
        // The glyph is painted last and nothing tints it. Well inside the
        // upper arm, not near its edge -- a pixel on the boundary is a blend by
        // design and would fail this for the right reason.
        let px = rgba(128);
        let on_bolt = pixel(&px, 128, 64, 40);
        let on_tile = pixel(&px, 128, 12, 40);
        assert_eq!([on_bolt[0], on_bolt[1], on_bolt[2]], [255, 255, 255]);
        assert!(on_tile[2] > on_tile[0] + 40, "the tile is not blue: {on_tile:?}");
    }

    #[test]
    fn the_bolt_has_its_notch() {
        // What makes it a bolt rather than a chevron, and what a convex test
        // would quietly fill in.
        assert!(polygon(200.0, 20.0, &BOLT) < 0.0, "the upper arm is not solid");
        assert!(polygon(120.0, 20.0, &BOLT) > 0.0, "there is fill left of the upper arm");
        assert!(polygon(240.0, 180.0, &BOLT) < 0.0, "the lower arm is not solid");
        // Out to the right of the lower arm, under the re-entrant corner. This
        // is the one a convex hull would swallow.
        assert!(polygon(280.0, 250.0, &BOLT) > 0.0, "the notch was filled in");
    }

    #[test]
    fn the_bolt_narrows_towards_the_point() {
        // Wide where it starts, a point where it ends. Getting that backwards
        // draws the same shape upside down, which still looks like a bolt.
        let across = |y: f32| (0..400_u16).filter(|&n| polygon(f32::from(n), y, &BOLT) < 0.0).count();
        let (top, tip) = (across(10.0), across(380.0));
        assert!(top > tip * 4, "{top} across the top against {tip} at the point");
    }

    #[test]
    fn the_glyph_sits_inside_the_tile() {
        // It is 62% of the side, centred, so nothing of it may reach an edge.
        for at in [0.02_f32, 0.06, 0.94, 0.98] {
            assert!(glyph(at, 0.5) > 0.0, "the glyph reaches x = {at}");
            assert!(glyph(0.5, at) > 0.0, "the glyph reaches y = {at}");
        }
        assert!(glyph(0.5, 0.5) < 0.0, "the middle of the tile is not glyph");
    }

    #[test]
    fn the_light_comes_from_the_near_corner() {
        // The fill runs from a bright sky blue to a deeper one at 147 degrees,
        // so the near corner has to be the lighter. A gradient transcribed with
        // its ends swapped would still draw a perfectly good tile.
        let px = rgba(128);
        let near = pixel(&px, 128, 20, 20);
        let far = pixel(&px, 128, 108, 108);
        assert!(near[1] > far[1] + 20, "the near corner is not lighter: {near:?} {far:?}");
    }

    #[test]
    fn the_far_corner_is_the_warm_one() {
        // The magenta only ever reaches one corner. It is what keeps the tile
        // from being a flat blue square, and a gradient placed the wrong way
        // round would put it in the other corner or nowhere at all.
        let px = rgba(128);
        let cool = pixel(&px, 128, 20, 20);
        let warm = pixel(&px, 128, 110, 110);
        assert!(warm[0] > cool[0] + 20, "the far corner is not redder: {warm:?} {cool:?}");
    }

    #[test]
    fn the_bolt_is_measured_in_its_own_space() {
        // Every point of it has to be inside the box it was drawn in, or the
        // mapping onto the tile is off by however much it hangs over.
        for [x, y] in BOLT {
            assert!((0.0..=BOLT_SPACE).contains(&x), "x = {x}");
            assert!((0.0..=BOLT_SPACE).contains(&y), "y = {y}");
        }
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

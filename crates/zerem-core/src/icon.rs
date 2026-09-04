//! The app mark, drawn rather than shipped as a file.
//!
//! One source for both places it appears: the tray rasterises it at 32 px at
//! runtime, and the build script writes the same shape into the `.ico` that
//! becomes the executable's icon. A committed binary asset would be a second
//! copy of this that nothing keeps in step.
//!
//! Pure maths into a `Vec<u8>`, which is why it lives in core.

/// A rounded square in the accent blue with a white download arrow.
///
/// Straight RGBA, row-major from the top — the layout every raster consumer
/// wants and the one the ICO writer converts from.
#[must_use]
pub fn rgba(size: u32) -> Vec<u8> {
    // One fixed blue, and the same one the app fills its buttons with. It was
    // #4c8dff, which the palette left behind — so the mark in the taskbar was a
    // different blue from every accent inside the window, and an identity split
    // in two is not an identity.
    //
    // Fixed and not themed on purpose: this is drawn into the taskbar, the
    // `.ico` and the file associations, where there is no theme to read.
    const ACCENT: [u8; 3] = [0x08, 0x90, 0xff];

    let n = size as f32;
    let radius = n * 0.22;
    let mut out = vec![0_u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);

            // Rounded-rect coverage: how far past the corner circle we are.
            let dx = (radius - fx).max(fx - (n - radius)).max(0.0);
            let dy = (radius - fy).max(fy - (n - radius)).max(0.0);
            // A pixel of feathering, so the corners are curves and not stairs.
            let cover = (0.5 - (dx.hypot(dy) - radius)).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }

            let [r, g, b] = if in_arrow(fx, fy, n) { [0xff, 0xff, 0xff] } else { ACCENT };
            let i = ((y * size + x) * 4) as usize;
            out[i] = r;
            out[i + 1] = g;
            out[i + 2] = b;
            out[i + 3] = (cover * 255.0) as u8;
        }
    }
    out
}

/// A downward arrow: a stem, and a head that tapers to the point.
#[must_use]
pub fn in_arrow(x: f32, y: f32, n: f32) -> bool {
    let cx = n / 2.0;
    let (top, tip) = (n * 0.22, n * 0.78);
    let shoulder = n * 0.52;

    if y < top || y > tip {
        return false;
    }
    if y <= shoulder {
        return (x - cx).abs() <= n * 0.09;
    }
    let taper = (tip - y) / (tip - shoulder);
    (x - cx).abs() <= n * 0.26 * taper
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
    use super::{ico, in_arrow, rgba};

    #[test]
    fn the_arrow_points_down() {
        let n = 32.0;
        let cx = n / 2.0;
        // Wide at the shoulder, a point at the tip: that is what makes it read
        // as an arrow rather than a cross.
        let shoulder = (0..32_u16).filter(|&x| in_arrow(f32::from(x), n * 0.55, n)).count();
        let tip = (0..32_u16).filter(|&x| in_arrow(f32::from(x), n * 0.76, n)).count();
        assert!(shoulder > tip, "{shoulder} vs {tip}");
        assert!(in_arrow(cx, n * 0.3, n), "the stem is centred");
        assert!(!in_arrow(n / 2.0, 0.0, n), "nothing above the top");
    }

    #[test]
    fn the_corners_are_transparent_and_the_middle_is_not() {
        let px = rgba(32);
        assert_eq!(px[3], 0, "the very corner is outside the rounded square");
        let middle = ((16 * 32 + 16) * 4 + 3) as usize;
        assert_eq!(px[middle], 255);
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

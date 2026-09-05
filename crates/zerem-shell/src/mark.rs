//! Giving the window the application's mark.
//!
//! # Why this is not one line of `.slint`
//!
//! Slint's `Window` has an `icon` property, and it does nothing for a drawing
//! made at run time. The winit backend only calls `set_window_icon` when the
//! image's *cache key* changes, and `ImageCacheKey::new` returns nothing at all
//! for an image built from a pixel buffer — so the key is the same absence
//! before and after, the comparison finds no change, and the icon is never set.
//! It works for `@image-url`, which is a file with a path to key on.
//!
//! Shipping a PNG to get a path would mean a second copy of a mark that is
//! generated, and nothing keeping the two in step — which is the whole reason
//! the mark is drawn rather than shipped. So it is set here instead.
//!
//! # Why it matters more than it sounds
//!
//! A window with no icon is not a window with no icon in the taskbar: Windows
//! falls back to the one compiled into the executable, and **caches that by
//! path**. A mark that changes then keeps showing the old one until the cache
//! is cleared or the machine restarts, which looks exactly like a build that
//! did not take. Set on the window, it arrives with the window and there is
//! nothing in between to go stale.

/// Give this process's window the mark that `draw` produces.
///
/// Takes the drawing rather than the pixels because the sizes are the
/// desktop's to choose, not the caller's — they follow the display's scaling,
/// and asking for the wrong one gets a blurred icon rather than a wrong one,
/// which is harder to notice and no better.
///
/// Call once, after the window is on screen. Before that there is no window to
/// give anything to, and this quietly does nothing.
pub fn wear(draw: impl Fn(u32) -> Vec<u8>) {
    imp::wear(draw);
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIcon, GetSystemMetrics, SendMessageW, HICON, ICON_BIG, ICON_SMALL, SM_CXICON, SM_CXSMICON,
        WM_SETICON,
    };

    pub fn wear(draw: impl Fn(u32) -> Vec<u8>) {
        let Some(window) = crate::window::own() else { return };

        // What this display wants, which is not always sixteen and thirty-two:
        // both metrics scale with the desktop, so a machine at 150 per cent
        // asks for twenty-four and forty-eight. Handing it the smaller pair
        // and letting Windows stretch them is the difference between a mark
        // and a smudge.
        for (which, metric) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
            let size = unsafe { GetSystemMetrics(metric) };
            let Ok(size) = u32::try_from(size) else { continue };
            let Some(icon) = icon(&draw(size), size) else { continue };
            // The window owns it from here. Never destroyed: it is set once
            // for the life of the process, and freeing an icon a window is
            // still drawing is how a taskbar button goes blank.
            unsafe {
                SendMessageW(window, WM_SETICON, Some(WPARAM(which as usize)), Some(LPARAM(icon.0 as isize)));
            }
        }
    }

    /// An `HICON` from straight RGBA.
    ///
    /// The shape winit uses for the same job, and worth following rather than
    /// inventing: BGRA for the colour, and a mask of inverted alpha, one byte
    /// per pixel. The mask is documented as one *bit* per pixel and a 32-bit
    /// icon's alpha channel is what actually gets used — but a buffer larger
    /// than any reading of it needs cannot be read past, and on the fallback
    /// paths that do consult it, inverted alpha is the right answer where all
    /// zeroes would put a black square behind the mark.
    fn icon(rgba: &[u8], pixels: u32) -> Option<HICON> {
        let side = i32::try_from(pixels).ok()?;
        let mut colour = rgba.to_vec();
        let mut mask = Vec::with_capacity(rgba.len() / 4);
        for pixel in colour.chunks_exact_mut(4) {
            mask.push(pixel[3].wrapping_sub(u8::MAX));
            pixel.swap(0, 2);
        }
        unsafe { CreateIcon(None, side, side, 1, 32, mask.as_ptr(), colour.as_ptr()) }.ok()
    }
}

#[cfg(not(windows))]
mod imp {
    /// Not implemented here yet.
    ///
    /// X11 takes `_NET_WM_ICON` and Wayland takes none at all — it reads the
    /// icon from the `.desktop` file the application was launched from, which
    /// is a file the Linux build ships rather than a call it makes.
    pub fn wear(_draw: impl Fn(u32) -> Vec<u8>) {}
}

//! The icon the desktop itself shows for a kind of file.
//!
//! Not one drawn here. A `.mkv` in this app should carry the same picture it
//! carries in Explorer, because that picture is whatever the user installed —
//! their player, their archiver, their reader — and a hand-drawn stand-in is a
//! second opinion about something the machine has already answered.
//!
//! Asked by extension and never by path, and the file is never touched. That is
//! `SHGFI_USEFILEATTRIBUTES`: the shell answers from the association alone, so
//! this works for a torrent whose files have not been written yet, and it never
//! wakes a sleeping drive to look at one that has.

/// One icon, unpacked into the bytes a renderer wants.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// Straight (not premultiplied) RGBA, row by row from the top.
    pub rgba: Vec<u8>,
}

/// The small icon for files with this extension — `mkv`, `iso`, no dot.
///
/// `None` when the platform has no such notion, or when the shell declines. A
/// caller that gets `None` draws its own glyph, which is what it would have
/// drawn anyway.
#[must_use]
pub fn for_extension(extension: &str) -> Option<Bitmap> {
    imp::for_extension(extension)
}

#[cfg(windows)]
mod imp {
    use super::Bitmap;

    use windows::core::HSTRING;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGFI_USEFILEATTRIBUTES,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    /// COM, once per thread that asks.
    ///
    /// An extension with no handler — `.zqx9` — comes back fine without it,
    /// because the shell answers from its own generic icon. One with a handler
    /// does not: `.txt` goes through the association's own icon provider, which
    /// is a COM object, and on a thread where COM was never started the call
    /// simply returns nothing. That asymmetry is what this exists for, and it
    /// is exactly the kind of thing that works in the app — where the toolkit
    /// has already started COM on the UI thread — and fails everywhere else.
    fn ensure_com() {
        thread_local! {
            static STARTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        }
        STARTED.with(|started| {
            if started.replace(true) {
                return;
            }
            // Both "already initialised" answers are usable, and there is no
            // matching uninitialise on purpose: the thread keeps it for as long
            // as it lives, which is the life of the window.
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        });
    }

    /// One at a time.
    ///
    /// An extension with a handler is answered by a COM object in an
    /// apartment, and two threads asking at once get one right answer and one
    /// empty one — found by the test suite, which runs in parallel, failing on
    /// `.txt` while `.zqx9` passed: the one with a handler and the one without.
    ///
    /// The app only ever asks from the window thread, so this costs a lock on
    /// a cache miss and buys a function that is true from anywhere.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub fn for_extension(extension: &str) -> Option<Bitmap> {
        let _held = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_com();
        // A name that cannot exist, so the shell has nothing to stat even if
        // the flag were ignored.
        let name = HSTRING::from(format!("zerem-icon-probe.{extension}"));
        let mut info = SHFILEINFOW::default();
        let ok = unsafe {
            SHGetFileInfoW(
                &name,
                FILE_ATTRIBUTE_NORMAL,
                Some(&raw mut info),
                u32::try_from(size_of::<SHFILEINFOW>()).ok()?,
                SHGFI_ICON | SHGFI_SMALLICON | SHGFI_USEFILEATTRIBUTES,
            )
        };
        if ok == 0 || info.hIcon.is_invalid() {
            return None;
        }

        let bitmap = unpack(info.hIcon);
        // Ours to destroy the moment it is copied — an icon handle leaked once
        // per extension is a handle leaked for the life of the process.
        let _ = unsafe { DestroyIcon(info.hIcon) };
        bitmap
    }

    /// HICON → RGBA.
    ///
    /// The colour bitmap inside an icon is bottom-up BGRA with the alpha
    /// already in it for anything modern. `GetDIBits` is asked for top-down by
    /// passing a negative height, so the rows come out in the order every
    /// renderer wants rather than upside down.
    fn unpack(icon: HICON) -> Option<Bitmap> {
        let mut parts = ICONINFO::default();
        unsafe { GetIconInfo(icon, &raw mut parts) }.ok()?;
        // Both bitmaps are ours once GetIconInfo hands them over, whatever
        // happens next.
        let colour = parts.hbmColor;
        let mask = parts.hbmMask;
        let bitmap = read_pixels(colour);
        for handle in [colour, mask] {
            if !handle.is_invalid() {
                let _ = unsafe { DeleteObject(HGDIOBJ(handle.0)) };
            }
        }
        bitmap
    }

    fn read_pixels(colour: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<Bitmap> {
        if colour.is_invalid() {
            return None;
        }
        let mut shape = BITMAP::default();
        let wrote = unsafe {
            GetObjectW(
                HGDIOBJ(colour.0),
                i32::try_from(size_of::<BITMAP>()).ok()?,
                Some((&raw mut shape).cast()),
            )
        };
        if wrote == 0 {
            return None;
        }
        let width = u32::try_from(shape.bmWidth).ok()?;
        let height = u32::try_from(shape.bmHeight).ok()?;
        if width == 0 || height == 0 {
            return None;
        }

        let mut header = BITMAPINFO::default();
        header.bmiHeader.biSize = u32::try_from(size_of::<BITMAPINFOHEADER>()).ok()?;
        header.bmiHeader.biWidth = shape.bmWidth;
        // Negative: top-down, so the rows arrive the way they are drawn.
        header.bmiHeader.biHeight = -shape.bmHeight;
        header.bmiHeader.biPlanes = 1;
        header.bmiHeader.biBitCount = 32;
        header.bmiHeader.biCompression = BI_RGB.0;

        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        let screen = unsafe { GetDC(None) };
        let read = unsafe {
            GetDIBits(
                screen,
                colour,
                0,
                height,
                Some(pixels.as_mut_ptr().cast()),
                &raw mut header,
                DIB_RGB_COLORS,
            )
        };
        unsafe { ReleaseDC(None, screen) };
        if read == 0 {
            return None;
        }

        // BGRA → RGBA. Windows stores the blue channel first, and an icon that
        // comes out with its reds and blues swapped is the classic sign of
        // having skipped this.
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Some(Bitmap { width, height, rgba: pixels })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Bitmap;

    /// Not implemented here yet.
    ///
    /// The freedesktop answer is an icon theme lookup through the MIME type,
    /// which needs a theme walker and a PNG or SVG decoder. It lands with the
    /// Linux build; until then the caller draws its own glyph, which is what it
    /// would have drawn anyway.
    pub const fn for_extension(_extension: &str) -> Option<Bitmap> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::for_extension;

    #[test]
    #[cfg(windows)]
    fn a_common_extension_comes_back_as_pixels() {
        // Not asserting which picture: that is whatever this machine has
        // installed, and a test that demanded one would fail on another desk.
        // What is asserted is the shape of the answer.
        let icon = for_extension("txt").expect("the shell has an icon for text files");
        assert!(icon.width > 0 && icon.height > 0);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    }

    #[test]
    #[cfg(windows)]
    fn an_extension_nobody_has_still_answers() {
        // The shell falls back to its generic file icon rather than failing,
        // which is the right answer for a torrent full of `.r00` parts.
        assert!(for_extension("zqx9").is_some());
    }

    #[test]
    fn a_missing_extension_is_not_a_crash() {
        // Torrents do contain files with no dot in the name at all.
        let _ = for_extension("");
    }
}

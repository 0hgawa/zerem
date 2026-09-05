//! Asking Windows to round the window's corners.
//!
//! Windows 11 rounds a window because the *frame* is rounded, and an
//! application that draws its own frame has none — so an undecorated window
//! comes out with four square corners in a desktop where nothing else has any.
//!
//! `DWMWA_WINDOW_CORNER_PREFERENCE` is how you ask for them back. It is the
//! same corner the desktop draws everywhere else, cut by the compositor
//! outside the window, so it is the real one rather than a rounded rectangle
//! painted on top with the desktop still square behind it.
//!
//! Nothing happens on Windows 10, which has no such attribute and ignores the
//! call — and no such corners either, so there is nothing to be inconsistent
//! with.

/// Round this process's window, if the desktop knows how.
pub fn round() {
    imp::round();
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible};

    pub fn round() {
        let Some(window) = window() else { return };
        let preference = DWMWCP_ROUND;
        // Ignored on Windows 10, which does not know the attribute and does not
        // round anything either.
        let _ = unsafe {
            DwmSetWindowAttribute(
                window,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const preference).cast(),
                u32::try_from(size_of_val(&preference)).unwrap_or_default(),
            )
        };
    }

    /// This process's visible top-level window.
    ///
    /// The same search `attention` makes, and for the same reason: Slint will
    /// not hand out a native handle without a feature this build does not
    /// carry, so the window is found the way any process can find its own.
    fn window() -> Option<HWND> {
        let mut found = HWND::default();
        let _ = unsafe { EnumWindows(Some(visit), LPARAM(&raw mut found as isize)) };
        (!found.is_invalid()).then_some(found)
    }

    unsafe extern "system" fn visit(window: HWND, out: LPARAM) -> BOOL {
        let mut owner = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&raw mut owner)) };
        if owner != unsafe { GetCurrentProcessId() } || !unsafe { IsWindowVisible(window) }.as_bool() {
            return TRUE;
        }
        unsafe { *(out.0 as *mut HWND) = window };
        BOOL(0)
    }
}

#[cfg(not(windows))]
mod imp {
    /// Nothing to ask. A compositor here rounds a window or does not, and it is
    /// not the application's business either way.
    pub const fn round() {}
}

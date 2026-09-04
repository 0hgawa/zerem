//! Asking for attention without taking it.
//!
//! A download finishing is worth being told about and is never worth being
//! interrupted for. Stealing focus from whatever somebody is doing to announce
//! that a file arrived is the behaviour that makes people turn notifications
//! off, so this flashes the taskbar button and stops there.
//!
//! Nothing happens when the window is already in front — it is a way of saying
//! "when you come back", and somebody who is looking at it has come back.
//!
//! A real toast is what this wants to be, and it is not possible yet: Windows
//! wants an AppUserModelID and a shortcut in the Start menu to attribute one,
//! which means an installed copy. It lands with the installer.

/// Flash this process's window in the taskbar, if it is not already in front.
pub fn ask() {
    imp::ask();
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicIsize, Ordering};

    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FlashWindowEx, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
        FLASHWINFO, FLASHW_ALL, FLASHW_TIMERNOFG,
    };

    /// Found once and kept. Enumerating every top-level window on the desktop
    /// to answer "which one is mine?" is not something to do per download.
    static WINDOW: AtomicIsize = AtomicIsize::new(0);

    pub fn ask() {
        let Some(window) = window() else { return };
        // Already looking at it. Flashing a window somebody is using is noise
        // with no message in it.
        if unsafe { GetForegroundWindow() } == window {
            return;
        }
        let flash = FLASHWINFO {
            cbSize: u32::try_from(size_of::<FLASHWINFO>()).unwrap_or_default(),
            hwnd: window,
            // Until it is looked at, rather than a fixed number of blinks: the
            // point is that it is still saying so when somebody comes back.
            dwFlags: FLASHW_ALL | FLASHW_TIMERNOFG,
            uCount: 0,
            dwTimeout: 0,
        };
        let _ = unsafe { FlashWindowEx(&raw const flash) };
    }

    fn window() -> Option<HWND> {
        let cached = WINDOW.load(Ordering::Relaxed);
        if cached != 0 {
            return Some(HWND(cached as *mut _));
        }
        // Slint does not hand out a native handle without a feature this build
        // does not carry, so the window is found the way any process can find
        // its own: the visible top-level one that belongs to this process.
        let mut found = HWND::default();
        let _ = unsafe { EnumWindows(Some(visit), LPARAM(&raw mut found as isize)) };
        if found.is_invalid() {
            return None;
        }
        WINDOW.store(found.0 as isize, Ordering::Relaxed);
        Some(found)
    }

    unsafe extern "system" fn visit(window: HWND, out: LPARAM) -> BOOL {
        let mut owner = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&raw mut owner)) };
        if owner != unsafe { GetCurrentProcessId() } || !unsafe { IsWindowVisible(window) }.as_bool() {
            return TRUE;
        }
        unsafe { *(out.0 as *mut HWND) = window };
        // Stop: the first visible one is the window.
        BOOL(0)
    }
}

#[cfg(not(windows))]
mod imp {
    /// Not implemented here yet.
    ///
    /// The freedesktop answer is `org.freedesktop.Notifications` over D-Bus,
    /// which is a notification rather than a flash and belongs with the Linux
    /// build. Silence until then, which is what the app did before.
    pub const fn ask() {}
}

#[cfg(test)]
mod tests {
    #[test]
    fn asking_with_no_window_is_not_a_crash() {
        // Which is the case in a test binary, and would also be the case for a
        // headless run — the reason this returns rather than unwraps.
        super::ask();
    }
}

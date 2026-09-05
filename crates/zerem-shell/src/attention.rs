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
    use windows::Win32::UI::WindowsAndMessaging::{
        FlashWindowEx, GetForegroundWindow, FLASHWINFO, FLASHW_ALL, FLASHW_TIMERNOFG,
    };

    pub fn ask() {
        let Some(window) = crate::window::own() else { return };
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

// No test, and that is the finding rather than an omission.
//
// The one that was here called `ask()` to prove it did not panic. In a test
// binary `EnumWindows` finds the console window, which belongs to this process
// and is visible — so every `cargo test` on this repository flashed a window in
// the taskbar of whoever ran it.
//
// A test whose only assertion is "it returned" is not worth a side effect on
// somebody's desktop. What is left below the `window()` guard is one FFI call
// with nothing in it to get wrong, and the guard itself returns early on the
// only branch a test could reach.

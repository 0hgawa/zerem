//! This process's own window, found rather than handed over.
//!
//! Slint does not give out a native handle without a feature this build does
//! not carry, and three things here need one: rounding the corners the removed
//! frame took with it, flashing the taskbar button, and giving the window its
//! icon. So it is found the way any process can find its own — by enumeration.
//!
//! # Visible is not enough
//!
//! winit keeps a window of its own called `Winit Thread Event Target`, and it
//! is a top-level window of this process and reports itself visible. It also
//! comes back from `EnumWindows` *before* the real one, so "the first visible
//! window" is a rule that picks the wrong one — which would mean corners
//! rounded on a window nobody sees, a taskbar button flashed for nothing, and
//! the icon set where nothing draws it.
//!
//! A title is what separates them. Every window a person can see carries one,
//! and a toolkit's bookkeeping window does not.
//!
//! Found once and remembered. A window does not change its handle, and
//! enumerating every top-level window on the desktop is not something to repeat
//! for an answer that cannot have changed.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
};

static FOUND: AtomicIsize = AtomicIsize::new(0);

/// The handle, or nothing if this process has no visible window yet.
pub fn own() -> Option<HWND> {
    let cached = FOUND.load(Ordering::Relaxed);
    if cached != 0 {
        return Some(HWND(cached as *mut _));
    }
    let mut found = HWND::default();
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&raw mut found as isize)) };
    if found.is_invalid() {
        return None;
    }
    FOUND.store(found.0 as isize, Ordering::Relaxed);
    Some(found)
}

unsafe extern "system" fn visit(window: HWND, out: LPARAM) -> BOOL {
    let mut owner = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&raw mut owner)) };
    let ours = owner == unsafe { GetCurrentProcessId() };
    let shown = unsafe { IsWindowVisible(window) }.as_bool();
    let named = unsafe { GetWindowTextLengthW(window) } > 0;
    if !ours || !shown || !named {
        return TRUE;
    }
    unsafe { *(out.0 as *mut HWND) = window };
    // Stop: the first visible window of ours with a name on it is the window.
    BOOL(0)
}

//! How much room there actually is to open a window in.
//!
//! A preferred size is written in logical pixels, and a logical pixel is not a
//! pixel: at 200 % a window asking for 720 tall wants 1440 device pixels, which
//! is the whole of a 1440p screen. Zerem opened exactly that way — 1511 px tall
//! on a 1440 px display, with its status bar 169 px below the bottom edge, on a
//! setup common enough that it is the one this was found on.
//!
//! The work area rather than the screen, because the taskbar is not room.

/// The usable desktop, in device pixels, minus the taskbar and any docked bars.
///
/// `None` when the platform will not say, which the caller reads as "take the
/// preferred size as written" — the behaviour it had before this existed.
#[must_use]
pub fn work_area() -> Option<(u32, u32)> {
    imp::work_area()
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    pub fn work_area() -> Option<(u32, u32)> {
        let mut rect = RECT::default();
        unsafe {
            SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some((&raw mut rect).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .ok()?;
        let width = u32::try_from(rect.right - rect.left).ok()?;
        let height = u32::try_from(rect.bottom - rect.top).ok()?;
        (width > 0 && height > 0).then_some((width, height))
    }
}

#[cfg(not(windows))]
mod imp {
    /// Not implemented here yet.
    ///
    /// X11 and Wayland answer this differently and neither answers it cheaply.
    /// It lands with the Linux build; until then the window opens at the size
    /// it asks for, which is what it did before this existed.
    pub const fn work_area() -> Option<(u32, u32)> {
        None
    }
}

/// Room left around the edges, in device pixels.
///
/// Off both dimensions, not one: a window that exactly fills the work area is a
/// window with no edge to grab, and the first thing anybody does to a new
/// window is move it.
///
/// It also absorbs the frame. What is being sized is the *client* area, and the
/// border and shadow around it are not free — measured at about seventy device
/// pixels of width on Windows 11, which a margin of forty-eight did not cover.
const MARGIN: u32 = 96;

/// A window size that fits, given what it would like to be.
///
/// Kept pure and separate from the asking so the arithmetic can be tested: the
/// part that talks to the OS has nothing in it but the call.
#[must_use]
pub fn fit(wanted: (u32, u32), available: Option<(u32, u32)>) -> (u32, u32) {
    let Some((room_w, room_h)) = available else { return wanted };
    let cap = |want: u32, room: u32| want.min(room.saturating_sub(MARGIN).max(1));
    (cap(wanted.0, room_w), cap(wanted.1, room_h))
}

#[cfg(test)]
mod tests {
    use super::{fit, work_area};

    #[test]
    fn a_window_that_already_fits_is_left_alone() {
        assert_eq!(fit((1180, 720), Some((2560, 1440))), (1180, 720));
    }

    #[test]
    fn a_window_taller_than_the_desktop_is_brought_back_inside() {
        // The case this exists for: 720 logical at 200 % is 1440 device pixels,
        // which is the whole of a 1440p screen — so the status bar opened below
        // the bottom edge and had never been seen in a window that was not
        // maximised.
        let (_, height) = fit((2360, 1440), Some((2560, 1392)));
        assert!(height < 1392, "still taller than the work area: {height}");
    }

    #[test]
    fn both_dimensions_are_capped_not_just_the_one_that_overflows() {
        let (width, height) = fit((4000, 4000), Some((2560, 1392)));
        assert!(width < 2560 && height < 1392);
    }

    #[test]
    fn a_desktop_that_will_not_say_leaves_the_size_as_asked() {
        // Which is what the app did before any of this, and a readable window
        // beats a window sized from a guess.
        assert_eq!(fit((1180, 720), None), (1180, 720));
    }

    #[test]
    fn an_absurdly_small_desktop_still_yields_a_window() {
        // Never zero: a window of no size is a window nobody can find.
        let (width, height) = fit((1180, 720), Some((10, 10)));
        assert!(width > 0 && height > 0);
    }

    #[test]
    fn the_desktop_answers_with_something_smaller_than_itself() {
        // Not asserting a size — that is this machine's business. What is
        // asserted is that the work area excludes the taskbar, which is the
        // whole reason it is asked for instead of the screen.
        if let Some((width, height)) = work_area() {
            assert!(width > 0 && height > 0);
        }
    }
}

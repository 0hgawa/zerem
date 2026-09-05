//! Whether the desktop is set to a dark theme.
//!
//! So an application can open in the same one as everything else on the screen
//! rather than in whatever it was left in, which is what "follow the system"
//! means and what most people expect a fresh install to do.
//!
//! Read, not watched. Somebody who changes their desktop theme while a torrent
//! client is open gets the new one on the next launch, and the alternative is a
//! registry watch running for the life of the process to catch something that
//! happens a few times a year.

/// `true` for dark, `false` for light, `None` where the desktop will not say.
///
/// `None` is the answer that matters: it is what tells the caller to keep
/// whatever choice is already stored rather than guess at one.
#[must_use]
pub fn prefers_dark() -> Option<bool> {
    imp::prefers_dark()
}

#[cfg(windows)]
mod imp {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    pub fn prefers_dark() -> Option<bool> {
        // `AppsUseLightTheme` and not `SystemUsesLightTheme`: the second is the
        // taskbar and the start menu, and an application is an app.
        let mut value = 0u32;
        let mut size = u32::try_from(size_of::<u32>()).ok()?;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some((&raw mut value).cast()),
                Some(&raw mut size),
            )
        };
        status.is_ok().then_some(value == 0)
    }
}

#[cfg(not(windows))]
mod imp {
    /// Not implemented here yet.
    ///
    /// The freedesktop answer is `org.freedesktop.appearance color-scheme` over
    /// the settings portal, which needs D-Bus. It lands with the Linux build;
    /// until then the stored choice stands, which is what `None` asks for.
    pub const fn prefers_dark() -> Option<bool> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(windows)]
    fn the_desktop_has_an_opinion_and_it_is_a_boolean() {
        // Not asserting which: that is this machine's business. What is
        // asserted is that the key is found at all, because a typo in it would
        // fail exactly the same way a light desktop does.
        assert!(super::prefers_dark().is_some(), "the theme key was not found");
    }
}

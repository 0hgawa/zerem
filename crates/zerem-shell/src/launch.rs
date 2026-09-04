//! Handing a path back to the desktop.
//!
//! Two verbs, and the difference between them matters to whoever is clicking:
//! [`open`] gives the file to whatever program owns that kind of file, and
//! [`reveal`] opens the file manager with it picked out. People reach for the
//! second more often than they admit — "where did that go" is a more common
//! question than "play it".
//!
//! Neither reports failure. There is nothing useful to say: the desktop has
//! already shown whatever it wants to show about a file it cannot open, and a
//! second complaint from this app on top of that is noise. What does not happen
//! is a crash, and that is what the tests here are about.

use std::path::Path;

/// Hand the path to whatever the desktop opens that kind of file with.
pub fn open(path: &Path) {
    imp::open(path);
}

/// Show the path in the file manager, picked out if it is a file.
pub fn reveal(path: &Path) {
    imp::reveal(path);
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// A wide, NUL-terminated copy of an OS string.
    ///
    /// Kept as a `Vec` the caller holds: a pointer into a temporary is a
    /// pointer into freed memory by the time the call reads it, which is the
    /// classic way to get this wrong.
    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn open(path: &Path) {
        let file = wide(path.as_os_str());
        let verb = wide(std::ffi::OsStr::new("open"));
        // ShellExecuteW rather than spawning a shell: `cmd /c start` flashes a
        // console window for as long as it takes to hand the file over, and on
        // a slow first launch that is long enough to see.
        let _ = unsafe {
            ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(file.as_ptr()), None, None, SW_SHOWNORMAL)
        };
    }

    pub fn reveal(path: &Path) {
        // `/select,` needs the argument glued to it with no space, and the path
        // quoted — a comma or a space in a filename otherwise splits it into
        // two arguments and Explorer opens the user's Documents instead.
        //
        // A folder is shown rather than selected inside its own parent, which
        // is what somebody asking to see a folder means.
        let argument = if path.is_dir() {
            format!("\"{}\"", path.display())
        } else {
            format!("/select,\"{}\"", path.display())
        };
        let file = wide(std::ffi::OsStr::new("explorer.exe"));
        let args = wide(std::ffi::OsStr::new(&argument));
        let _ = unsafe {
            ShellExecuteW(None, None, PCWSTR(file.as_ptr()), PCWSTR(args.as_ptr()), None, SW_SHOWNORMAL)
        };
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;

    pub fn open(path: &Path) {
        spawn(path);
    }

    /// No `/select,` equivalent that every file manager agrees on, so the
    /// containing folder is opened and the file is not picked out. Naming the
    /// folder is most of the answer, and guessing at Nautilus versus Dolphin
    /// versus Thunar would be three ways to be wrong.
    pub fn reveal(path: &Path) {
        spawn(if path.is_dir() { path } else { path.parent().unwrap_or(path) });
    }

    fn spawn(path: &Path) {
        if let Err(e) = std::process::Command::new("xdg-open").arg(path).spawn() {
            tracing::warn!(path = %path.display(), error = %e, "could not hand the path to the desktop");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{open, reveal};
    use std::path::Path;

    #[test]
    fn a_path_that_is_not_there_is_not_a_crash() {
        // Which is the case in a test binary, and also the case when somebody
        // deletes a file between the menu opening and the click landing.
        open(Path::new("Z:/nowhere/at/all/nothing.mkv"));
        reveal(Path::new("Z:/nowhere/at/all/nothing.mkv"));
    }

    #[test]
    fn an_empty_path_is_not_a_crash_either() {
        open(Path::new(""));
        reveal(Path::new(""));
    }
}

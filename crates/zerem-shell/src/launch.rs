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
//! second complaint from this app on top of that is noise.
//!
//! # What is tested here, and what is not
//!
//! The argument is tested. The launch is not, and deliberately: the first
//! version of these tests called `open` and `reveal` with a made-up path to
//! prove they did not panic, and every `cargo test` on this repository opened
//! Explorer windows on the machine running it. `reveal("")` in particular
//! builds `/select,""`, which is the exact mistake the quoting below exists to
//! prevent — Explorer shrugs and opens the user's Documents.
//!
//! A test that has to be watched to notice what it did is worse than no test.
//! What is left is a pure function that builds the argument, which is where the
//! mistake would actually be made; past it is one FFI call with nothing in it
//! to get wrong.

use std::path::Path;

/// Hand the path to whatever the desktop opens that kind of file with.
pub fn open(path: &Path) {
    imp::open(path);
}

/// Show the path in the file manager, picked out if it is a file.
pub fn reveal(path: &Path) {
    imp::reveal(path);
}

/// Hand a URL to whatever opens that kind of link.
///
/// Separate from [`open`] because a URL is not a path: it has no filesystem
/// behind it, and building a `Path` out of one on Windows mangles the `//`
/// after the scheme.
pub fn open_url(url: &str) {
    imp::open_url(url);
}

/// What to hand Explorer to show a path.
///
/// `/select,` takes its argument glued to it with no space, and the path has to
/// be quoted: unquoted, a comma or a space in a filename splits it into two
/// arguments and Explorer shrugs and opens the user's Documents.
///
/// A folder is shown rather than selected inside its parent, which is what
/// somebody asking to see a folder means. Whether it *is* a folder is passed in
/// rather than asked here, so this can be tested without a disk.
#[must_use]
fn explorer_argument(path: &Path, is_folder: bool) -> String {
    if is_folder {
        format!("\"{}\"", path.display())
    } else {
        format!("/select,\"{}\"", path.display())
    }
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

    pub fn open_url(url: &str) {
        // The same call as a file's. The shell resolves a URL through the
        // protocol handler rather than the file association, and works out
        // which from the string itself.
        let target = wide(std::ffi::OsStr::new(url));
        let verb = wide(std::ffi::OsStr::new("open"));
        let _ = unsafe {
            ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(target.as_ptr()), None, None, SW_SHOWNORMAL)
        };
    }

    pub fn reveal(path: &Path) {
        let argument = super::explorer_argument(path, path.is_dir());
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

    pub fn open_url(url: &str) {
        if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
            tracing::warn!(url, error = %e, "could not hand the link to the desktop");
        }
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
    use super::explorer_argument;
    use std::path::Path;

    #[test]
    fn a_file_is_picked_out_and_a_folder_is_opened() {
        assert_eq!(explorer_argument(Path::new("D:/a/b.mkv"), false), r#"/select,"D:/a/b.mkv""#);
        assert_eq!(explorer_argument(Path::new("D:/a"), true), r#""D:/a""#);
    }

    #[test]
    fn a_name_with_a_comma_in_it_stays_one_argument() {
        // The whole reason for the quotes. Unquoted, Explorer takes everything
        // after the comma as a second argument, finds it is not a path, and
        // opens the user's Documents — which is a wrong window, not an error.
        let argument = explorer_argument(Path::new("D:/Some, Film (2011)/a.mkv"), false);
        assert_eq!(argument, r#"/select,"D:/Some, Film (2011)/a.mkv""#);
        assert_eq!(argument.matches('"').count(), 2, "the path is wrapped once and only once");
    }

    #[test]
    fn a_name_with_spaces_stays_one_argument_too() {
        let argument = explorer_argument(Path::new("D:/A Long Name/an episode.mkv"), false);
        assert!(argument.starts_with("/select,\""), "no space after the comma: {argument}");
        assert!(argument.ends_with('"'));
    }
}

//! File associations and URL protocol handlers, per user.
//!
//! Everything is written under `HKCU\Software\Classes`, so no administrator is
//! involved and every key is reversible. Idempotent: it rewrites only when the
//! executable path has changed — first run, or after an update moved it.
//!
//! **It refuses to register from a build directory.** Claiming `magnet:`
//! system-wide means every magnet link in every browser comes here, and pointing
//! that at `target\release\zerem.exe` would break the moment the file moved or
//! was rebuilt. A developer running from a checkout gets nothing registered, and
//! is told why.

use std::path::Path;

/// What a registration asks for.
pub struct Registration<'a> {
    /// Used for the ProgID prefix and shown in Explorer's "Open with".
    pub app: &'a str,
    /// Extensions without the dot.
    pub extensions: &'a [&'a str],
    /// URL schemes without the colon — `magnet`, say.
    pub protocols: &'a [&'a str],
    /// What Explorer calls the file type.
    pub description: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Registered,
    /// The registry already points at this executable.
    AlreadyCurrent,
    /// Running from somewhere that is not an installation.
    NotInstalled,
    Failed,
}

/// Register, unless this is not an installed copy.
#[must_use]
pub fn ensure_registered(reg: &Registration) -> Outcome {
    imp::ensure_registered(reg)
}

/// Whether `exe` looks like an installed copy rather than a build output.
///
/// The rule is the installer's own target — `%LOCALAPPDATA%\Programs\` on
/// Windows — because that is the one place the path is stable. A `target`
/// component anywhere in the path is a build directory whatever else it looks
/// like, and is refused outright.
#[must_use]
pub fn looks_installed(exe: &Path, install_root: Option<&Path>) -> bool {
    if exe.components().any(|c| c.as_os_str() == "target") {
        return false;
    }
    install_root.is_some_and(|root| exe.starts_with(root))
}

#[cfg(windows)]
mod imp {
    use super::{looks_installed, Outcome, Registration};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
        REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    };
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Read a string under HKCU. `name` of `None` reads the key's default value.
    fn read_string(subkey: &str, name: Option<&str>) -> Option<String> {
        let sub = wide(subkey);
        let nm = name.map(wide);
        let nmptr = nm.as_ref().map_or(PCWSTR::null(), |w| PCWSTR(w.as_ptr()));
        let mut buf = [0_u16; 1024];
        let mut cb = (buf.len() * 2) as u32;
        // SAFETY: `buf` and `cb` are live, and the call writes at most `cb` bytes.
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                nmptr,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&raw mut cb),
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        // `cb` counts bytes and includes the terminator.
        let len = (cb as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    fn write_string(subkey: &str, name: Option<&str>, value: &str) -> bool {
        let sub = wide(subkey);
        let mut hkey = HKEY::default();
        // SAFETY: ordinary key creation under HKCU; `hkey` receives the handle.
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &raw mut hkey,
                None,
            )
        };
        if rc != ERROR_SUCCESS {
            return false;
        }
        let val = wide(value);
        let nm = name.map(wide);
        let nmptr = nm.as_ref().map_or(PCWSTR::null(), |w| PCWSTR(w.as_ptr()));
        // SAFETY: `val` is a live NUL-terminated UTF-16 buffer and the byte view spans it.
        let bytes = unsafe { std::slice::from_raw_parts(val.as_ptr().cast::<u8>(), val.len() * 2) };
        // SAFETY: `hkey` is valid until closed just below.
        let rc2 = unsafe { RegSetValueExW(hkey, nmptr, None, REG_SZ, Some(bytes)) };
        // SAFETY: closing the handle we opened.
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        rc2 == ERROR_SUCCESS
    }

    pub fn ensure_registered(reg: &Registration) -> Outcome {
        let Ok(exe) = std::env::current_exe() else { return Outcome::Failed };
        let install_root = dirs::data_local_dir().map(|d| d.join("Programs"));
        if !looks_installed(&exe, install_root.as_deref()) {
            tracing::debug!(
                exe = %exe.display(),
                "not an installed copy; leaving the system's magnet handler alone"
            );
            return Outcome::NotInstalled;
        }

        let exe = exe.to_string_lossy().replace('/', "\\");
        let command = format!("\"{exe}\" \"%1\"");
        let icon = format!("\"{exe}\",0");

        // One probe rather than a full rewrite every launch. If the first ProgID
        // already points here, so do the rest — they are written together.
        let probe = reg
            .extensions
            .first()
            .map(|ext| format!("Software\\Classes\\{}.{ext}\\shell\\open\\command", reg.app))
            .or_else(|| {
                reg.protocols.first().map(|p| format!("Software\\Classes\\{p}\\shell\\open\\command"))
            });
        if let Some(probe) = probe {
            if read_string(&probe, None).as_deref() == Some(command.as_str()) {
                return Outcome::AlreadyCurrent;
            }
        }

        for ext in reg.extensions {
            let progid = format!("{}.{ext}", reg.app);
            let base = format!("Software\\Classes\\{progid}");
            write_string(&base, None, reg.description);
            write_string(&format!("{base}\\DefaultIcon"), None, &icon);
            if !write_string(&format!("{base}\\shell\\open\\command"), None, &command) {
                return Outcome::Failed;
            }
            write_string(&format!("Software\\Classes\\.{ext}"), None, &progid);
            // Listed under "Open with" even where another app owns the default.
            write_string(&format!("Software\\Classes\\.{ext}\\OpenWithProgids"), Some(&progid), "");
        }

        for scheme in reg.protocols {
            let base = format!("Software\\Classes\\{scheme}");
            write_string(&base, None, &format!("URL:{scheme}"));
            // The empty value under this exact name is what marks a key as a URL
            // protocol handler. Without it Windows ignores the whole entry.
            write_string(&base, Some("URL Protocol"), "");
            write_string(&format!("{base}\\DefaultIcon"), None, &icon);
            if !write_string(&format!("{base}\\shell\\open\\command"), None, &command) {
                return Outcome::Failed;
            }
        }

        // Tell the shell so icons and "Open with" refresh now rather than at the
        // next sign-in.
        // SAFETY: the documented null-item form only broadcasts the change.
        unsafe {
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        }
        Outcome::Registered
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Outcome, Registration};

    /// Not implemented here yet.
    ///
    /// On freedesktop this is a `.desktop` entry with `MimeType=` and
    /// `x-scheme-handler/magnet`, installed by the packaging rather than written
    /// by the running program — so it belongs with the Linux installer, not here.
    pub fn ensure_registered(_reg: &Registration) -> Outcome {
        Outcome::NotInstalled
    }
}

#[cfg(test)]
mod tests {
    use super::looks_installed;
    use std::path::Path;

    #[test]
    fn a_build_output_never_counts_as_installed() {
        // The important half. Registering `magnet:` from a checkout would point
        // every magnet link in the system at a file that moves.
        let root = Path::new("/home/u/.local/share/Programs");
        assert!(!looks_installed(Path::new("/home/u/code/zerem/target/release/zerem"), Some(root)));
        // Even inside the install root, a `target` component is refused.
        assert!(!looks_installed(Path::new("/home/u/.local/share/Programs/target/zerem"), Some(root)));
    }

    #[test]
    fn only_the_install_root_counts() {
        let root = Path::new("/home/u/.local/share/Programs");
        assert!(looks_installed(Path::new("/home/u/.local/share/Programs/Zerem/zerem"), Some(root)));
        assert!(!looks_installed(Path::new("/opt/zerem/zerem"), Some(root)));
    }

    #[test]
    fn no_install_root_means_nothing_is_installed() {
        // A system with no local data directory registers nothing rather than
        // guessing at a path.
        assert!(!looks_installed(Path::new("/anywhere/zerem"), None));
    }
}

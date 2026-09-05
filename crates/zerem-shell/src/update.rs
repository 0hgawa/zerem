//! Replacing the running binary with a newer one, and coming back up as it.
//!
//! What is *not* here: the feed, the URL, the public key and the signature
//! check. Those name a particular application and a particular publisher, and
//! this crate names neither. What is here is the half every self-updating
//! desktop app has to solve identically — whether an in-place swap could work
//! at all, doing the swap, and getting the new image running once the old one
//! is out of the way.
//!
//! # Why the relaunch is not just a spawn
//!
//! Because an app that allows one instance cannot start its own replacement.
//! The new process reaches the single-instance guard while the old one is still
//! answering it, is told a copy is already running, hands over its argument and
//! exits — leaving the user with the window closing, no window opening, and the
//! old binary still in charge as far as they can tell. So the replacement is
//! told which process to outlive, and waits.

use std::path::Path;
use std::process::Command;

/// How this build is installed, which decides whether it can update itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// The binary sits in a directory this process can write to. A swap works.
    Itself,
    /// Somebody else owns the binary — a package manager, an administrator.
    Managed,
    /// A sandbox whose runtime does the updating.
    Sandboxed,
}

/// Whether an in-place swap could work here.
///
/// Offering an update that cannot be applied is worse than offering none: the
/// user clicks, waits out a download, and is handed an error about their own
/// filesystem that they can do nothing about. This is what keeps the button
/// from being drawn when the swap could never have worked.
///
/// Answered once and remembered. A running binary does not move, and the probe
/// touches the filesystem — not worth repeating every time a panel opens.
#[must_use]
pub fn install_kind() -> Install {
    static KIND: std::sync::OnceLock<Install> = std::sync::OnceLock::new();
    *KIND.get_or_init(|| {
        // A Flatpak sandbox mounts its own metadata at the root. The app lives
        // in a read-only `/app` there and updating it belongs to the runtime.
        if Path::new("/.flatpak-info").exists() {
            return Install::Sandboxed;
        }
        // Everything else reduces to one question: can this process create a
        // file in the directory holding the binary? That is precisely what the
        // swap needs, since it stages the replacement there before renaming it
        // over. Asking the filesystem beats matching path prefixes, which would
        // misjudge both a user-owned /opt install and a root-owned ~/.local/bin.
        std::env::current_exe().map_or(Install::Managed, |exe| {
            if writable_beside(&exe) {
                Install::Itself
            } else {
                Install::Managed
            }
        })
    })
}

/// Counts probes, so no two of them are ever the same file.
///
/// Both halves of the name earn their place. The pid separates two copies of
/// the app installed in one directory; this separates two threads of one copy,
/// which share a pid — and two probes on one name is one thread deleting the
/// file the other has not looked for yet.
static PROBES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Whether a new file can be created beside `exe`.
fn writable_beside(exe: &Path) -> bool {
    let Some(dir) = exe.parent() else { return false };
    let nth = PROBES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let probe = dir.join(format!(".write-probe-{}-{nth}", std::process::id()));
    std::fs::File::create(&probe).is_ok_and(|_| {
        let _ = std::fs::remove_file(&probe);
        true
    })
}

/// Put `staged` in place of the running executable.
///
/// The bytes are the caller's business, and so is whether they were worth
/// trusting; by the time this is called that question is settled.
///
/// # Errors
///
/// When the swap fails, with a sentence saying what the user can do about it.
pub fn swap(staged: &Path) -> Result<(), String> {
    self_replace::self_replace(staged).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            // Unusual on Windows, where the install is per-user. On Unix the
            // binary is as likely to sit under `/usr`, owned by root — where no
            // retry will ever succeed and the honest answer is to say so.
            "no permission to replace the running program — it is installed for everybody, so update it through your package manager or download the release by hand".to_owned()
        } else {
            format!("could not replace the running program: {e}")
        }
    })
}

/// Start the replacement, telling it to outlive this process first.
///
/// The caller then quits. The instruction travels in the environment rather
/// than in an argument because an argument is exactly what a single-instance
/// guard forwards: a `--wait-for` on the command line would be handed to the
/// very process being waited for, which would try to open it as a torrent.
pub fn relaunch(handoff: &str) {
    let Ok(exe) = std::env::current_exe() else { return };
    let mut child = Command::new(exe);
    child.env(handoff, std::process::id().to_string());
    // Detached, so the new image is not tied to this process's console or job.
    // A Unix child already outlives its parent, so there is nothing to add.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        child.creation_flags(DETACHED_PROCESS);
    }
    if let Err(e) = child.spawn() {
        tracing::error!(error = %e, "could not start the updated program");
    }
}

/// Longest a replacement waits for the process it replaced.
///
/// Generous, because the cost of being wrong is lopsided: waiting a moment too
/// long is invisible, and giving up too early is the single-instance guard
/// handing this process's window to a binary that is about to vanish. Anything
/// still alive after ten seconds is not shutting down, and the guard's own
/// takeover — it terminates an owner that will not answer — is the right
/// mechanism from that point on.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);

/// Wait for the process named in `handoff`, if this one was started by it.
///
/// Call before acquiring the single-instance name; that is the whole point of
/// waiting. Returns immediately for an ordinary launch, which is every launch
/// but the one that follows an update.
pub fn settle(handoff: &str) {
    let Ok(pid) = std::env::var(handoff) else { return };
    // Removed so it is not inherited by anything this process starts later, and
    // so two updates in one session cannot leave the second waiting on a pid
    // that went away with the first.
    std::env::remove_var(handoff);
    if let Ok(pid) = pid.parse::<u32>() {
        imp::wait_for(pid);
    }
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_ACCESS_RIGHTS};

    /// The right to wait on a handle, and the only one wanted here — a process
    /// this one did not start is not something to open for reading or, worse,
    /// for terminating. windows-rs types the constant as a *file* access right,
    /// so the number is restated rather than cast across.
    const SYNCHRONIZE: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);

    pub fn wait_for(pid: u32) {
        // A process that cannot be opened has already gone, which is the answer
        // this was waiting for.
        let Ok(process) = (unsafe { OpenProcess(SYNCHRONIZE, false, pid) }) else { return };
        let waited = unsafe { WaitForSingleObject(process, super::PATIENCE.as_millis() as u32) };
        unsafe {
            let _ = CloseHandle(process);
        }
        if waited == WAIT_TIMEOUT {
            tracing::warn!(pid, "the replaced program is still running; starting anyway");
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::time::Instant;

    /// No waiting on a process that is not this one's child, so the portable
    /// question is asked instead: is `/proc/<pid>` still there? A poll rather
    /// than a signal, because signalling a pid this process does not own is not
    /// something to do for tidiness.
    pub fn wait_for(pid: u32) {
        let deadline = Instant::now() + super::PATIENCE;
        let alive = std::path::PathBuf::from(format!("/proc/{pid}"));
        while alive.exists() && Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{install_kind, settle, writable_beside, Install};

    #[test]
    fn the_test_binary_can_write_beside_itself() {
        // Cargo's target directory is the developer's own, so the answer here
        // is `Itself` and the probe is what says so. It is the branch that
        // decides whether the update button is drawn at all.
        let exe = std::env::current_exe().expect("a running test has a path");
        assert!(writable_beside(&exe));
        assert_eq!(install_kind(), Install::Itself);
    }

    #[test]
    fn the_probe_leaves_nothing_behind() {
        // It creates a file beside the binary, and one left there would turn up
        // in the install directory of everybody who ever opened the About
        // panel. Asked in a folder of this test's own rather than beside the
        // real binary: the sibling test probes there too, at the same moment,
        // and "the folder is empty" is only an answer where nothing else is
        // working.
        let dir = std::env::temp_dir().join(format!("zerem-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("make a scratch folder");

        assert!(writable_beside(&dir.join("pretend.exe")), "a folder just created is writable");
        let left = std::fs::read_dir(&dir).expect("list the folder").count();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(left, 0, "the probe was left behind");
    }

    #[test]
    fn an_ordinary_launch_does_not_wait() {
        // No variable set is every launch but the one after an update, and it
        // has to cost nothing.
        settle("ZEREM_TEST_HANDOFF_ABSENT");
    }

    #[test]
    fn the_handoff_is_consumed() {
        // Left set, it would be inherited by everything the app starts, and a
        // second update in one session would wait on a pid long gone. The pid
        // used cannot be opened, so nothing is actually waited for.
        const KEY: &str = "ZEREM_TEST_HANDOFF_CONSUMED";
        std::env::set_var(KEY, "4294967295");
        settle(KEY);
        assert!(std::env::var(KEY).is_err(), "the handoff outlived the launch it was for");
    }
}

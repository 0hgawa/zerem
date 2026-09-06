//! A running program replacing itself, done for real.
//!
//! This is the third of the update that had never once been executed. The feed
//! is parsed and tested, the signature is verified against a real signed
//! fixture, and then [`zerem_shell::update::swap`] was taken on faith — because
//! running it means replacing the binary that is running, and no unit test can
//! do that to its own harness.
//!
//! It is also the step that cannot be fixed afterwards. Everything before it
//! fails safely: a bad feed is ignored, a bad signature is refused, and the app
//! carries on being the version it was. A swap that does not work is a download
//! every installed copy has already made, and a replacement that never happens
//! — or worse, half happens.
//!
//! So: copy this test binary somewhere, give the copy a marker that says which
//! one it is, run it, and have it replace itself with a second copy carrying a
//! different marker. Then read the bytes back. The child is this same binary
//! asked to run one named test, which is how it knows to be the child.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Tells the child which file to become. Its presence is what makes it the
/// child at all.
const BECOME: &str = "ZEREM_TEST_SWAP_TO";

/// Tells a child to stay alive for a while, so there is something to wait for.
const LINGER: &str = "ZEREM_TEST_LINGER_MS";

/// The name `settle` reads, matching what the app passes it.
const HANDOFF: &str = "ZEREM_RELAUNCH_AFTER";

/// Every test here reads or writes the environment, and the environment is one
/// table shared by every thread cargo runs them on.
///
/// Not a theoretical worry: `set_var` while another thread is inside `var` is a
/// data race in the C library underneath, and the failure it produces is a test
/// that passes until the day it does not.
fn alone<T>(body: impl FnOnce() -> T) -> T {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    body()
}

/// Appended to a copy so the two are different files that both still run.
///
/// Bytes after the end of a PE or ELF image are overlay: the loader ignores
/// them and the program starts exactly as it did. It is the smallest way to
/// have two binaries that differ, without building a second one.
fn marked(from: &Path, to: &Path, mark: &[u8]) {
    let mut bytes = std::fs::read(from).expect("read the binary");
    bytes.extend_from_slice(mark);
    std::fs::write(to, bytes).expect("write the copy");

    // The copy has to be executable, which on Unix a plain write is not.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o755)).expect("make the copy runnable");
    }
}

fn tail(of: &Path, len: usize) -> Vec<u8> {
    let mut bytes = std::fs::read(of).expect("read it back");
    bytes.split_off(bytes.len().saturating_sub(len))
}

/// The child half. Does nothing at all unless it was told what to become, which
/// is every run but the one the test below starts.
#[test]
fn a_copy_of_this_binary_replaces_itself_when_told_to() {
    let told = alone(|| std::env::var(BECOME));
    let Ok(staged) = told else { return };
    zerem_shell::update::swap(Path::new(&staged)).expect("the swap should succeed");
}

/// The other child half: stays alive, so `settle` has something real to block
/// on. An ordinary run of the suite sets nothing and this returns at once.
#[test]
fn a_copy_of_this_binary_lingers_when_told_to() {
    let told = alone(|| std::env::var(LINGER));
    let Ok(ms) = told else { return };
    std::thread::sleep(Duration::from_millis(ms.parse().unwrap_or_default()));
}

/// The second half of the handoff, and the second thing that had never run.
///
/// After the swap the old process starts the new image and quits, and the new
/// one must not take the single-instance name until the old one is gone. If it
/// does, the guard hands this process's window to a binary that is about to
/// vanish — the update appears to work and the window disappears.
///
/// So `settle` has to actually block. On Windows that is a `WaitForSingleObject`
/// on a handle opened with `SYNCHRONIZE` and nothing else, which is exactly the
/// sort of call that returns immediately when it is subtly wrong.
#[test]
fn settling_waits_for_the_program_that_started_this_one() {
    let exe = std::env::current_exe().expect("this test binary");
    let mut lingering = Command::new(exe)
        .args(["--exact", "a_copy_of_this_binary_lingers_when_told_to", "--nocapture"])
        .env(LINGER, "700")
        .spawn()
        .expect("start something to wait for");

    let waited = alone(|| {
        std::env::set_var(HANDOFF, lingering.id().to_string());
        let began = Instant::now();
        zerem_shell::update::settle(HANDOFF);
        let waited = began.elapsed();
        assert!(std::env::var(HANDOFF).is_err(), "the handoff must be consumed, not inherited");
        waited
    });

    // Generously under the 700 ms it sleeps, because a process that has to
    // start a test harness first takes longer than that, never less.
    assert!(
        waited >= Duration::from_millis(400),
        "settle returned after {waited:?}, so it did not wait at all"
    );
    assert!(waited < Duration::from_secs(10), "settle waited past its own patience: {waited:?}");
    let _ = lingering.wait();
}

#[test]
fn a_running_program_can_replace_itself() {
    let exe = std::env::current_exe().expect("this test binary");
    let yard = std::env::temp_dir().join(format!("zerem-swap-{}", std::process::id()));
    std::fs::create_dir_all(&yard).expect("a place to work");

    let running: PathBuf = yard.join(format!("running{}", std::env::consts::EXE_SUFFIX));
    let staged: PathBuf = yard.join(format!("staged{}", std::env::consts::EXE_SUFFIX));
    marked(&exe, &running, b"-- the old version --");
    marked(&exe, &staged, b"-- the new version --");
    assert_ne!(tail(&running, 21), tail(&staged, 21), "the two copies must differ to prove anything");

    // The copy, running, asked to run the one test above and nothing else.
    let done = Command::new(&running)
        .args(["--exact", "a_copy_of_this_binary_replaces_itself_when_told_to", "--nocapture"])
        .env(BECOME, &staged)
        .output()
        .expect("start the copy");

    assert!(
        done.status.success(),
        "the copy could not replace itself:\n{}\n{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );

    // The whole claim: the file that was running is now the other one.
    assert_eq!(tail(&running, 21), b"-- the new version --".to_vec(), "the running program was not replaced");
    assert!(running.exists(), "and it is still there to be run again");

    let _ = std::fs::remove_dir_all(&yard);
}

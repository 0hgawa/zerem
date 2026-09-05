//! The update row in About: checking, installing, and coming back as the new
//! build.
//!
//! Every one of these runs the network on a worker thread and returns to the
//! event loop to say what happened. The check is one small request and the
//! download is a whole binary; either on the UI thread would freeze the window,
//! and the second would freeze it for as long as the user's connection takes.
//!
//! # Why the automatic check is quiet and the manual one is not
//!
//! A check nobody asked for that fails has nothing to tell anybody. The usual
//! reason is that the machine has no network yet — a laptop opened before the
//! wifi associates — and "Could not update: error sending request" is a
//! sentence about a question the user never asked. So the automatic check falls
//! back to saying nothing, and only a press of the button reports a failure.

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle as _;

use crate::update::{self, Available};
use crate::{MainWindow, Prefs, UpdateState};

/// How long after launch the quiet check runs.
///
/// Late enough to be behind the session coming up and the window drawing, which
/// is what the user is waiting for. Nothing about an update is urgent — the
/// build they are running was current a moment ago.
const SETTLE: std::time::Duration = std::time::Duration::from_secs(4);

thread_local! {
    /// What the last check found, waiting for Install to be pressed.
    ///
    /// A thread-local rather than something shared: it is only ever touched on
    /// the UI thread, and that keeps the worker closures `Send` without an Arc
    /// and a lock around a value that is never contended.
    static FOUND: RefCell<Option<Available>> = const { RefCell::new(None) };
}

pub fn wire(ui: &MainWindow) {
    let prefs = ui.global::<Prefs>();

    // Settled once, because a running program does not move. It decides whether
    // the button is drawn at all, so it has to be in place before the panel can
    // be opened.
    prefs.set_install_kind(
        match zerem_shell::install_kind() {
            zerem_shell::Install::Itself => "self",
            zerem_shell::Install::Managed => "managed",
            zerem_shell::Install::Sandboxed => "sandboxed",
        }
        .into(),
    );

    prefs.on_check_update({
        let ui = ui.as_weak();
        move || check(&ui, Report::Yes)
    });

    prefs.on_install_update({
        let ui = ui.as_weak();
        move || {
            let Some(window) = ui.upgrade() else { return };
            // Nothing found, nothing to install. Reachable if the panel is left
            // open across a reset, and doing nothing is the whole handling.
            let Some(update) = FOUND.with_borrow(Clone::clone) else { return };
            window.global::<Prefs>().set_update(UpdateState::Installing);

            let back = ui.clone();
            std::thread::spawn(move || {
                let outcome = update::fetch_and_apply(&update);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(window) = back.upgrade() else { return };
                    let prefs = window.global::<Prefs>();
                    match outcome {
                        Ok(()) => prefs.set_update(UpdateState::Installed),
                        Err(why) => {
                            prefs.set_update_detail(why.into());
                            prefs.set_update(UpdateState::Failed);
                        }
                    }
                });
            });
        }
    });

    prefs.on_relaunch({
        let ui = ui.as_weak();
        move || {
            // Started before this one quits, and told to wait: the replacement
            // has to outlive us before it can take the single-instance name, or
            // it hands its window back to a process that is on its way out.
            zerem_shell::update::relaunch(update::HANDOFF);
            if let Some(window) = ui.upgrade() {
                let _ = window.window().hide();
            }
            // The real quit. Closing the window only hides it here — a torrent
            // client that stopped seeding on a close would be a worse app — so
            // the event loop has to be ended by name.
            let _ = slint::quit_event_loop();
        }
    });

    prefs.on_open_releases(update::release_page);
}

/// Whether a failure is worth putting in front of somebody.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Report {
    Yes,
    No,
}

/// Ask the feed, on a worker, and come back to say what it answered.
fn check(ui: &slint::Weak<MainWindow>, report: Report) {
    let Some(window) = ui.upgrade() else { return };
    window.global::<Prefs>().set_update(UpdateState::Checking);

    let back = ui.clone();
    std::thread::spawn(move || {
        let outcome = update::check();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(window) = back.upgrade() else { return };
            let prefs = window.global::<Prefs>();
            match outcome {
                Ok(Some(update)) => {
                    prefs.set_update_detail(update.version.as_str().into());
                    FOUND.with_borrow_mut(|slot| *slot = Some(update));
                    prefs.set_update(UpdateState::Available);
                }
                Ok(None) => prefs.set_update(UpdateState::Current),
                Err(why) if report == Report::Yes => {
                    prefs.set_update_detail(why.into());
                    prefs.set_update(UpdateState::Failed);
                }
                // A check nobody asked for, and no network to answer it. Back
                // to the line that offers one, rather than an error about a
                // question the user never put.
                Err(why) => {
                    tracing::debug!("the quiet update check did not get through: {why}");
                    prefs.set_update(UpdateState::Idle);
                }
            }
        });
    });
}

/// Look for a newer build shortly after launch, without saying so.
///
/// Only where an update could actually be installed. On a package manager's
/// copy the answer changes nothing anybody can act on from this window, and the
/// panel says as much without a request being made.
pub fn watch_for_new_builds(ui: &MainWindow) -> Rc<slint::Timer> {
    let timer = Rc::new(slint::Timer::default());
    if zerem_shell::install_kind() != zerem_shell::Install::Itself {
        return timer;
    }
    let ui = ui.as_weak();
    timer.start(slint::TimerMode::SingleShot, SETTLE, move || check(&ui, Report::No));
    timer
}

//! The tray icon, and what closing the window means.
//!
//! Closing hides rather than quits. A torrent client that stops seeding because
//! its window was in the way is doing the wrong thing, and every client worth
//! using behaves this way. The tray menu is the way out, and the reason it is
//! not merely tolerable here is the work already done: a hidden window stops the
//! UI tick entirely — Phase 0 measured that state at 0.000 % CPU — while the
//! engine keeps transferring.

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use slint::ComponentHandle;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::MainWindow;

/// How often the tray's event channels are drained.
///
/// `tray-icon` delivers through its own queues rather than winit's, so something
/// has to pump them. 100 ms is under the threshold where a menu click feels
/// delayed, and the poll itself is two `try_recv` calls on empty channels.
const PUMP: Duration = Duration::from_millis(100);

const SIZE: u32 = 32;

/// The tray mark. The same rasteriser the build script uses for the executable
/// icon, so the two cannot drift.
fn icon() -> Icon {
    Icon::from_rgba(zerem_core::icon::rgba(SIZE), SIZE, SIZE)
        .expect("the icon is generated, so it is always well-formed")
}

/// Whether there is a tray icon to come back from.
///
/// A process-wide fact settled once at startup, read later by whatever decides
/// what the close button does. It has to be readable from a closure registered
/// *before* `install` runs -- the bridge wires the window before the tray is
/// built -- and by the time that closure fires the answer is final.
static PRESENT: AtomicBool = AtomicBool::new(false);

/// Whether closing the window can safely hide it.
#[must_use]
pub fn present() -> bool {
    PRESENT.load(Ordering::Relaxed)
}

/// Install the tray icon and make the window's close button hide it.
///
/// The returned handle has to outlive the event loop: dropping a `TrayIcon`
/// removes it from the tray.
///
/// # Nothing here is fatal
///
/// It used to be: a tray that would not build took the whole application down
/// at startup. On Windows that happens for a reason nobody chose -- the shell
/// restarting takes the notification area with it for a second or two -- and
/// dying because of it is a worse answer than starting without a tray.
///
/// What made the panic defensible was the coupling: closing the window hides
/// it, and hiding with nowhere to come back from strands somebody with a
/// process they cannot reach. So the coupling is stated instead of assumed, and
/// `present` is what states it. Without a tray, closing quits.
pub fn install(ui: &MainWindow) -> Option<(TrayIcon, slint::Timer)> {
    let show = MenuItem::new("Show Zerem", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let (show_id, quit_id) = (show.id().clone(), quit.id().clone());

    let menu = Menu::new();
    if let Err(e) = menu.append_items(&[&show, &quit]) {
        tracing::warn!(error = %e, "no tray menu; the close button will quit instead");
        return None;
    }

    let tray = match TrayIconBuilder::new()
        .with_tooltip("Zerem")
        .with_icon(icon())
        .with_menu(Box::new(menu))
        .build()
    {
        Ok(tray) => tray,
        Err(e) => {
            tracing::warn!(error = %e, "no tray icon; the close button will quit instead");
            return None;
        }
    };
    PRESENT.store(true, Ordering::Relaxed);

    ui.window().on_close_requested({
        let window = ui.as_weak();
        move || {
            // Hidden, not closed. The engine keeps going; the UI tick stops.
            // This is only ever registered once the tray is up, so there is
            // always somewhere to come back from.
            if let Some(ui) = window.upgrade() {
                let _ = ui.window().hide();
            }
            slint::CloseRequestResponse::HideWindow
        }
    });

    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, PUMP, {
        let window = ui.as_weak();
        let ids = Rc::new((show_id, quit_id));
        move || {
            let Some(ui) = window.upgrade() else { return };

            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == ids.0 {
                    let _ = ui.window().show();
                } else if event.id == ids.1 {
                    // The only way out. Everything worth keeping is already on
                    // disk — librqbit persists the session as it changes.
                    let _ = slint::quit_event_loop();
                }
            }

            // A left click on the icon toggles, which is what people try first.
            while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                let TrayIconEvent::Click { button, button_state, .. } = event else { continue };
                if button != MouseButton::Left || button_state != MouseButtonState::Up {
                    continue;
                }
                if ui.window().is_visible() {
                    let _ = ui.window().hide();
                } else {
                    let _ = ui.window().show();
                }
            }
        }
    });

    Some((tray, timer))
}

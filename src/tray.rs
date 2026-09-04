//! The tray icon, and what closing the window means.
//!
//! Closing hides rather than quits. A torrent client that stops seeding because
//! its window was in the way is doing the wrong thing, and every client worth
//! using behaves this way. The tray menu is the way out, and the reason it is
//! not merely tolerable here is the work already done: a hidden window stops the
//! UI tick entirely — Phase 0 measured that state at 0.000 % CPU — while the
//! engine keeps transferring.

use std::rc::Rc;
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

/// Install the tray icon and make the window's close button hide it.
///
/// The returned handle has to outlive the event loop: dropping a `TrayIcon`
/// removes it from the tray.
#[must_use]
pub fn install(ui: &MainWindow) -> (TrayIcon, slint::Timer) {
    let show = MenuItem::new("Show Zerem", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let (show_id, quit_id) = (show.id().clone(), quit.id().clone());

    let menu = Menu::new();
    menu.append_items(&[&show, &quit]).expect("build the tray menu");

    let tray = TrayIconBuilder::new()
        .with_tooltip("Zerem")
        .with_icon(icon())
        .with_menu(Box::new(menu))
        .build()
        .expect("build the tray icon");

    ui.window().on_close_requested({
        let window = ui.as_weak();
        move || {
            // Hidden, not closed. The engine keeps going; the UI tick stops.
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

    (tray, timer)
}

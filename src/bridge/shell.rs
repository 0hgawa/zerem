//! The window doing what the title bar used to do for it.
//!
//! Removing the frame removed the operating system's part in moving, sizing,
//! minimising and closing a window. Every callback here is one of those,
//! handed back.
//!
//! # Logical and physical, which is the whole of the arithmetic
//!
//! A drag arrives in logical pixels — what the layout is written in — and a
//! window's position and size are physical. At 100 % they are the same number
//! and every mistake here is invisible; at 150 % the window moves two thirds as
//! far as the cursor and slides out from under it. So every delta is scaled,
//! once, at the boundary.

use slint::ComponentHandle;

use crate::{MainWindow, Shell};

/// Which edges a resize is pulling. The same bits `Grip` is given in the
/// `.slint`, named here so the arithmetic below reads.
const LEFT: i32 = 1;
const RIGHT: i32 = 2;
const TOP: i32 = 4;
const BOTTOM: i32 = 8;

/// The smallest a window may be dragged to.
///
/// The layout has its own minimum and Slint enforces it on the *window*, but a
/// resize computed here would still walk the far edge past the near one first
/// and only then be clamped — which looks like the window flipping inside out.
const FLOOR: u32 = 480;

pub fn wire(ui: &MainWindow) {
    let shell = ui.global::<Shell>();

    shell.on_minimize({
        let ui = ui.as_weak();
        move || {
            if let Some(ui) = ui.upgrade() {
                ui.window().set_minimized(true);
            }
        }
    });

    shell.on_close({
        let ui = ui.as_weak();
        move || {
            // The same thing the frame's close did: hidden, not closed. The
            // engine keeps going and the tray brings it back — the button lost
            // its frame, not its meaning.
            if let Some(ui) = ui.upgrade() {
                let _ = ui.window().hide();
            }
        }
    });

    shell.on_toggle_maximize({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let window = ui.window();
            let next = !window.is_maximized();
            window.set_maximized(next);
            ui.global::<Shell>().set_maximized(next);
        }
    });

    shell.on_move_by({
        let ui = ui.as_weak();
        move |dx, dy| {
            let Some(ui) = ui.upgrade() else { return };
            let window = ui.window();
            let scale = window.scale_factor();
            let at = window.position();
            window.set_position(slint::PhysicalPosition::new(
                at.x + (dx * scale) as i32,
                at.y + (dy * scale) as i32,
            ));
        }
    });

    shell.on_resize_by({
        let ui = ui.as_weak();
        move |edges, dx, dy| {
            let Some(ui) = ui.upgrade() else { return };
            let window = ui.window();
            let scale = window.scale_factor();
            let (dx, dy) = ((dx * scale) as i32, (dy * scale) as i32);

            let at = window.position();
            let size = window.size();
            let (mut x, mut y) = (at.x, at.y);
            let (mut w, mut h) = (size.width as i32, size.height as i32);

            // A near edge moves the window as well as sizing it: without the
            // move, pulling the left edge leftwards would grow the window to
            // the *right* and the edge would run away from the cursor.
            if edges & LEFT != 0 {
                w -= dx;
                x += dx;
            }
            if edges & RIGHT != 0 {
                w += dx;
            }
            if edges & TOP != 0 {
                h -= dy;
                y += dy;
            }
            if edges & BOTTOM != 0 {
                h += dy;
            }

            let floor = FLOOR as i32;
            // Clamped before it is applied, and the origin put back with it: a
            // left edge that has hit the floor must stop moving too, or the
            // window slides sideways while refusing to shrink.
            if w < floor {
                if edges & LEFT != 0 {
                    x -= floor - w;
                }
                w = floor;
            }
            if h < floor / 2 {
                if edges & TOP != 0 {
                    y -= floor / 2 - h;
                }
                h = floor / 2;
            }

            window.set_size(slint::PhysicalSize::new(w as u32, h as u32));
            window.set_position(slint::PhysicalPosition::new(x, y));
        }
    });
}

/// Keep the maximise button honest about which glyph it is.
///
/// The window can be maximised by something other than that button — a snap
/// from the keyboard, the taskbar's own menu — and a button showing "maximise"
/// on a maximised window is a button that lies.
pub fn refresh(ui: &MainWindow) {
    let maximized = ui.window().is_maximized();
    let shell = ui.global::<Shell>();
    if shell.get_maximized() != maximized {
        shell.set_maximized(maximized);
    }
}

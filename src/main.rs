// Zerem — native BitTorrent client.
//
// One process, one window, no WebView. The engine runs on its own thread and
// publishes an immutable snapshot once a second; the UI diffs it into the table
// and notifies only the rows that moved. State flows down, commands flow up, and
// neither direction takes a shortcut.
//
// `ZEREM_LOG=debug` prints what each tick cost.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString};
use zerem_core::sort;
use zerem_engine::{Command, Engine, EngineConfig};
use zerem_shell as shell;

/// The name the single-instance guard claims. Stable across launches and
/// distinct per application — it is namespaced per user inside the guard.
const APP_ID: &str = "Zerem";

/// Write a UI property only when the value actually differs — the scalar form of
/// the rule the model diff follows.
///
/// Declared before the modules below so it is in scope for them.
macro_rules! push {
    ($ui:expr, $get:ident, $set:ident, $value:expr) => {{
        let next = $value;
        if $ui.$get() != next {
            $ui.$set(next);
        }
    }};
}

mod bridge;
mod model;
mod settings;
mod state;
mod tray;

use state::UiState;

slint::include_modules!();

/// Logging to stderr: warnings and worse by default, `ZEREM_LOG=debug` for the
/// per-tick cost. No env-filter feature on purpose — a plain level filter avoids
/// pulling in the regex engine and the few hundred KiB that come with it.
fn install_tracing() {
    let level = match std::env::var("ZEREM_LOG").as_deref() {
        Ok("trace") => tracing::Level::TRACE,
        Ok("debug") => tracing::Level::DEBUG,
        Ok("info") => tracing::Level::INFO,
        Ok("error") => tracing::Level::ERROR,
        _ => tracing::Level::WARN,
    };
    let _ = tracing_subscriber::fmt().with_max_level(level).with_target(false).compact().try_init();
}

fn main() -> Result<(), slint::PlatformError> {
    install_tracing();

    // Anything on the command line is something to open — that is how Explorer
    // hands over a double-clicked `.torrent`, and how a `magnet:` handler will.
    // Flags are skipped so a future `--something` is not mistaken for a torrent.
    let opening: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();

    // Before anything is opened and before the engine touches the disk. A second
    // copy would share this one's session folder and download folder — two
    // processes writing the same state and the same files.
    let server = match shell::acquire(APP_ID, &opening.join("\n")) {
        shell::Instance::Secondary => return Ok(()),
        shell::Instance::Primary(server) => server,
    };

    // Off-thread so it never delays the window, and a no-op unless this is an
    // installed copy — claiming `magnet:` from a build directory would point
    // every magnet link in the system at a file that moves.
    std::thread::spawn(|| {
        let outcome = shell::ensure_registered(&shell::Registration {
            app: APP_ID,
            extensions: &["torrent"],
            protocols: &["magnet"],
            description: "BitTorrent file",
        });
        tracing::debug!(?outcome, "file associations");
    });

    // Read before the engine starts: the download folder and the listening port
    // are what it is built from.
    let store = Rc::new(settings::Store::open(&EngineConfig::default().state_dir));
    let settings = store.get();

    let ui = MainWindow::new()?;
    let state = Rc::new(UiState::new(Engine::start(settings.to_engine_config())));

    // The limits are the one engine setting that is applied rather than built
    // in, because librqbit takes them while it runs.
    state.engine.send(Command::SetLimits {
        down: bridge::prefs::to_bps(settings.down_limit),
        up: bridge::prefs::to_bps(settings.up_limit),
    });

    let list = ui.global::<TorrentList>();
    list.set_col_w(ModelRc::from(state.widths.clone()));
    list.set_col_visible(ModelRc::from(state.columns.clone()));
    // The headings, once. They come from core so the header and the column
    // menu cannot drift apart, and so the `.slint` invents no text.
    list.set_col_title(ModelRc::from(
        sort::TITLES.iter().map(|&t| SharedString::from(t)).collect::<Vec<_>>().as_slice(),
    ));
    list.set_rows(ModelRc::from(state.model.clone()));
    state.restore_view(&settings);
    ui.global::<TorrentList>().set_rail_open(settings.rail_open);
    bridge::detail::show_width(&ui, &state);
    bridge::prefs::show(&ui, &settings);

    for source in opening {
        state.engine.send(Command::Add { source });
    }

    // What a later launch forwards. It goes straight to the engine rather than
    // through the UI: `Engine` is a `Send` handle, the window is not, and the
    // table picks the new torrent up on its next tick either way.
    server.run({
        let engine = state.engine.clone();
        // `slint::Weak` is `Send` precisely so a background thread can reach the
        // window this way.
        let window = ui.as_weak();
        move |arg| {
            for source in arg.split('\n').filter(|s| !s.is_empty()) {
                tracing::info!(source, "another launch handed this over");
                engine.send(Command::Add { source: source.to_string() });
            }
            // Whatever the second launch was, the user expects a window. It
            // comes back from the tray here; putting it in *front* of whatever
            // has focus is something Windows only grants to the foreground
            // process, so this shows it rather than promising more.
            let _ = window.upgrade_in_event_loop(|ui| {
                let _ = ui.window().show();
            });
            true
        }
    });

    let views = Rc::new(bridge::Views::new(&ui));
    bridge::wire(&ui, &state, &store, &views);

    // All three have to outlive the event loop: dropping a `TrayIcon` removes it
    // from the tray, and dropping a `Timer` stops it. `start_tick` also paints
    // the first frame, before the engine's first heartbeat.
    let _tray = tray::install(&ui);
    let _tick = bridge::start_tick(&ui, &state, &views);

    // After the window exists, not before: asked any earlier it has no size to
    // read and the preferred one is applied afterwards, over the top of the
    // answer. Which is exactly how this looked fixed once and then was not.
    ui.show()?;
    fit_to_desktop(&ui);
    slint::run_event_loop()?;

    // A setting changed a fraction of a second before the window closed is still
    // sitting on a timer the event loop will never run again.
    state.save_view(&store);
    store.flush();
    Ok(())
}

/// Bring the window inside the desktop it is opening on.
///
/// A preferred size is written in logical pixels and a logical pixel is not a
/// pixel: 720 tall at 200 % is 1440 device pixels, which is the whole of a
/// 1440p screen. Zerem opened exactly that way — 1511 px tall on a 1440 px
/// display, with its status bar 169 px below the bottom edge and never once
/// seen in a window that had not been maximised.
///
/// Only ever shrinks. A small desktop is a reason to be smaller; it is not a
/// reason to be larger than asked for.
fn fit_to_desktop(ui: &MainWindow) {
    let window = ui.window();
    let size = window.size();
    let (width, height) = zerem_shell::fit((size.width, size.height), zerem_shell::work_area());
    if (width, height) != (size.width, size.height) {
        tracing::debug!(
            from = ?(size.width, size.height),
            to = ?(width, height),
            "window brought inside the desktop"
        );
        window.set_size(slint::PhysicalSize::new(width, height));
    }
}

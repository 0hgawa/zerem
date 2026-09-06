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

use slint::{ComponentHandle, ModelRc};
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
mod update;

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

    // Before the guard below, which is most of why it is worth waiting at all.
    // A build that has just replaced another one has to see that one gone before
    // it asks for the instance name, or it is told a copy is already running --
    // by the copy it replaced, on its way out -- and exits, leaving the user
    // with no window and, as far as they can tell, no update.
    shell::update::settle(update::HANDOFF);

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
    state.engine.send(bridge::prefs::in_force(&settings));

    let list = ui.global::<TorrentList>();
    list.set_col_w(ModelRc::from(state.widths.clone()));
    list.set_col_visible(ModelRc::from(state.columns.clone()));
    // The headings. From core, so the header and the column menu cannot drift
    // apart and the `.slint` invents no text — and no longer "once": they are
    // words, and a word has to change when the language does.
    bridge::prefs::show_titles(&ui);
    list.set_rows(ModelRc::from(state.model.clone()));
    state.restore_view(&settings);
    state.adopt_shelves(&settings);
    ui.global::<TorrentList>().set_rail_state(i32::from(settings.rail_state.min(2)));
    ui.global::<Theme>().set_density(i32::from(settings.density.min(2)));
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
    // A fourth: the one-shot that looks for a newer build a few seconds in.
    let _updates = bridge::updates::watch_for_new_builds(&ui);

    // After the window exists, not before: asked any earlier it has no size to
    // read and the preferred one is applied afterwards, over the top of the
    // answer. Which is exactly how this looked fixed once and then was not.
    //
    // And on the event loop rather than before it. Between `show` and the first
    // turn of the loop the window exists without being mapped: a size set there
    // lands, a *position* is quietly dropped, and the software renderer keeps
    // dirty regions from a geometry that no longer holds — so the window opened
    // where the desktop put it, with pieces of it missing. Queued here, the
    // first thing the loop does is place the window, before anything is drawn.
    ui.show()?;
    let placing = ui.as_weak();
    slint::invoke_from_event_loop(move || {
        if let Some(ui) = placing.upgrade() {
            fit_to_desktop(&ui);
            // The frame that was removed took the desktop-s rounded corners
            // with it: Windows rounds a window because its *frame* is
            // rounded, and this one has none. Asked back here, once the window
            // exists to be asked about.
            zerem_shell::round_corners();
            // And the mark, from the same drawing the `.ico` and the tray use.
            // The `.slint` cannot do this: its `icon` property is only applied
            // when the image's cache key changes, and an image made at run time
            // has no cache key at all -- so the property is set, nothing
            // notices, and the taskbar falls back to the executable's icon,
            // which Windows caches by path. See `zerem_shell::mark`.
            zerem_shell::wear_mark(zerem_core::icon::rgba);
        }
    })
    .unwrap_or_else(|e| tracing::warn!("could not queue the window placement: {e}"));
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
    let desktop = zerem_shell::work_area();

    let (width, height) = zerem_shell::fit((size.width, size.height), desktop);
    if (width, height) != (size.width, size.height) {
        tracing::debug!(
            from = ?(size.width, size.height),
            to = ?(width, height),
            "window brought inside the desktop"
        );
        window.set_size(slint::PhysicalSize::new(width, height));
    }

    // And in the middle of it. Sized first, because where the middle is depends
    // on how big the window ended up.
    //
    // Every launch, rather than remembering where it was left. A window that
    // comes back where it was is the better behaviour for something opened all
    // day; this is opened, watched, and closed, and "where did it go" on a
    // second monitor that is not there any more is the failure that costs more
    // than the convenience is worth.
    if let Some((x, y)) = zerem_shell::centre((width, height), desktop) {
        window.set_position(slint::PhysicalPosition::new(x, y));
    }
}

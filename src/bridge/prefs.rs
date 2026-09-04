//! Preferences: the download folder and the transfer limits.
//!
//! Only what can change while the app runs. The listening port, uTP and UPnP are
//! in `settings.json` but not in the panel, because they are fixed when the
//! session is built — a control that silently does nothing until the next launch
//! is worse than no control.

use std::rc::Rc;

use slint::ComponentHandle;
use zerem_engine::Command;

use zerem_core::language;

use crate::settings::{Settings, Store};
use crate::{MainWindow, Prefs, Theme, TorrentList};

/// kB/s → B/s, with `0` meaning unlimited.
#[must_use]
pub fn to_bps(kb: u32) -> Option<u32> {
    (kb > 0).then(|| kb.saturating_mul(1024))
}

/// How a limit reads in the panel. Formatted here, like every other number.
#[must_use]
fn speed_label(kb: u32) -> String {
    match kb {
        0 => "Unlimited".into(),
        kb if kb >= 1000 => format!("{:.1} MB/s", f64::from(kb) / 1024.0),
        kb => format!("{kb} kB/s"),
    }
}

/// Push the settings into the window. Also the startup path: the theme, the
/// sort and the column widths are restored by calling this once.
pub fn show(ui: &MainWindow, settings: &Settings) {
    let prefs = ui.global::<Prefs>();
    push!(prefs, get_download_dir, set_download_dir, settings.download_dir.display().to_string().into());
    push!(prefs, get_down_limit, set_down_limit, speed_label(settings.down_limit).into());
    push!(prefs, get_up_limit, set_up_limit, speed_label(settings.up_limit).into());

    // A hand-edited value the menus cannot step through. Saying so beats the
    // button appearing to do nothing useful.
    let off_menu = |kb: u32| kb != 0 && !crate::settings::SPEED_PRESETS.contains(&kb);
    push!(
        prefs,
        get_custom_limits,
        set_custom_limits,
        off_menu(settings.down_limit) || off_menu(settings.up_limit)
    );

    push!(prefs, get_language, set_language, language::label(&settings.language).into());
    apply_language(&settings.language);

    ui.global::<Theme>().set_dark(settings.dark);
    // The add dialog shows the same folder, because it is the same setting.
    super::add::show_destination(ui, &settings.download_dir.display().to_string());
}

/// Tell the renderer which bundled catalogue to draw from.
///
/// An empty preference means "follow the machine", so the tag is asked for
/// every time rather than resolved once and stored — a machine that changes
/// its language should be followed, not pinned to what it was on first launch.
///
/// Live: Slint holds the selection as a property, so every `@tr` in the window
/// redraws on the spot. Nothing here waits for a restart.
fn apply_language(stored: &str) {
    let tag = if stored == language::SYSTEM {
        zerem_shell::preferred_language().unwrap_or_default()
    } else {
        stored.to_owned()
    };
    let chosen = language::resolve(&tag);
    // Both halves, from one decision. The `.slint` half is Slint's bundled
    // catalogue; the half Rust composes is [`zerem_core::text`], and they have
    // to be told the same thing or the window would be half translated.
    zerem_core::text::set(chosen);
    if let Err(e) = slint::select_bundled_translation(chosen) {
        // Not fatal and not silent: the window opens in the source language,
        // which is a readable app rather than a missing one.
        tracing::warn!(chosen, error = ?e, "could not select the translation");
    }
}

pub fn wire(ui: &MainWindow, state: &Rc<crate::state::UiState>, store: &Rc<Store>, views: &Rc<super::Views>) {
    let prefs = ui.global::<Prefs>();

    prefs.on_close({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            ui.global::<Prefs>().set_open(false);
        }
    });

    prefs.on_cycle_language({
        let (store, ui) = (store.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let next = language::next(&store.get().language).to_owned();
            store.update(|s| s.language.clone_from(&next));
            // Pushed and applied here rather than waiting for the next tick:
            // the whole window changing language is the feedback for the click.
            push!(ui.global::<Prefs>(), get_language, set_language, language::label(&next).into());
            apply_language(&next);
        }
    });

    prefs.on_cycle_down_limit(cycle(ui, state, store, |s| &mut s.down_limit));
    prefs.on_cycle_up_limit(cycle(ui, state, store, |s| &mut s.up_limit));

    prefs.on_pick_download_dir({
        let (store, ui) = (store.clone(), ui.as_weak());
        move || {
            // Off the UI thread, as every native dialog must be. Unlike the
            // "add file" picker this has to come back here, because the choice
            // is written to the settings — and `Rc<Store>` cannot cross a
            // thread. So the thread carries only the path and a `Weak`, both
            // `Send`, and re-enters through the callback below.
            let start = store.get().download_dir;
            let ui = ui.clone();
            std::thread::spawn(move || {
                let Some(dir) = rfd::FileDialog::new()
                    .set_title("Where should new torrents be saved?")
                    .set_directory(&start)
                    .pick_folder()
                else {
                    return;
                };
                let chosen = dir.to_string_lossy().into_owned();
                let _ = ui.upgrade_in_event_loop(move |ui| {
                    ui.global::<Prefs>().invoke_download_dir_picked(chosen.into());
                });
            });
        }
    });

    prefs.on_download_dir_picked({
        let (store, state, ui, views) = (store.clone(), state.clone(), ui.as_weak(), views.clone());
        move |chosen| {
            let Some(ui) = ui.upgrade() else { return };
            let dir = std::path::PathBuf::from(chosen.as_str());
            if store.update(|s| s.download_dir.clone_from(&dir)) {
                state.engine.send(Command::SetDownloadDir(dir));
                show(&ui, &store.get());
                // The add dialog may be open, asking about this very folder.
                // Its "not enough room" line is answered by the picker that
                // just closed, so it is answered now and not a tick later.
                super::add::show_choice(&ui, &views.add);
            }
        }
    });

    // The view settings have no panel — they are changed by using the app, and
    // only need to survive a restart.
    ui.global::<TorrentList>().on_toggle_theme({
        let (store, ui) = (store.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let theme = ui.global::<Theme>();
            let dark = !theme.get_dark();
            theme.set_dark(dark);
            store.update(|s| s.dark = dark);
        }
    });
}

/// One handler for both limits, differing only in which field it steps.
fn cycle(
    ui: &MainWindow,
    state: &Rc<crate::state::UiState>,
    store: &Rc<Store>,
    field: fn(&mut Settings) -> &mut u32,
) -> impl FnMut() + 'static {
    let (store, state, ui) = (store.clone(), state.clone(), ui.as_weak());
    move || {
        let Some(ui) = ui.upgrade() else { return };
        store.update(|s| {
            let slot = field(s);
            *slot = Settings::next_speed(*slot);
        });
        let settings = store.get();
        // Applied at once. librqbit's limiters are settable while it runs, so
        // there is no reason to make the user wait for a restart.
        state
            .engine
            .send(Command::SetLimits { down: to_bps(settings.down_limit), up: to_bps(settings.up_limit) });
        show(&ui, &settings);
    }
}

#[cfg(test)]
mod tests {
    use super::{speed_label, to_bps};

    #[test]
    fn zero_means_unlimited_everywhere() {
        assert_eq!(to_bps(0), None);
        assert_eq!(speed_label(0), "Unlimited");
    }

    #[test]
    fn limits_are_kilobytes_on_the_way_in() {
        assert_eq!(to_bps(500), Some(512_000));
        // Saturating rather than wrapping: a hand-edited absurd value should
        // clamp, not become a tiny limit.
        assert_eq!(to_bps(u32::MAX), Some(u32::MAX));
    }

    #[test]
    fn labels_switch_unit_where_the_number_gets_long() {
        assert_eq!(speed_label(50), "50 kB/s");
        assert_eq!(speed_label(999), "999 kB/s");
        assert_eq!(speed_label(1024), "1.0 MB/s");
        assert_eq!(speed_label(5120), "5.0 MB/s");
    }
}

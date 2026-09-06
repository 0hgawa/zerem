//! Preferences.
//!
//! Everything the app will let somebody change, grouped, and typed rather than
//! stepped through. The presets that came before were faster to change and
//! impossible to get wrong, and that argument only holds while every number
//! anybody wants is on the list — which a transfer limit never is, because
//! people have a line speed and want a figure that relates to it.
//!
//! The listening port, uTP and UPnP are here now too, which reverses an earlier
//! call. The objection was that a control which silently does nothing until the
//! next launch is worse than no control, and that was an argument against the
//! silence rather than against the control: each of those three says so on its
//! own line.

use std::rc::Rc;

use slint::ComponentHandle;
use zerem_core::language;
use zerem_engine::Command;

use crate::settings::{Settings, Store};
use crate::{MainWindow, Prefs, Theme, TorrentList};

/// Ceilings, so a hand-edited file cannot produce a figure the panel then shows
/// back as fact. A gigabyte a second and a hundred at once are both far past
/// anything real, which is the point: they catch nonsense, not use.
const MAX_KB: u32 = 1_000_000;
const MAX_ACTIVE: u32 = 100;

/// kB/s → B/s, with `0` meaning unlimited.
#[must_use]
pub fn to_bps(kb: u32) -> Option<u32> {
    (kb > 0).then(|| kb.saturating_mul(1024))
}

/// What a field shows for a number that can be off.
///
/// Empty rather than the word: the placeholder underneath says what empty
/// means, so there is nothing to clear before typing a figure.
#[must_use]
fn optional(value: u32) -> String {
    if value == 0 {
        String::new()
    } else {
        value.to_string()
    }
}

/// A typed field back into a number.
///
/// Anything that is not one is zero, which every field here reads as off. The
/// input only accepts digits, so this is the belt to that pair of braces — and
/// the hole it actually covers is the settings file, which is hand-editable.
#[must_use]
fn typed(text: &str, ceiling: u32) -> u32 {
    text.trim().parse::<u32>().unwrap_or(0).min(ceiling)
}

/// Which pair of limits is in force.
///
/// Two pairs and a switch rather than one pair that gets edited: the point is
/// to go quiet for an evening and come back, and a single pair means retyping
/// the real numbers from memory every time.
#[must_use]
pub fn in_force(settings: &Settings) -> Command {
    let (down, up) = if settings.alt_speed {
        (settings.alt_down_limit, settings.alt_up_limit)
    } else {
        (settings.down_limit, settings.up_limit)
    };
    Command::SetLimits { down: to_bps(down), up: to_bps(up) }
}

/// Push the settings into the window. Also the startup path: the theme, the
/// sort and the column widths are restored by calling this once.
pub fn show(ui: &MainWindow, settings: &Settings) {
    let prefs = ui.global::<Prefs>();
    push!(prefs, get_download_dir, set_download_dir, settings.download_dir.display().to_string().into());
    push!(prefs, get_down_limit, set_down_limit, optional(settings.down_limit).into());
    push!(prefs, get_up_limit, set_up_limit, optional(settings.up_limit).into());
    push!(prefs, get_max_active, set_max_active, optional(settings.max_active).into());
    push!(prefs, get_alt_down_limit, set_alt_down_limit, optional(settings.alt_down_limit).into());
    push!(prefs, get_alt_up_limit, set_alt_up_limit, optional(settings.alt_up_limit).into());
    push!(prefs, get_alt_speed, set_alt_speed, settings.alt_speed);
    push!(prefs, get_port, set_port, optional(u32::from(settings.port)).into());
    push!(prefs, get_add_paused, set_add_paused, settings.add_paused);
    push!(prefs, get_utp, set_utp, settings.utp);
    push!(prefs, get_upnp, set_upnp, settings.upnp);
    push!(prefs, get_keep_dir, set_keep_dir, settings.keep_dir.as_str().into());
    push!(prefs, get_watch_dir, set_watch_dir, settings.watch_dir.as_str().into());
    apply_language(&settings.language);
    offer_languages(&prefs, &settings.language);

    ui.global::<Theme>().set_dark(settings.dark);
    push!(prefs, get_theme, set_theme, settings.theme.as_str().into());
    push!(prefs, get_encryption, set_encryption, settings.encryption.as_str().into());
    // Neither of these changes while the app runs, so they are stated once and
    // read from the manifest rather than typed into the window — a version in
    // two places is a version that is wrong in one of them.
    prefs.set_version(concat!("v", env!("CARGO_PKG_VERSION")).into());
    prefs.set_engine(concat!("librqbit ", env!("ZEREM_ENGINE_VERSION")).into());
    // The add dialog shows the same folder, because it is the same setting.
    super::add::show_destination(ui, &settings.download_dir.display().to_string());
}

/// Put the list of languages, and which one is chosen, in front of the panel.
///
/// Rebuilt on every change rather than filled once, because the first entry is
/// "System" — a word, not a name — and a word in a list of languages has to be
/// in the language the window is now speaking.
fn offer_languages(prefs: &Prefs, chosen: &str) {
    let names: Vec<slint::SharedString> =
        language::choices().into_iter().map(|(_, name)| zerem_core::text::tr(name).into()).collect();
    prefs.set_languages(slint::ModelRc::new(slint::VecModel::from(names)));
    prefs.set_language_index(language::index_of(chosen) as i32);
}

/// Tell both catalogues which language to answer in.
///
/// An empty preference means "follow the machine", so the tag is asked for
/// every time rather than resolved once and stored — a machine that changes its
/// language should be followed, not pinned to what it was on first launch.
///
/// One decision, two halves: Slint's bundled catalogue draws the chrome and
/// [`zerem_core::text`] composes the sentences made of numbers. Telling only one
/// would leave the window translated down the middle.
///
/// Live: Slint holds the selection as a property, so every `@tr` redraws on the
/// spot. Nothing here waits for a restart.
fn apply_language(stored: &str) {
    let tag = if stored == language::SYSTEM {
        zerem_shell::preferred_language().unwrap_or_default()
    } else {
        stored.to_owned()
    };
    let chosen = language::resolve(&tag);
    zerem_core::text::set(chosen);
    if let Err(e) = slint::select_bundled_translation(chosen) {
        // Not fatal and not silent: the window opens in the source language,
        // which is a readable app rather than a missing one.
        tracing::warn!(chosen, error = ?e, "could not select the translation");
    }
}

/// The table's column headings, in the language the app is in.
///
/// They were set once at startup, straight out of `sort::TITLES` and never
/// through `tr` — eight English words over the columns of an app that ships
/// eleven languages, and nothing caught it: the `.po` test compares the
/// `.slint` against the catalogues, and these were in neither.
///
/// Called again whenever the language changes, because `@tr` in a `.slint`
/// redraws itself and a string handed over from Rust does not.
pub fn show_titles(ui: &MainWindow) {
    ui.global::<crate::TorrentList>().set_col_title(slint::ModelRc::from(
        zerem_core::sort::TITLES
            .iter()
            .map(|&title| slint::SharedString::from(zerem_core::tr(title)))
            .collect::<Vec<_>>()
            .as_slice(),
    ));
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

    prefs.on_set_down_limit(limit(ui, state, store, |s, kb| s.down_limit = kb));
    prefs.on_set_up_limit(limit(ui, state, store, |s, kb| s.up_limit = kb));
    prefs.on_set_alt_down_limit(limit(ui, state, store, |s, kb| s.alt_down_limit = kb));
    prefs.on_set_alt_up_limit(limit(ui, state, store, |s, kb| s.alt_up_limit = kb));

    prefs.on_toggle_alt_speed({
        let (store, state, ui) = (store.clone(), state.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let on = !store.get().alt_speed;
            store.update(|s| s.alt_speed = on);
            let settings = store.get();
            state.engine.send(in_force(&settings));
            push!(ui.global::<Prefs>(), get_alt_speed, set_alt_speed, on);
        }
    });

    prefs.on_set_max_active({
        let (store, state, ui) = (store.clone(), state.clone(), ui.as_weak());
        move |text| {
            let Some(ui) = ui.upgrade() else { return };
            let chosen = typed(&text, MAX_ACTIVE);
            store.update(|s| s.max_active = chosen);
            state.engine.send(Command::SetMaxActive(chosen));
            push!(ui.global::<Prefs>(), get_max_active, set_max_active, optional(chosen).into());
        }
    });

    prefs.on_set_port({
        let (store, ui) = (store.clone(), ui.as_weak());
        move |text| {
            let Some(ui) = ui.upgrade() else { return };
            // Below 1024 is the privileged range, where a client gets a bind
            // failure it cannot explain. Anything down there becomes the
            // default rather than a port that was never going to work.
            let asked = u16::try_from(typed(&text, u32::from(u16::MAX))).unwrap_or(0);
            let port = if asked < 1024 { zerem_engine::DEFAULT_PORT } else { asked };
            store.update(|s| s.port = port);
            push!(ui.global::<Prefs>(), get_port, set_port, optional(u32::from(port)).into());
        }
    });

    prefs.on_set_add_paused({
        let (store, state) = (store.clone(), state.clone());
        move |on| {
            store.update(|s| s.add_paused = on);
            // The one of the four that takes effect now: it is read when a
            // torrent is added, not when the session is built.
            state.engine.send(Command::SetAddPaused(on));
        }
    });
    prefs.on_set_utp(flag(store, |s, on| s.utp = on));
    prefs.on_set_upnp(flag(store, |s, on| s.upnp = on));

    prefs.on_set_density({
        let (store, ui) = (store.clone(), ui.as_weak());
        move |at| {
            let Some(ui) = ui.upgrade() else { return };
            let chosen = at.clamp(0, 2);
            // Applied here rather than on the next tick, for the same reason
            // the language is: the list changing under the cursor is the
            // feedback for the click, and a quarter of a second later reads as
            // a click that missed.
            ui.global::<crate::Theme>().set_density(chosen);
            store.update(|s| s.density = u8::try_from(chosen).unwrap_or(1));
        }
    });

    prefs.on_set_language({
        let (store, ui) = (store.clone(), ui.as_weak());
        move |at| {
            let Some(ui) = ui.upgrade() else { return };
            let chosen = language::at(at.max(0) as usize).to_owned();
            store.update(|s| s.language.clone_from(&chosen));
            // Applied here rather than on the next tick: the whole window
            // changing language is the feedback for the click. The list itself
            // is rebuilt too — "System" is a word, and it is the one entry that
            // has to change with the language it sits in.
            apply_language(&chosen);
            offer_languages(&ui.global::<Prefs>(), &chosen);
            // The headings are Rust strings handed over once; a `@tr` in a
            // `.slint` redraws itself and these cannot.
            show_titles(&ui);
        }
    });

    wire_download_dir(ui, state, store, views);
}

/// The folder picker and the answer coming back from it.
///
/// Its own function because it is its own thing: a thread, a native dialog and
/// a re-entry, where everything above is a field being written down.
/// The native folder dialog, off the UI thread, coming back through a callback.
///
/// Every folder setting needs the same handoff and none of them can do it
/// inline: a native dialog blocks the thread it runs on, and `Rc<Store>` cannot
/// leave the UI one. So the thread carries only a path and a `Weak`, both
/// `Send`, and re-enters through a callback with the UI's own context intact.
///
/// `deliver` is a plain function pointer rather than a closure, which is what
/// makes it `Send` without anything having to be moved into it.
fn ask_folder(
    ui: &slint::Weak<MainWindow>,
    title: &'static str,
    start: std::path::PathBuf,
    deliver: fn(&Prefs, slint::SharedString),
) {
    let ui = ui.clone();
    std::thread::spawn(move || {
        let Some(dir) = rfd::FileDialog::new().set_title(title).set_directory(&start).pick_folder() else {
            return;
        };
        let chosen = dir.to_string_lossy().into_owned();
        let _ = ui.upgrade_in_event_loop(move |ui| deliver(&ui.global::<Prefs>(), chosen.into()));
    });
}

/// Where a folder picker should open, which is where it already points or the
/// download folder for one that points nowhere.
fn starting_at(current: &str, fallback: std::path::PathBuf) -> std::path::PathBuf {
    if current.is_empty() {
        fallback
    } else {
        std::path::PathBuf::from(current)
    }
}

fn wire_download_dir(
    ui: &MainWindow,
    state: &Rc<crate::state::UiState>,
    store: &Rc<Store>,
    views: &Rc<super::Views>,
) {
    let prefs = ui.global::<Prefs>();

    prefs.on_pick_download_dir({
        let (store, ui) = (store.clone(), ui.as_weak());
        move || {
            ask_folder(
                &ui,
                "Where should new torrents be saved?",
                store.get().download_dir,
                |prefs, chosen| {
                    prefs.invoke_download_dir_picked(chosen);
                },
            );
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

    prefs.on_pick_keep_dir({
        let (store, ui) = (store.clone(), ui.as_weak());
        move || {
            let settings = store.get();
            let start = starting_at(&settings.keep_dir, settings.download_dir);
            ask_folder(&ui, "Where should finished torrents be moved to?", start, |prefs, chosen| {
                prefs.invoke_keep_dir_picked(chosen);
            });
        }
    });

    prefs.on_keep_dir_picked({
        let (store, state, ui) = (store.clone(), state.clone(), ui.as_weak());
        move |chosen| {
            let Some(ui) = ui.upgrade() else { return };
            let dir = chosen.to_string();
            if store.update(|s| s.keep_dir.clone_from(&dir)) {
                state.engine.send(Command::SetKeepDir(Some(dir)));
                show(&ui, &store.get());
            }
        }
    });

    prefs.on_pick_watch_dir({
        let (store, ui) = (store.clone(), ui.as_weak());
        move || {
            let settings = store.get();
            let start = starting_at(&settings.watch_dir, settings.download_dir);
            ask_folder(&ui, "Which folder should be watched for .torrent files?", start, |prefs, chosen| {
                prefs.invoke_watch_dir_picked(chosen);
            });
        }
    });

    prefs.on_watch_dir_picked({
        let (store, state, ui) = (store.clone(), state.clone(), ui.as_weak());
        move |chosen| {
            let Some(ui) = ui.upgrade() else { return };
            let dir = chosen.to_string();
            if store.update(|s| s.watch_dir.clone_from(&dir)) {
                state.engine.send(Command::SetWatchDir(Some(dir)));
                show(&ui, &store.get());
            }
        }
    });

    prefs.on_clear_watch_dir({
        let (store, state, ui) = (store.clone(), state.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            if store.update(|s| s.watch_dir.clear()) {
                state.engine.send(Command::SetWatchDir(None));
                show(&ui, &store.get());
            }
        }
    });

    wire_switches(ui, state, store, views);
}

/// What is left of the panel once the pickers have their own place: the
/// switches that clear a folder, the theme, and putting everything back.
///
/// Split off because the pickers alone had grown past the line this workspace
/// holds, and because the name of the function they were in had stopped
/// describing what was inside it.
fn wire_switches(
    ui: &MainWindow,
    state: &Rc<crate::state::UiState>,
    store: &Rc<Store>,
    views: &Rc<super::Views>,
) {
    let prefs = ui.global::<Prefs>();

    prefs.on_clear_keep_dir({
        let (store, state, ui) = (store.clone(), state.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            if store.update(|s| s.keep_dir.clear()) {
                state.engine.send(Command::SetKeepDir(None));
                show(&ui, &store.get());
            }
        }
    });

    // The theme has no panel row of its own to be wired from — the toolbar
    // button and the preferences check both come through here.
    ui.global::<TorrentList>().on_toggle_theme({
        let (store, ui) = (store.clone(), ui.as_weak());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let theme = ui.global::<Theme>();
            let dark = !theme.get_dark();
            theme.set_dark(dark);
            // And it is now an explicit choice rather than whatever the desktop
            // is: pressing this is somebody saying which one they want, so
            // "follow the system" stops being true the moment they do.
            let chosen = if dark { "dark" } else { "light" };
            store.update(|s| {
                s.dark = dark;
                chosen.clone_into(&mut s.theme);
            });
            push!(ui.global::<Prefs>(), get_theme, set_theme, chosen.into());
        }
    });

    prefs.on_set_encryption({
        let (store, ui) = (store.clone(), ui.as_weak());
        move |chosen| {
            let Some(ui) = ui.upgrade() else { return };
            let chosen = chosen.to_string();
            store.update(|s| s.encryption.clone_from(&chosen));
            ui.global::<Prefs>().set_encryption(chosen.into());
            // Not pushed to the running session on purpose. The policy is fixed
            // when the engine is built, and a control that quietly did nothing
            // until the next launch is the thing the note under this card
            // exists to prevent -- so it says so instead of pretending.
        }
    });

    prefs.on_pick_theme({
        let (store, ui) = (store.clone(), ui.as_weak());
        move |chosen| {
            let Some(ui) = ui.upgrade() else { return };
            let chosen = chosen.to_string();
            // "System" is a choice that has to be resolved before it can be
            // drawn, and a desktop that will not say leaves whatever is on
            // screen alone rather than guessing.
            let dark = match chosen.as_str() {
                "light" => Some(false),
                "dark" => Some(true),
                _ => zerem_shell::prefers_dark(),
            };
            store.update(|s| {
                s.theme.clone_from(&chosen);
                if let Some(dark) = dark {
                    s.dark = dark;
                }
            });
            if let Some(dark) = dark {
                ui.global::<Theme>().set_dark(dark);
            }
            push!(ui.global::<Prefs>(), get_theme, set_theme, chosen.as_str().into());
        }
    });

    prefs.on_reset_all({
        let (store, state, ui, views) = (store.clone(), state.clone(), ui.as_weak(), views.clone());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            // The download folder is kept. It is the one setting that describes
            // where somebody's files already are, and putting it back to the
            // default would silently start writing the next torrent somewhere
            // else — a reset of preferences, not a rearrangement of a disk.
            store.update(|s| {
                let keep = s.download_dir.clone();
                *s = crate::settings::Settings::default();
                s.download_dir = keep;
            });

            let settings = store.get();
            // Everything the engine was told, told again. A setting the session
            // is holding does not un-tell itself.
            state.engine.send(in_force(&settings));
            state.engine.send(Command::SetMaxActive(settings.max_active));
            state.engine.send(Command::SetAddPaused(settings.add_paused));
            state.engine.send(Command::SetKeepDir(None));
            state.engine.send(Command::SetWatchDir(None));
            apply_language(&settings.language);

            ui.global::<Prefs>().set_resetting(false);
            show(&ui, &settings);
            offer_languages(&ui.global::<Prefs>(), &settings.language);
            super::refresh_now(&ui, &state, &views);
        }
    });
}

/// One handler for both transfer limits, differing only in the field it writes.
///
/// The figure is pushed back after it is stored, so a value that was clamped —
/// or typed with a stray space — shows as what was kept rather than as what was
/// typed.
fn limit(
    ui: &MainWindow,
    state: &Rc<crate::state::UiState>,
    store: &Rc<Store>,
    field: fn(&mut Settings, u32),
) -> impl FnMut(slint::SharedString) + 'static {
    let (store, state, ui) = (store.clone(), state.clone(), ui.as_weak());
    move |text| {
        let Some(ui) = ui.upgrade() else { return };
        let kb = typed(&text, MAX_KB);
        store.update(|s| field(s, kb));
        let settings = store.get();
        // Applied at once. librqbit's limiters are settable while it runs, so
        // there is no reason to make anybody wait for a restart.
        state.engine.send(in_force(&settings));
        show(&ui, &settings);
    }
}

/// One handler for the settings that are only written down.
///
/// The port, uTP and UPnP are read when the session is built, so there is
/// nothing to send and nothing to redraw — the panel already says they wait for
/// the next launch. Adding torrents stopped is read when one is added.
fn flag(store: &Rc<Store>, field: fn(&mut Settings, bool)) -> impl FnMut(bool) + 'static {
    let store = store.clone();
    move |on| {
        store.update(|s| field(s, on));
    }
}

#[cfg(test)]
mod tests {
    use super::{optional, to_bps, typed, MAX_ACTIVE, MAX_KB};

    #[test]
    fn zero_means_unlimited_everywhere() {
        assert_eq!(to_bps(0), None);
        // And shows as an empty field, so the placeholder can say what empty
        // means instead of leaving a word to clear before typing.
        assert_eq!(optional(0), "");
    }

    #[test]
    fn limits_are_kilobytes_on_the_way_in() {
        assert_eq!(to_bps(500), Some(512_000));
        // Saturating rather than wrapping: a hand-edited absurd value should
        // clamp, not become a tiny limit.
        assert_eq!(to_bps(u32::MAX), Some(u32::MAX));
    }

    #[test]
    fn a_number_comes_back_as_itself() {
        assert_eq!(optional(500), "500");
        assert_eq!(typed("500", MAX_KB), 500);
        assert_eq!(typed(" 500 ", MAX_KB), 500, "a stray space is not a refusal");
    }

    #[test]
    fn anything_that_is_not_a_number_is_off() {
        // The field only accepts digits, so this is the belt to that pair of
        // braces — and the hole it actually covers is the settings file, which
        // is hand-editable.
        assert_eq!(typed("", MAX_KB), 0);
        assert_eq!(typed("fast", MAX_KB), 0);
        assert_eq!(typed("-5", MAX_KB), 0);
    }

    #[test]
    fn nonsense_is_clamped_rather_than_shown_back_as_fact() {
        assert_eq!(typed("999999999", MAX_ACTIVE), MAX_ACTIVE);
        assert_eq!(typed("4000000000", MAX_KB), MAX_KB);
    }
}

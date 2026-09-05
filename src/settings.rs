//! What survives a restart besides the torrents.
//!
//! One file, written atomically and only after the user stops changing things.
//! Both of those matter for different reasons: a crash mid-write must not leave
//! a truncated file where the settings were, and a control that emits while it
//! is being dragged would otherwise rewrite the whole file dozens of times for a
//! value nobody has finished choosing.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How long the settings have to stay unchanged before they are written. Long
/// enough that a drag produces one write rather than dozens, short enough that a
/// crash in that window costs nothing anyone would notice.
const QUIET: std::time::Duration = std::time::Duration::from_millis(400);

/// The choices the speed menus offer, in kB/s. `0` is unlimited.
///
/// A menu rather than a text field: nobody wants to type "512", and picking from
/// a short list is both faster and impossible to get wrong. A hand-edited file
/// may hold any value, and the menu will show it.

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // --- engine ---
    pub download_dir: PathBuf,
    /// kB/s, `0` for unlimited.
    pub down_limit: u32,
    pub up_limit: u32,
    /// The other pair, and the switch between them.
    ///
    /// Two sets rather than one that gets edited: the point is to go quiet for
    /// an evening and come back, and a single pair means retyping the real
    /// numbers from memory every time.
    pub alt_down_limit: u32,
    pub alt_up_limit: u32,
    pub alt_speed: bool,
    /// Changing these needs a restart, so they are here but not in the panel —
    /// a control that silently does nothing until relaunch is worse than none.
    pub port: u16,
    pub utp: bool,
    pub upnp: bool,

    // --- view ---
    /// Which theme was chosen: "system", "light" or "dark".
    ///
    /// A string and not a boolean, because there are three answers and the
    /// third is the interesting one — "whatever the desktop is" has to survive
    /// the desktop changing its mind, which a stored `true` cannot.
    ///
    /// `dark` stays beside it and is what the window is *currently* drawn in,
    /// so a session that follows the system still remembers what it resolved to
    /// and the first frame after a launch is not the wrong colour.
    #[serde(default = "follow_the_system")]
    pub theme: String,

    /// What the app does about protocol encryption: "off", "prefer" or
    /// "require".
    ///
    /// Three answers, and both of the others cost something. Requiring it
    /// turns away every peer that will not, which on a healthy swarm is most of
    /// the ones holding the data. And *either* of them turns uTP off, because
    /// the handshake stalls over it -- so this is off until somebody asks,
    /// rather than a default that quietly changes how the app uses the line.
    #[serde(default = "off_because_it_costs_utp")]
    pub encryption: String,
    pub dark: bool,
    pub sort_col: usize,
    pub sort_desc: bool,
    /// The shelves: a name and where torrents on it are written.
    ///
    /// Made by typing a name in the add dialog rather than in a manager of
    /// their own — see [`zerem_core::category`] for why there is no manager.
    pub categories: zerem_core::Shelves,
    /// Which shelf each torrent is on, by infohash.
    ///
    /// Here and not in the engine because it is an organising idea, not a
    /// transfer one: librqbit neither knows nor needs to know. A `BTreeMap` so
    /// the file is stable between saves and a diff of it says something.
    pub assigned: std::collections::BTreeMap<String, String>,
    /// Whether the state rail is on screen. Like the column widths, nobody
    /// chooses this in a panel — they arrive at it by using the app.
    pub rail_open: bool,
    /// Empty means "never resized"; the table falls back to its own defaults.
    pub column_widths: Vec<f32>,
    /// Empty means "never touched"; every column shows. Hand-editable like the
    /// rest of the file, so the name column is forced back on when it is read.
    pub column_visible: Vec<bool>,
    /// How many torrents may download at once. `0` is no limit.
    ///
    /// Seeding is never counted: a finished torrent costs no download
    /// bandwidth, which is the thing this rations.
    pub max_active: u32,
    /// Whether a torrent is added stopped rather than started.
    ///
    /// For somebody who adds a batch and then picks: the alternative is twenty
    /// torrents all racing before anybody has looked at them.
    pub add_paused: bool,
    /// Where a torrent is moved once it finishes. Empty is off, which is the
    /// default: moving somebody's files is not something to start doing
    /// because an update shipped.
    #[serde(default)]
    pub keep_dir: String,
    /// A folder `.torrent` files are picked up from. Empty is off, which is
    /// the default: a folder nobody named is a folder nobody wants watched.
    #[serde(default)]
    pub watch_dir: String,
    /// Which language the interface is in, as a folder name under `lang/`.
    ///
    /// Empty means "whatever the machine is set to", and is stored as empty
    /// rather than resolved: a machine that changes its language should be
    /// followed, not pinned to what it was the day the app was first opened.
    pub language: String,
    /// How wide the details drawer is, in pixels. Set by dragging its edge, so
    /// it survives like the column widths do: nobody chose it in a panel, they
    /// arrived at it, and arriving at it twice is the annoyance.
    pub drawer_width: f32,
}

/// The theme a fresh install follows.
fn follow_the_system() -> String {
    "system".to_owned()
}

/// Off, because turning it on turns uTP off. The day that stops being true
/// this becomes "prefer", which costs nothing in peers.
fn off_because_it_costs_utp() -> String {
    "off".to_owned()
}

impl Default for Settings {
    fn default() -> Self {
        let engine = zerem_engine::EngineConfig::default();
        Self {
            download_dir: engine.download_dir,
            down_limit: 0,
            up_limit: 0,
            // A tenth of a common line, which is a number somebody will change
            // — but it is off by default, so it is a starting point and not a
            // limit imposed on anybody.
            alt_down_limit: 500,
            alt_up_limit: 100,
            alt_speed: false,
            port: engine.port,
            utp: engine.utp,
            upnp: engine.upnp,
            theme: follow_the_system(),
            encryption: off_because_it_costs_utp(),
            dark: true,
            sort_col: 0,
            sort_desc: false,
            categories: zerem_core::Shelves::new(),
            assigned: std::collections::BTreeMap::new(),
            rail_open: true,
            column_widths: Vec::new(),
            column_visible: Vec::new(),
            max_active: 0,
            add_paused: false,
            keep_dir: String::new(),
            watch_dir: String::new(),
            language: zerem_core::language::SYSTEM.to_owned(),
            drawer_width: crate::state::DEFAULT_DRAWER_W,
        }
    }
}

impl Settings {
    /// Read them, or fall back to the defaults.
    ///
    /// A settings file that will not parse is renamed aside rather than deleted
    /// or silently overwritten: the user gets working defaults now, and whatever
    /// they had hand-edited is still there to look at.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        // Windows text editors write a UTF-8 BOM, and `serde_json` refuses a
        // document that starts with one. Hand-editing this file is the whole
        // reason it exists, so refusing what Notepad produces is not an option.
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        match serde_json::from_str(text) {
            Ok(settings) => settings,
            Err(e) => {
                tracing::warn!(error = %e, "settings could not be read; keeping them aside");
                let _ = std::fs::rename(path, path.with_extension("json.bad"));
                Self::default()
            }
        }
    }

    /// Write via temp + rename.
    ///
    /// A crash mid-write leaves the previous file intact instead of a truncated
    /// one. `sync_all` before the rename so the bytes are on the disk and not
    /// merely in a cache the crash would take with it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(&text)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, path)
    }

    #[must_use]
    pub fn to_engine_config(&self) -> zerem_engine::EngineConfig {
        zerem_engine::EngineConfig {
            download_dir: self.download_dir.clone(),
            max_active: self.max_active,
            add_paused: self.add_paused,
            keep_dir: (!self.keep_dir.is_empty()).then(|| PathBuf::from(&self.keep_dir)),
            watch_dir: (!self.watch_dir.is_empty()).then(|| PathBuf::from(&self.watch_dir)),
            port: self.port,
            utp: self.utp,
            upnp: self.upnp,
            encryption: self.encryption.clone(),
            ..Default::default()
        }
    }
}

/// Settings plus the timer that writes them.
///
/// UI thread only: the timer and the pending flag live there, which is also
/// where every control callback runs.
pub struct Store {
    path: PathBuf,
    current: std::cell::RefCell<Settings>,
    timer: slint::Timer,
    pending: std::cell::Cell<bool>,
}

impl Store {
    #[must_use]
    pub fn open(state_dir: &Path) -> Self {
        let path = state_dir.join("settings.json");
        let current = Settings::load(&path);

        // Written on the first run even though nothing has changed: a settings
        // file the user cannot find is a settings file they cannot edit, and
        // the port, uTP and UPnP are only reachable that way.
        if !path.exists() {
            if let Err(e) = current.save(&path) {
                tracing::warn!(error = %e, path = %path.display(), "could not create the settings file");
            }
        }

        Self {
            current: std::cell::RefCell::new(current),
            path,
            timer: slint::Timer::default(),
            pending: std::cell::Cell::new(false),
        }
    }

    #[must_use]
    pub fn get(&self) -> Settings {
        self.current.borrow().clone()
    }

    /// Change the settings and schedule a write.
    ///
    /// Returns whether anything actually changed, so a caller can skip the work
    /// that would follow — re-applying a limit the engine already has, say.
    pub fn update(self: &std::rc::Rc<Self>, edit: impl FnOnce(&mut Settings)) -> bool {
        let before = self.current.borrow().clone();
        edit(&mut self.current.borrow_mut());
        if *self.current.borrow() == before {
            return false;
        }

        self.pending.set(true);
        let this = self.clone();
        self.timer.start(slint::TimerMode::SingleShot, QUIET, move || this.write());
        true
    }

    /// Write a change still waiting on the debounce. For the way out.
    pub fn flush(&self) {
        if self.pending.get() {
            self.write();
        }
    }

    fn write(&self) {
        self.pending.set(false);
        if let Err(e) = self.current.borrow().save(&self.path) {
            tracing::error!(error = %e, path = %self.path.display(), "could not save settings");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("zerem-settings-tests");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir.join(name)
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let s = Settings::load(&scratch("nonexistent.json"));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let path = scratch("round-trip.json");
        let written =
            Settings { up_limit: 250, dark: false, column_widths: vec![120.0, 80.0], ..Default::default() };
        written.save(&path).expect("save");

        assert_eq!(Settings::load(&path), written);
    }

    #[test]
    fn a_new_field_does_not_invalidate_an_old_file() {
        // `#[serde(default)]` is what makes an upgrade keep the user's settings
        // instead of resetting them.
        let path = scratch("partial.json");
        std::fs::write(&path, br#"{"up_limit": 500}"#).expect("write");
        let s = Settings::load(&path);
        assert_eq!(s.up_limit, 500);
        assert_eq!(s.down_limit, Settings::default().down_limit);
    }

    #[test]
    fn a_file_saved_by_a_windows_editor_still_reads() {
        // Notepad and PowerShell's `Set-Content -Encoding utf8` both prepend a
        // BOM, and serde_json refuses a document that starts with one. Editing
        // this file by hand is the whole reason it exists.
        let path = scratch("bom.json");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"up_limit": 500}"#);
        std::fs::write(&path, bytes).expect("write");

        assert_eq!(Settings::load(&path).up_limit, 500);
    }

    #[test]
    fn a_broken_file_is_kept_aside_rather_than_lost() {
        let path = scratch("broken.json");
        std::fs::write(&path, b"{ this is not json").expect("write");
        let _ = std::fs::remove_file(path.with_extension("json.bad"));

        assert_eq!(Settings::load(&path), Settings::default());
        assert!(path.with_extension("json.bad").exists(), "the original is still there to look at");
    }

    #[test]
    fn no_temporary_file_is_left_behind() {
        let path = scratch("atomic.json");
        Settings::default().save(&path).expect("save");
        assert!(!path.with_extension("json.tmp").exists());
    }
}

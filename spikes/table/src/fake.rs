//! Synthetic session — stands in for `zerem-engine` until it exists.
//!
//! The shape that matters for the spike is the realistic one: a large list in
//! which only a small fraction of rows changes per tick. A client with 2000
//! torrents typically has a few dozen actually transferring; the rest sit
//! paused or seeding at zero. If the diff only pays for what moved, that is
//! where it shows.
//!
//! Every field is integral. No floats anywhere in the domain row, so
//! `PartialEq` is exact and cannot be tripped by rounding noise — a ratio that
//! wobbles in the last float digit would mark every row dirty every tick and
//! silently defeat the whole design.

use std::sync::Arc;

/// Fraction of the list that is actually transferring, in realistic mode.
const ACTIVE_PERMILLE: u64 = 20;

/// Fixed seed: two runs of the spike must produce the same list, or one
/// measurement cannot be compared against the one before it.
const SEED: u64 = 0x5A45_5245_4D00_0001;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Paused,
    Downloading,
    Seeding,
    Checking,
}

impl State {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Paused => "Paused",
            Self::Downloading => "Downloading",
            Self::Seeding => "Seeding",
            Self::Checking => "Checking",
        }
    }

    /// The discriminant the UI colours by, kept in sync with `app.slint`.
    pub const fn kind(self) -> i32 {
        match self {
            Self::Paused => 0,
            Self::Downloading => 1,
            Self::Seeding => 2,
            Self::Checking => 3,
        }
    }
}

/// One torrent as the engine would report it.
///
/// `name` is an `Arc<str>` because it never changes: cloning a row per tick
/// must not re-allocate the string. With 2000 rows that is 2000 avoided
/// allocations a second, for a value that is identical every time.
#[derive(Clone, PartialEq, Eq)]
pub struct Torrent {
    pub id: u32,
    pub name: Arc<str>,
    /// `name`, lower-cased once at creation, purely so the name sort is a byte
    /// comparison. Folding case inside the comparator instead cost 3.2 ms per
    /// sort at 2000 rows — fifty times the entire diff — because a sort makes
    /// ~22 000 comparisons and each one re-walked both strings through the
    /// Unicode lowercase tables.
    pub name_key: Arc<str>,
    pub size: u64,
    pub done: u64,
    pub state: State,
    pub down_bps: u64,
    pub up_bps: u64,
    pub peers_connected: u32,
    pub peers_total: u32,
    pub eta: Option<u32>,
    pub ratio_x100: u32,
}

impl Torrent {
    pub const fn is_active(&self) -> bool {
        matches!(self.state, State::Downloading | State::Seeding | State::Checking)
    }

    /// Progress in ten-thousandths — the sort key, integral so ordering is
    /// stable and comparison is exact.
    pub const fn progress_bp(&self) -> u64 {
        match (self.done * 10_000).checked_div(self.size) {
            Some(bp) => bp,
            None => 0,
        }
    }
}

/// xorshift64*. Deterministic on purpose: two runs of the spike produce the
/// same list, so a measurement can be compared against the previous one.
pub struct Rng(u64);

impl Rng {
    pub const fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0x2545_F491_4F6C_DD1D } else { seed })
    }

    pub const fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `0..n`.
    pub const fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

const SUBJECTS: [&str; 24] = [
    "Archipelago",
    "Nightfall",
    "Blue Meridian",
    "Iron Harvest",
    "The Longest Winter",
    "Solaris Drift",
    "Copper Gardens",
    "Deep Field",
    "Salt and Ash",
    "Northern Line",
    "Paper Lanterns",
    "The Quiet Coast",
    "Vermilion",
    "Foxglove",
    "Static Bloom",
    "Harbour Lights",
    "Ember Season",
    "Glass Cathedral",
    "Tidewater",
    "Low Orbit",
    "Marble Hall",
    "Winter Signal",
    "Roseline",
    "The Ninth Gate",
];

const KINDS: [&str; 6] = ["S01", "S02", "2160p", "1080p", "Complete", "Remastered"];
const TAGS: [&str; 6] = ["WEB-DL", "BluRay", "x265-HEVC", "AV1-OPUS", "FLAC-24bit", "ISO-amd64"];

fn make_name(rng: &mut Rng, i: usize) -> Arc<str> {
    let s = SUBJECTS[i % SUBJECTS.len()];
    let k = KINDS[rng.below(KINDS.len() as u64) as usize];
    let t = TAGS[rng.below(TAGS.len() as u64) as usize];
    format!("{s}.{k}.{t}-ZEREM").into()
}

pub struct Session {
    pub torrents: Vec<Torrent>,
    /// When set, *every* row mutates each tick — the worst case the diff can
    /// face, and the number worth quoting as the ceiling.
    pub churn: bool,
    rng: Rng,
}

impl Session {
    #[must_use]
    pub fn new(count: usize) -> Self {
        let mut rng = Rng::new(SEED);
        let torrents = (0..count).map(|i| Self::make(&mut rng, i)).collect();
        Self { torrents, churn: false, rng }
    }

    fn make(rng: &mut Rng, i: usize) -> Torrent {
        let size = (300_u64 << 20) + rng.below(60_u64 << 30);
        let active = rng.below(1000) < ACTIVE_PERMILLE;
        let done = if active { rng.below(size) } else { size };

        let state = if !active {
            State::Paused
        } else if done >= size {
            State::Seeding
        } else if rng.below(8) == 0 {
            // A few of the active ones are re-hashing. Worth generating: it is
            // the third progress-bar colour and the one state that advances
            // without transferring anything.
            State::Checking
        } else {
            State::Downloading
        };

        let name = make_name(rng, i);
        let mut t = Torrent {
            id: i as u32,
            name_key: name.to_lowercase().into(),
            name,
            size,
            done,
            state,
            down_bps: 0,
            up_bps: 0,
            peers_connected: 0,
            peers_total: 0,
            eta: None,
            ratio_x100: rng.below(400) as u32,
        };
        if active {
            Self::stir(rng, &mut t);
        }
        t
    }

    /// Move one torrent forward by a tick.
    fn stir(rng: &mut Rng, t: &mut Torrent) {
        match t.state {
            State::Downloading => {
                t.down_bps = (400 << 10) + rng.below(11 << 20);
                t.up_bps = rng.below(2 << 20);
                t.done = (t.done + t.down_bps).min(t.size);
                if t.done >= t.size {
                    t.state = State::Seeding;
                    t.down_bps = 0;
                    t.eta = None;
                } else {
                    t.eta = Some(((t.size - t.done) / t.down_bps.max(1)).min(u64::from(u32::MAX)) as u32);
                }
            }
            State::Seeding => {
                t.down_bps = 0;
                t.up_bps = rng.below(6 << 20);
                t.ratio_x100 = t.ratio_x100.saturating_add(rng.below(3) as u32);
                t.eta = None;
            }
            State::Checking => {
                // Re-hashing: verified bytes advance, nothing crosses the wire.
                t.down_bps = 0;
                t.up_bps = 0;
                t.done = (t.done + (90_u64 << 20)).min(t.size);
                t.eta = None;
                if t.done >= t.size {
                    t.state = State::Seeding;
                }
            }
            State::Paused => {}
        }
        t.peers_connected = rng.below(40) as u32;
        t.peers_total = t.peers_connected + rng.below(200) as u32;
    }

    /// Advance the session by one tick.
    ///
    /// Returns how many rows actually mutated, which is the number the diff in
    /// `model.rs` should independently arrive at. The two disagreeing means the
    /// diff is dirtying rows that did not change.
    pub fn tick(&mut self) -> usize {
        // Destructured so the rows and the generator are two disjoint borrows —
        // `self.rng` inside a loop over `&mut self.torrents` does not compile.
        let Self { torrents, churn, rng } = self;
        let mut moved = 0;
        for t in torrents.iter_mut() {
            if *churn && !t.is_active() {
                // Worst case: drag paused rows too, so nothing can be skipped.
                t.up_bps = rng.below(64 << 10);
                t.peers_connected = rng.below(8) as u32;
                moved += 1;
            } else if t.is_active() {
                Self::stir(rng, t);
                moved += 1;
            }
        }
        moved
    }

    #[must_use]
    pub fn active_count(&self) -> usize {
        self.torrents.iter().filter(|t| t.is_active()).count()
    }

    pub fn resize(&mut self, count: usize) {
        let mut rng = Rng::new(SEED);
        self.torrents = (0..count).map(|i| Self::make(&mut rng, i)).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::{Session, State};

    #[test]
    fn generation_is_deterministic() {
        let a = Session::new(64);
        let b = Session::new(64);
        assert!(a.torrents == b.torrents, "same seed must give the same list");
    }

    #[test]
    fn idle_rows_are_byte_identical_across_a_tick() {
        // The premise the whole diff rests on: a paused torrent must not move,
        // or the model would mark it dirty and repaint it for nothing.
        let mut s = Session::new(500);
        let before: Vec<_> = s.torrents.iter().filter(|t| !t.is_active()).cloned().collect();
        s.tick();
        let after: Vec<_> = s.torrents.iter().filter(|t| !t.is_active()).cloned().collect();
        assert_eq!(before.len(), after.len());
        assert!(before == after);
    }

    #[test]
    fn tick_reports_exactly_the_rows_it_moved() {
        let mut s = Session::new(500);
        let before = s.torrents.clone();
        let reported = s.tick();
        let actual = before.iter().zip(&s.torrents).filter(|(a, b)| a != b).count();
        assert_eq!(reported, actual);
    }

    #[test]
    fn churn_mode_moves_every_row() {
        let mut s = Session::new(200);
        s.churn = true;
        assert_eq!(s.tick(), 200);
    }

    #[test]
    fn a_finished_download_becomes_a_seed() {
        let mut s = Session::new(1);
        s.torrents[0].state = State::Downloading;
        s.torrents[0].done = s.torrents[0].size - 1;
        s.tick();
        assert_eq!(s.torrents[0].state, State::Seeding);
        assert_eq!(s.torrents[0].down_bps, 0);
    }
}

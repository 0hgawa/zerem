//! Zerem core — the layer everything else is built on.
//!
//! Pure: no async runtime, no I/O, no UI. That is not purism, it is what lets
//! the ordering and the arithmetic be tested in microseconds without a window
//! or a network, and it is the boundary that keeps the engine swappable.

pub mod category;
pub mod choice;
pub mod content;
pub mod contrast;
pub mod detail;
pub mod fault;
pub mod filename;
mod filter;
pub mod flag;
pub mod fmt;
mod folder;
mod geo;
pub mod history;
pub mod icon;
pub mod language;
pub mod mime;
mod pending;
pub mod queue;
pub mod rate;
mod relocate;
pub mod sort;
pub mod text;
pub mod torrent;
mod watch;

pub use category::Shelves;
pub use choice::{flags, is_narrowed, ticked, to_fetch};
pub use content::Content;
pub use detail::{Details, FileRow, PeerRow, Transport};
pub use fault::explain;
pub use filename::split as split_path;
pub use filter::{Filter, Shown};
pub use flag::{of as flag, Flag};
pub use folder::subfolder;
pub use geo::country;
pub use history::{History, Spark};
pub use language::SHIPPED as LANGUAGES;
pub use mime::{is_playable, of as mime_of, Stream};
pub use pending::{Pending, PendingFile};
pub use queue::{admit, Waiting};
pub use rate::Rate;
pub use relocate::{landing as move_landing, plan as move_plan, Step};
pub use sort::Sort;
pub use text::tr;
pub use torrent::{SessionStats, Stall, State, TorrentId, TorrentRow};
pub use watch::{parked, waiting, Outcome as Watched};

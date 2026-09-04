//! Zerem core — the layer everything else is built on.
//!
//! Pure: no async runtime, no I/O, no UI. That is not purism, it is what lets
//! the ordering and the arithmetic be tested in microseconds without a window
//! or a network, and it is the boundary that keeps the engine swappable.

pub mod choice;
pub mod content;
pub mod detail;
pub mod fault;
pub mod filter;
pub mod fmt;
pub mod history;
pub mod icon;
pub mod pending;
pub mod rate;
pub mod sort;
pub mod torrent;

pub use choice::{flags, is_narrowed, ticked, to_fetch};
pub use content::Content;
pub use detail::{Details, FileRow, PeerRow, Transport};
pub use fault::explain;
pub use filter::Filter;
pub use history::{History, Spark};
pub use pending::{Pending, PendingFile};
pub use rate::Rate;
pub use sort::Sort;
pub use torrent::{SessionStats, Stall, State, TorrentId, TorrentRow};

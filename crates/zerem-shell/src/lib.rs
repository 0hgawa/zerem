//! Desktop plumbing: what an installed application needs from the OS and what
//! has nothing to do with what the application is *for*.
//!
//! App-neutral on purpose. Nothing here names Zerem, so promoting this folder to
//! a repository shared with Clipo and Vayou is a move rather than a rewrite.

pub mod appearance;
pub mod assoc;
pub mod attention;
pub mod corners;
pub mod disk;
pub mod file_icon;
pub mod launch;
pub mod locale;
pub mod mark;
pub mod screen;
pub mod single_instance;
pub mod update;
#[cfg(windows)]
mod window;

pub use appearance::prefers_dark;
pub use assoc::{ensure_registered, Outcome, Registration};
pub use attention::ask as ask_attention;
pub use corners::round as round_corners;
pub use disk::free_space;
pub use file_icon::{for_extension as file_icon, for_folder as folder_icon};
pub use launch::{open, open_url, reveal};
pub use locale::preferred as preferred_language;
pub use mark::wear as wear_mark;
pub use screen::{centre, fit, work_area};
pub use single_instance::{acquire, Instance};
pub use update::{install_kind, Install};

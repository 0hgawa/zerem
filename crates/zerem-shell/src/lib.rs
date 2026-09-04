//! Desktop plumbing: what an installed application needs from the OS and what
//! has nothing to do with what the application is *for*.
//!
//! App-neutral on purpose. Nothing here names Zerem, so promoting this folder to
//! a repository shared with Clipo and Vayou is a move rather than a rewrite.

pub mod assoc;
pub mod attention;
pub mod disk;
pub mod file_icon;
pub mod locale;
pub mod screen;
pub mod single_instance;

pub use assoc::{ensure_registered, Outcome, Registration};
pub use attention::ask as ask_attention;
pub use disk::free_space;
pub use file_icon::for_extension as file_icon;
pub use locale::preferred as preferred_language;
pub use screen::{fit, work_area};
pub use single_instance::{acquire, Instance};

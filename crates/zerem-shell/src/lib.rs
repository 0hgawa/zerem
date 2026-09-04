//! Desktop plumbing: what an installed application needs from the OS and what
//! has nothing to do with what the application is *for*.
//!
//! App-neutral on purpose. Nothing here names Zerem, so promoting this folder to
//! a repository shared with Clipo and Vayou is a move rather than a rewrite.

pub mod assoc;
pub mod single_instance;

pub use assoc::{ensure_registered, Outcome, Registration};
pub use single_instance::{acquire, Instance};

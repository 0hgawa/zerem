//! Message Stream Encryption — what BitTorrent clients mean by "protocol
//! encryption".
//!
//! A plain BitTorrent connection announces itself in its first nineteen bytes.
//! Equipment that shapes traffic reads those bytes at line rate and throttles
//! what it finds, which is why every mainstream client learned to obfuscate the
//! stream, and why several private trackers refuse clients that cannot.
//!
//! # What it does and does not give you
//!
//! It replaces a recognisable header with a Diffie-Hellman exchange, random
//! padding of an unstated length, and an RC4 stream. What that buys is that the
//! connection has **no fixed pattern to match on** — no constant bytes, no
//! constant length, and no info hash in the clear.
//!
//! What it does not buy is confidentiality. The group is 768 bits and the
//! cipher is RC4, both fixed by a specification from 2006, and neither would be
//! chosen today for a secret worth keeping. Anyone who already knows the info
//! hash can confirm a connection carries it. Nothing above this layer should
//! treat a peer connection as private because it came through here.
//!
//! It is also unauthenticated, and cannot be otherwise: there is nothing to
//! authenticate against on an open swarm. Somebody in the middle of the
//! connection can be both ends of it. That is true of plain BitTorrent as well,
//! and the piece hashes are what makes it not matter for the data.
//!
//! # Where the work is
//!
//! Not in the cryptography, which is two well-known primitives. It is in the
//! padding: both ends send up to 512 random bytes and never say how many, so
//! neither can read a known number of bytes and each has to scan the stream for
//! a marker it computes itself. [`handshake`] is where that lives.

pub mod dh;
pub mod handshake;
pub mod rc4;

pub use handshake::{accept, initiate, Agreed, Failure};
pub use rc4::Rc4;

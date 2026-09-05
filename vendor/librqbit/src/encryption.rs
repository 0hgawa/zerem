//! Protocol encryption. **Not upstream — added by Zerem.**
//!
//! The protocol itself is in `zerem-mse`, where it is tested. What is here is
//! the two things that can only be here: adapters that put a cipher between
//! this crate's streams and the wire, and the decision of when to use one.
//!
//! # Why the patch exists at all
//!
//! A plain BitTorrent connection announces itself in its first nineteen bytes,
//! and equipment that shapes traffic reads them at line rate. Every mainstream
//! client obfuscates the stream and several private trackers refuse those that
//! do not. This crate had none of it, and no seam to add it through: the
//! connector is a concrete struct and the handshake is written directly after
//! `connect` returns.
//!
//! See `vendor/CHANGES.md` for everything this copy alters.

use std::io::IoSliceMut;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use zerem_mse::Rc4;

use crate::vectored_traits::AsyncReadVectored;

/// What a session is willing to do about encryption.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Encryption {
    /// Never offer it. What this crate did before the patch.
    #[default]
    Off,
    /// Offer it, and accept a peer that will not. The setting for somebody
    /// whose connection is not being interfered with, who wants the swarm.
    Prefer,
    /// Refuse a peer that will not encrypt. Fewer peers, and nothing on the
    /// wire that says what this is.
    Require,
}

impl Encryption {
    /// Whether outgoing connections should try the encrypted handshake.
    pub(crate) fn outgoing(self) -> bool {
        matches!(self, Self::Prefer | Self::Require)
    }

    /// Whether a peer that opened in the clear may be talked to.
    pub(crate) fn allows_plaintext(self) -> bool {
        matches!(self, Self::Off | Self::Prefer)
    }
}

/// A reader that deciphers whatever it hands back.
pub(crate) struct Deciphering<R> {
    inner: R,
    cipher: Rc4,
}

impl<R> Deciphering<R> {
    pub(crate) fn new(inner: R, cipher: Rc4) -> Self {
        Self { inner, cipher }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Deciphering<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        // Where the buffer stood before, because that is the boundary between
        // what has already been deciphered and what has just arrived. A stream
        // cipher cannot be rewound, so touching a byte twice ruins the rest of
        // the connection.
        let already = buf.filled().len();
        ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
        this.cipher.apply(&mut buf.filled_mut()[already..]);
        Poll::Ready(Ok(()))
    }
}

impl<R: AsyncRead + Unpin> AsyncReadVectored for Deciphering<R>
where
    R: AsyncReadVectored,
{
    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        vec: &mut [IoSliceMut<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let read = ready!(Pin::new(&mut this.inner).poll_read_vectored(cx, vec))?;
        // The keystream runs across the buffers in the order they were filled,
        // not one stream per buffer.
        let mut left = read;
        for slice in vec.iter_mut() {
            if left == 0 {
                break;
            }
            let take = left.min(slice.len());
            this.cipher.apply(&mut slice[..take]);
            left -= take;
        }
        Poll::Ready(Ok(read))
    }
}

/// How much is enciphered in one go.
///
/// The buffer below holds this much at most, and a write longer than it simply
/// takes more than one call — which the caller already has to handle.
const CHUNK: usize = 32 * 1024;

/// A writer that enciphers on the way out.
///
/// # Why it buffers
///
/// A stream cipher cannot encipher the same bytes twice, and the writer
/// underneath is free to accept fewer bytes than it was offered. So what has
/// been enciphered is kept until it has all gone out, and nothing new is
/// enciphered until it has — because bytes that reach the peer out of order are
/// bytes it cannot decipher at all.
pub(crate) struct Enciphering<W> {
    inner: W,
    cipher: Rc4,
    pending: Vec<u8>,
    sent: usize,
}

impl<W> Enciphering<W> {
    pub(crate) fn new(inner: W, cipher: Rc4) -> Self {
        Self { inner, cipher, pending: Vec::new(), sent: 0 }
    }
}

impl<W: AsyncWrite + Unpin> Enciphering<W> {
    /// Push out what is already enciphered.
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        while self.sent < self.pending.len() {
            let wrote = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.pending[self.sent..]))?;
            if wrote == 0 {
                return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()));
            }
            self.sent += wrote;
        }
        self.pending.clear();
        self.sent = 0;
        Poll::Ready(Ok(()))
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for Enciphering<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        // Everything already enciphered goes first. Returning `Pending` here
        // consumes nothing, which is what the contract wants.
        ready!(this.drain(cx))?;

        let take = buf.len().min(CHUNK);
        this.pending.extend_from_slice(&buf[..take]);
        this.cipher.apply(&mut this.pending);
        // Whether it all goes now or not, those bytes are spoken for: the
        // keystream has moved past them and they cannot be offered again.
        let _ = this.drain(cx)?;
        Poll::Ready(Ok(take))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        ready!(this.drain(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        ready!(this.drain(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

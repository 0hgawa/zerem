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

    /// Whether an incoming encrypted connection is answered.
    ///
    /// The same answer as `outgoing`, and a separate name all the same: they
    /// are different questions about different connections, and a policy that
    /// ever wanted to offer one without the other would find them already
    /// apart rather than have to be prised in two.
    pub(crate) fn incoming(self) -> bool {
        matches!(self, Self::Prefer | Self::Require)
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

/// A reader that hands back `head` before anything from `inner`.
///
/// Two jobs, both of them unavoidable. Deciding whether an incoming connection
/// is encrypted means looking at its first byte — a plain one opens with `0x13`
/// and an encrypted one opens with a random public value — and there is no
/// peeking on these streams, so the byte has to be read and then given back.
/// And a peer that encrypts sends its BitTorrent handshake *inside* the
/// exchange, so those bytes arrive early and have to be put in front of the
/// reader that will parse them.
///
/// For the second job this has to sit **outside** the deciphering reader: what
/// it holds has already been deciphered, and passing it through the cipher a
/// second time would turn it back into noise and take the keystream with it.
pub(crate) struct Prefixed<R> {
    head: std::collections::VecDeque<u8>,
    inner: R,
}

impl<R> Prefixed<R> {
    pub(crate) fn new(head: Vec<u8>, inner: R) -> Self {
        Self { head: head.into(), inner }
    }

    /// Move what is held into `buf`, and say how much went.
    fn pour(&mut self, buf: &mut [u8]) -> usize {
        let take = buf.len().min(self.head.len());
        for slot in buf.iter_mut().take(take) {
            *slot = self.head.pop_front().unwrap_or_default();
        }
        take
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Prefixed<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if this.head.is_empty() {
            return Pin::new(&mut this.inner).poll_read(cx, buf);
        }
        let poured = this.pour(buf.initialize_unfilled());
        buf.advance(poured);

        // And straight on into the stream in the same call. `read_handshake`
        // reads *once* and parses whatever came back -- it does not loop -- so
        // handing it the pushed-back byte on its own hands it a handshake one
        // byte long, and every connection is refused as malformed. That is
        // exactly what happened.
        if buf.remaining() > 0 {
            if let Poll::Ready(Err(e)) = Pin::new(&mut this.inner).poll_read(cx, buf) {
                return Poll::Ready(Err(e));
            }
        }
        // Whether the stream had more or not, what was held is real and cannot
        // be reported as nothing.
        Poll::Ready(Ok(()))
    }
}

impl<R: AsyncReadVectored> AsyncReadVectored for Prefixed<R> {
    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        vec: &mut [IoSliceMut<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if this.head.is_empty() {
            return Pin::new(&mut this.inner).poll_read_vectored(cx, vec);
        }
        // Only what is held, and not the stream after it, which is the opposite
        // of what the plain read above does. Vectored buffers are filled in
        // order and the count says how far that filling got, so pouring into
        // the first and then letting the stream fill the second would claim the
        // first was full when it was not. Nothing reads a handshake this way --
        // the one caller that cannot loop uses the plain read -- so stopping
        // early here costs a round trip and never a byte.
        let mut poured = 0;
        for slice in vec.iter_mut() {
            if this.head.is_empty() {
                break;
            }
            poured += this.pour(slice);
        }
        Poll::Ready(Ok(poured))
    }
}

#[cfg(test)]
mod tests {
    use super::{Deciphering, Enciphering, Prefixed};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use zerem_mse::Rc4;

    /// A pair of ciphers keyed the same, as the two ends of one direction.
    fn pair() -> (Rc4, Rc4) {
        (Rc4::new(b"a key for a test"), Rc4::new(b"a key for a test"))
    }

    #[tokio::test]
    async fn what_is_enciphered_comes_back_out() {
        let (here, there) = tokio::io::duplex(64 * 1024);
        let (out, back) = pair();
        let mut writer = Enciphering::new(here, out);
        let mut reader = Deciphering::new(there, back);

        // No flush, because nothing in this crate flushes: it writes and
        // expects the bytes to be gone. A writer that holds anything back is a
        // writer that hangs a connection.
        let greeting = vec![0x13_u8; 68];
        writer.write_all(&greeting).await.expect("write");

        let mut got = vec![0_u8; 68];
        reader.read_exact(&mut got).await.expect("read");
        assert_eq!(got, greeting);
    }

    #[tokio::test]
    async fn a_whole_message_arrives_in_one_read() {
        // What `read_handshake` needs: it reads *once* and parses whatever came
        // back, so an adapter that hands over less than arrived breaks every
        // connection with a malformed handshake.
        let (here, there) = tokio::io::duplex(64 * 1024);
        let (out, back) = pair();
        let mut writer = Enciphering::new(here, out);
        let mut reader = Deciphering::new(there, back);

        writer.write_all(&vec![7_u8; 68]).await.expect("write");
        let mut got = vec![0_u8; 128];
        let n = reader.read(&mut got).await.expect("read");
        assert_eq!(n, 68, "the read came back short");
    }

    #[tokio::test]
    async fn a_pushed_back_byte_does_not_shorten_the_read() {
        // The same requirement, through the other adapter. This is the bug that
        // made every plain connection fail once the first byte was being
        // examined: the handshake came back one byte long.
        let (here, mut there) = tokio::io::duplex(64 * 1024);
        there.write_all(&vec![9_u8; 67]).await.expect("write");

        let mut reader = Prefixed::new(vec![0x13], here);
        let mut got = vec![0_u8; 128];
        let n = reader.read(&mut got).await.expect("read");
        assert_eq!(n, 68, "the pushed-back byte came back on its own");
        assert_eq!(got[0], 0x13);
        assert_eq!(got[1], 9);
    }
}

#[cfg(test)]
mod trickle {
    use super::{Deciphering, Enciphering};
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
    use zerem_mse::Rc4;

    /// A writer that takes only a few bytes at a time, the way a datagram
    /// transport does and a loopback socket never will.
    struct Trickle<W> {
        inner: W,
        most: usize,
    }

    impl<W: AsyncWrite + Unpin> AsyncWrite for Trickle<W> {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            let this = self.get_mut();
            let take = buf.len().min(this.most);
            Pin::new(&mut this.inner).poll_write(cx, &buf[..take])
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_flush(cx)
        }

        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
        }
    }

    #[tokio::test]
    async fn a_writer_that_takes_seven_bytes_at_a_time_still_gets_it_all_there() {
        // The case a loopback TCP socket never produces and a datagram
        // transport produces constantly. A stream cipher cannot encipher the
        // same bytes twice, so a partial write has to be remembered rather than
        // retried -- and anything remembered has to actually go out.
        let (here, there) = tokio::io::duplex(64 * 1024);
        let mut writer = Enciphering::new(Trickle { inner: here, most: 7 }, Rc4::new(b"k"));
        let mut reader = Deciphering::new(there, Rc4::new(b"k"));

        let message: Vec<u8> = (0..=255_u8).cycle().take(4096).collect();
        writer.write_all(&message).await.expect("write");
        writer.flush().await.expect("flush");

        let mut got = vec![0_u8; message.len()];
        reader.read_exact(&mut got).await.expect("read");
        assert_eq!(got, message);
    }

    #[tokio::test]
    async fn nothing_is_left_behind_once_it_has_been_flushed() {
        // The whole reason this crate now flushes. Without it the last message
        // of a connection sits in the buffer and both ends wait for the other.
        let (here, mut there) = tokio::io::duplex(64 * 1024);
        let mut writer = Enciphering::new(Trickle { inner: here, most: 3 }, Rc4::new(b"k"));

        writer.write_all(&[42_u8; 68]).await.expect("write");
        writer.flush().await.expect("flush");

        let mut got = [0_u8; 68];
        there.read_exact(&mut got).await.expect("read");
        let mut expected = [42_u8; 68];
        Rc4::new(b"k").apply(&mut expected);
        assert_eq!(got, expected, "the bytes on the wire are not the enciphered ones");
    }
}

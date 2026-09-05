//! The handshake, both ends of it.
//!
//! # The shape of it
//!
//! ```text
//!   A → B   Ya                    padding
//!   B → A   Yb                    padding
//!   A → B   req1 req2^req3        encrypted: VC, what A offers, padding, the
//!                                 BitTorrent handshake
//!   B → A   encrypted: VC, what B chose, padding
//! ```
//!
//! `Ya` and `Yb` are the key exchange. `req1` proves A worked out the same
//! shared secret; `req2^req3` says which torrent, without saying it in the
//! clear — a listener who does not already know the info hash cannot read it
//! out, and one who does can only confirm a guess.
//!
//! # Why the padding is the difficult part
//!
//! Both ends send up to 512 random bytes whose length is never stated. That is
//! what stops the handshake having a fixed size to match on — and it means
//! neither end can simply read a known number of bytes. Each has to *scan* the
//! incoming stream for a pattern it can compute for itself: B looks for `req1`,
//! and A looks for its own end of the encrypted `VC`. Getting that scan wrong
//! is the difference between a client that connects to everything and one that
//! connects to whatever happened to send no padding.

use ring::digest::{Context, SHA1_FOR_LEGACY_USE_ONLY as SHA1};
use ring::rand::{SecureRandom as _, SystemRandom};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

use crate::dh::{Half, WIDTH};
use crate::rc4::Rc4;

/// The eight zero bytes both ends encrypt so the other can find the start.
const VC: [u8; 8] = [0; 8];

/// What each end may ask for and answer with.
const PLAINTEXT: u32 = 0x01;
const RC4: u32 = 0x02;

/// The longest padding either end may send, and so the furthest a scan has to
/// look before giving up.
const MAX_PAD: usize = 512;

/// Keystream thrown away before anything real goes through it.
const DISCARD: usize = 1024;

/// What went wrong.
#[derive(Debug)]
pub enum Failure {
    /// The other end hung up, or sent fewer bytes than it promised.
    Ended,
    /// Nothing that could be the agreed marker turned up inside the padding
    /// allowance. Either the other end is not speaking this, or it does not
    /// have the same secret — which for the responder means it does not know
    /// the info hash it is asking for.
    NoSync,
    /// The other end wants a cipher this does not offer.
    NoCipherInCommon,
    /// The torrent named by the initiator is not one this end has.
    UnknownTorrent,
    /// The other end claimed a padding or payload longer than the protocol
    /// allows, which is a length nobody sane sends and a buffer nobody should
    /// allocate on a stranger's say-so.
    Absurd,
    Io(std::io::Error),
    NoRandom,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ended => f.write_str("the peer closed the connection during the handshake"),
            Self::NoSync => f.write_str("the peer did not encrypt with the secret we agreed"),
            Self::NoCipherInCommon => f.write_str("the peer offered no encryption we speak"),
            Self::UnknownTorrent => f.write_str("the peer asked for a torrent we do not have"),
            Self::Absurd => f.write_str("the peer declared a length the protocol does not allow"),
            Self::Io(e) => write!(f, "{e}"),
            Self::NoRandom => f.write_str("the system would not produce random bytes"),
        }
    }
}

impl std::error::Error for Failure {}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ring::error::Unspecified> for Failure {
    fn from(_: ring::error::Unspecified) -> Self {
        Self::NoRandom
    }
}

/// A settled connection: which torrent it is for, and the two ciphers.
///
/// Two, because RC4 keystreams run one way only. Naming them by direction
/// rather than by role is what keeps the caller from having to remember which
/// end it was.
pub struct Agreed {
    pub info_hash: [u8; 20],
    /// Applied to everything sent from here on, or `None` when both ends
    /// settled on sending in the clear.
    pub outgoing: Option<Rc4>,
    pub incoming: Option<Rc4>,
}

// Nothing here reads past the end of the handshake, which is why there is no
// buffer of leftovers on the way out. It is deliberate and it costs something:
// the scan below goes a byte at a time rather than in blocks. A block read
// would be faster and would swallow the first bytes of the peer's BitTorrent
// handshake, and a byte read here and dropped is a byte the peer never sends
// again -- which shows up as a peer that connects and then says nothing.

/// `HASH(prefix, rest…)`.
fn hash(parts: &[&[u8]]) -> [u8; 20] {
    let mut context = Context::new(&SHA1);
    for part in parts {
        context.update(part);
    }
    let mut out = [0_u8; 20];
    out.copy_from_slice(context.finish().as_ref());
    out
}

/// The cipher for one direction, keyed and skipped past its weak start.
fn cipher(role: &[u8], secret: &[u8; WIDTH], info_hash: &[u8; 20]) -> Rc4 {
    let mut rc4 = Rc4::new(&hash(&[role, secret, info_hash]));
    rc4.discard(DISCARD);
    rc4
}

/// Between zero and 512 random bytes.
fn padding() -> Result<Vec<u8>, Failure> {
    let random = SystemRandom::new();
    let mut pick = [0_u8; 2];
    random.fill(&mut pick)?;
    let mut pad = vec![0_u8; usize::from(u16::from_be_bytes(pick)) % (MAX_PAD + 1)];
    if !pad.is_empty() {
        random.fill(&mut pad)?;
    }
    Ok(pad)
}

/// Read until `pattern` has been seen, and hand back everything after it.
///
/// The heart of the thing. Neither end knows how much padding precedes the
/// marker it is waiting for, so it reads a byte at a time and slides a window
/// along — and it has to stop looking somewhere, or a peer that says nothing
/// keeps a connection open for ever.
async fn scan<S: AsyncRead + Unpin>(reader: &mut S, pattern: &[u8]) -> Result<(), Failure> {
    let mut window = Vec::with_capacity(pattern.len());
    // The marker can begin anywhere up to the end of the allowance, so the last
    // byte of it can arrive that much further along.
    let allowance = MAX_PAD + pattern.len();

    for _ in 0..allowance {
        let mut byte = [0_u8; 1];
        if reader.read_exact(&mut byte).await.is_err() {
            return Err(Failure::Ended);
        }
        if window.len() == pattern.len() {
            window.remove(0);
        }
        window.push(byte[0]);
        if window == pattern {
            return Ok(());
        }
    }
    Err(Failure::NoSync)
}

/// Read exactly `n` bytes.
async fn take<S: AsyncRead + Unpin>(reader: &mut S, n: usize) -> Result<Vec<u8>, Failure> {
    let mut buf = vec![0_u8; n];
    reader.read_exact(&mut buf).await.map_err(|_| Failure::Ended)?;
    Ok(buf)
}

/// Open an encrypted connection to a peer, carrying `first` — the BitTorrent
/// handshake — inside the last encrypted message.
///
/// Sending it inside rather than after is not an optimisation. It is what the
/// specification asks for, and it is why the exchange is four messages and not
/// five: the payload rides along in the one A was sending anyway.
///
/// # Errors
///
/// When the peer will not agree, does not answer, or is not speaking this.
pub async fn initiate<R, W>(
    reader: &mut R,
    writer: &mut W,
    info_hash: &[u8; 20],
    first: &[u8],
) -> Result<Agreed, Failure>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    // The field that carries it is two bytes wide. A caller passing more than
    // fits would have it quietly cut in half and spend the connection wondering
    // why the peer hung up.
    if u16::try_from(first.len()).is_err() {
        return Err(Failure::Absurd);
    }

    let ours = Half::new()?;
    writer.write_all(ours.public()).await?;
    writer.write_all(&padding()?).await?;
    writer.flush().await?;

    let mut theirs = [0_u8; WIDTH];
    reader.read_exact(&mut theirs).await.map_err(|_| Failure::Ended)?;
    let secret = ours.secret(&theirs);

    // Keyed before the padding is even read: the response cannot be recognised
    // without them, because what is being looked for is VC already encrypted.
    let mut send = cipher(b"keyA", &secret, info_hash);
    let mut receive = cipher(b"keyB", &secret, info_hash);

    let mut message = Vec::new();
    message.extend_from_slice(&hash(&[b"req1", &secret]));
    let named = hash(&[b"req2", info_hash]);
    let masked = hash(&[b"req3", &secret]);
    message.extend(named.iter().zip(masked).map(|(a, b)| a ^ b));

    // Everything from here is under the cipher, in one buffer, because RC4 is a
    // keystream and the order it is applied in is the order it must be read in.
    let pad = padding()?;
    let mut sealed = Vec::new();
    sealed.extend_from_slice(&VC);
    sealed.extend_from_slice(&(PLAINTEXT | RC4).to_be_bytes());
    sealed.extend_from_slice(&(pad.len() as u16).to_be_bytes());
    sealed.extend_from_slice(&pad);
    sealed.extend_from_slice(&(first.len() as u16).to_be_bytes());
    sealed.extend_from_slice(first);
    send.apply(&mut sealed);

    message.extend_from_slice(&sealed);
    writer.write_all(&message).await?;
    writer.flush().await?;

    // What B's encrypted VC will look like, worked out from a copy of the
    // cipher so the real one stays at the start of its keystream — the bytes
    // before the marker are B's padding and were never encrypted.
    let mut expected = VC;
    cipher(b"keyB", &secret, info_hash).apply(&mut expected);
    scan(reader, &expected).await?;
    // Those eight bytes were matched, not decrypted, and the keystream was
    // spent on them all the same. Walking the real cipher past them is what
    // keeps the two ends in step from here on.
    let mut matched = VC;
    receive.apply(&mut matched);

    let mut chosen = take(reader, 4).await?;
    receive.apply(&mut chosen);
    let chosen = u32::from_be_bytes([chosen[0], chosen[1], chosen[2], chosen[3]]);

    let mut raw = take(reader, 2).await?;
    receive.apply(&mut raw);
    let pad_len = usize::from(u16::from_be_bytes([raw[0], raw[1]]));
    if pad_len > MAX_PAD {
        return Err(Failure::Absurd);
    }
    let mut pad = take(reader, pad_len).await?;
    receive.apply(&mut pad);

    settle(chosen, info_hash, send, receive)
}

/// Accept an encrypted connection.
///
/// `known` answers "do I have this torrent?", because the initiator names it by
/// a hash of the info hash and the only way to read that is to try the ones
/// this end already has.
///
/// # Errors
///
/// When the peer is not speaking this, names a torrent this end does not have,
/// or will not settle on a cipher.
pub async fn accept<R, W, K>(
    reader: &mut R,
    writer: &mut W,
    known: K,
    first: &mut Vec<u8>,
) -> Result<Agreed, Failure>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    K: Fn() -> Vec<[u8; 20]>,
{
    let mut theirs = [0_u8; WIDTH];
    reader.read_exact(&mut theirs).await.map_err(|_| Failure::Ended)?;

    let ours = Half::new()?;
    writer.write_all(ours.public()).await?;
    writer.write_all(&padding()?).await?;
    writer.flush().await?;

    let secret = ours.secret(&theirs);
    scan(reader, &hash(&[b"req1", &secret])).await?;

    // Which torrent. The initiator masked it, so the only way through is to
    // unmask with the shared secret and compare against what this end holds.
    let asked = take(reader, 20).await?;
    let masked = hash(&[b"req3", &secret]);
    let wanted: Vec<u8> = asked.iter().zip(masked).map(|(a, b)| a ^ b).collect();
    let info_hash = known()
        .into_iter()
        .find(|candidate| hash(&[b"req2", candidate]) == wanted.as_slice())
        .ok_or(Failure::UnknownTorrent)?;

    let mut receive = cipher(b"keyA", &secret, &info_hash);
    let mut send = cipher(b"keyB", &secret, &info_hash);

    // The initiator's VC comes first and is already encrypted, so no scan is
    // needed here — this end knows exactly where it starts.
    let mut vc = take(reader, 8).await?;
    receive.apply(&mut vc);
    if vc != VC {
        return Err(Failure::NoSync);
    }

    let mut offered = take(reader, 4).await?;
    receive.apply(&mut offered);
    let offered = u32::from_be_bytes([offered[0], offered[1], offered[2], offered[3]]);

    let mut raw = take(reader, 2).await?;
    receive.apply(&mut raw);
    let pad_len = usize::from(u16::from_be_bytes([raw[0], raw[1]]));
    if pad_len > MAX_PAD {
        return Err(Failure::Absurd);
    }
    let mut pad = take(reader, pad_len).await?;
    receive.apply(&mut pad);

    // Prefer RC4: the whole reason to be here is that the traffic should not
    // look like BitTorrent, and plaintext after an encrypted handshake looks
    // exactly like BitTorrent.
    let chosen = if offered & RC4 != 0 {
        RC4
    } else if offered & PLAINTEXT != 0 {
        PLAINTEXT
    } else {
        return Err(Failure::NoCipherInCommon);
    };

    let mut raw = take(reader, 2).await?;
    receive.apply(&mut raw);
    let payload = usize::from(u16::from_be_bytes([raw[0], raw[1]]));
    let mut carried = take(reader, payload).await?;
    receive.apply(&mut carried);
    first.clear();
    first.extend_from_slice(&carried);

    let pad = padding()?;
    let mut sealed = Vec::new();
    sealed.extend_from_slice(&VC);
    sealed.extend_from_slice(&chosen.to_be_bytes());
    sealed.extend_from_slice(&(pad.len() as u16).to_be_bytes());
    sealed.extend_from_slice(&pad);
    send.apply(&mut sealed);
    writer.write_all(&sealed).await?;
    writer.flush().await?;

    settle(chosen, &info_hash, send, receive)
}

/// Turn the agreed number into the pair of ciphers the caller keeps.
const fn settle(chosen: u32, info_hash: &[u8; 20], send: Rc4, receive: Rc4) -> Result<Agreed, Failure> {
    let (outgoing, incoming) = match chosen {
        RC4 => (Some(send), Some(receive)),
        PLAINTEXT => (None, None),
        // A peer that answers with something neither side named, or with both
        // at once, is one whose stream cannot be read either way.
        _ => return Err(Failure::NoCipherInCommon),
    };
    Ok(Agreed { info_hash: *info_hash, outgoing, incoming })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{accept, initiate, padding, scan, Failure, MAX_PAD, VC};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::time::timeout;

    const HASH_A: [u8; 20] = [0xAA; 20];
    const HASH_B: [u8; 20] = [0xBB; 20];

    /// Run both ends against each other over a pipe and hand back what each got.
    async fn meet(
        initiator: [u8; 20],
        held: Vec<[u8; 20]>,
        payload: &'static [u8],
    ) -> (Result<super::Agreed, Failure>, Result<super::Agreed, Failure>, Vec<u8>) {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (mut read_a, mut write_a) = tokio::io::split(a);
        let there = tokio::spawn(async move {
            let (mut read_b, mut write_b) = tokio::io::split(b);
            let mut carried = Vec::new();
            let out = accept(&mut read_b, &mut write_b, move || held.clone(), &mut carried).await;
            // `b` is dropped here, on purpose. Handing it back out of the task
            // would keep the far end of the pipe open past the responder's own
            // life, and an initiator waiting on a reply that is not coming
            // would never be told the line had closed -- which is a test that
            // hangs rather than fails, on exactly the paths where the responder
            // refuses.
            (out, carried)
        });
        let here = initiate(&mut read_a, &mut write_a, &initiator, payload).await;
        let (there, carried) = there.await.expect("the responder panicked");
        (here, there, carried)
    }

    #[tokio::test]
    async fn the_two_ends_agree() {
        let (here, there, carried) = meet(HASH_A, vec![HASH_A], b"BitTorrent handshake").await;
        let here = here.expect("the initiator failed");
        let there = there.expect("the responder failed");

        assert_eq!(here.info_hash, HASH_A);
        assert_eq!(there.info_hash, HASH_A, "the responder picked the wrong torrent");
        // The payload rides inside the last encrypted message, which is the
        // whole reason this is four messages rather than five.
        assert_eq!(carried, b"BitTorrent handshake");
        assert!(here.outgoing.is_some(), "they settled on plaintext");
    }

    #[tokio::test]
    async fn what_one_encrypts_the_other_reads() {
        // The keys have to be crossed: what the initiator sends with is what
        // the responder receives with. Two ends that key the same direction
        // twice still complete a handshake and then talk gibberish, which is
        // the failure that looks like a broken peer rather than a broken client.
        let (here, there, _) = meet(HASH_A, vec![HASH_A], b"x").await;
        let (mut here, mut there) = (here.expect("initiator"), there.expect("responder"));

        let mut message = b"a piece request".to_vec();
        here.outgoing.as_mut().expect("keyed").apply(&mut message);
        there.incoming.as_mut().expect("keyed").apply(&mut message);
        assert_eq!(message, b"a piece request");

        let mut back = b"a piece".to_vec();
        there.outgoing.as_mut().expect("keyed").apply(&mut back);
        here.incoming.as_mut().expect("keyed").apply(&mut back);
        assert_eq!(back, b"a piece");
    }

    #[tokio::test]
    async fn the_torrent_is_picked_out_of_everything_this_end_holds() {
        // The initiator never says which one in the clear. The responder has to
        // find it by trying the ones it has, and it has to find the right one
        // when it is not the first.
        let held = vec![[0x11; 20], HASH_B, [0x22; 20], HASH_A];
        let (_, there, _) = meet(HASH_A, held, b"x").await;
        assert_eq!(there.expect("responder").info_hash, HASH_A);
    }

    #[tokio::test]
    async fn a_torrent_this_end_does_not_have_is_refused() {
        let (_, there, _) = meet(HASH_A, vec![HASH_B], b"x").await;
        assert!(matches!(there, Err(Failure::UnknownTorrent)), "it accepted a stranger's torrent");
    }

    #[tokio::test]
    async fn nothing_of_the_torrent_appears_on_the_wire() {
        // The point of masking it. Somebody watching who does not already know
        // the info hash must not be able to read it out of the handshake, or
        // the encryption has hidden the letter and printed the address on the
        // envelope.
        //
        // The watcher has to answer with something, and only with that. The
        // initiator cannot write the interesting half until it has a public
        // value to work a secret out of -- so a listener that says nothing
        // captures the key exchange and stops, which would let this pass
        // without ever seeing the part that carries the torrent.
        //
        // Ninety-six bytes of anything will do. Nothing checks them, and the
        // secret they lead to being wrong is exactly why the rest is never
        // answered.
        let (ours, mut watched) = tokio::io::duplex(64 * 1024);
        let sending = tokio::spawn(async move {
            let (mut read, mut write) = tokio::io::split(ours);
            initiate(&mut read, &mut write, &HASH_A, b"BitTorrent protocol").await
        });

        let mut seen = Vec::new();
        let mut opening = [0_u8; 96];
        watched.read_exact(&mut opening).await.expect("the key exchange");
        seen.extend_from_slice(&opening);
        watched.write_all(&[0x5A; 96]).await.expect("answer");

        let mut chunk = [0_u8; 4096];
        while let Ok(Ok(n)) = timeout(Duration::from_millis(250), watched.read(&mut chunk)).await {
            if n == 0 {
                break;
            }
            seen.extend_from_slice(&chunk[..n]);
        }
        sending.abort();

        // Ya, its padding, the two hashes and the encrypted block. That is 171
        // bytes before a single byte of padding, and the padding is what makes
        // the total unpredictable -- which is the point of it.
        assert!(seen.len() >= 171, "only {} bytes were captured", seen.len());
        assert!(!seen.windows(20).any(|w| w == HASH_A), "the info hash went out in the clear");
        assert!(!seen.windows(19).any(|w| w == b"BitTorrent protocol"), "the payload went out in the clear");
        // And the thing that started all of this: the nineteen bytes a plain
        // connection opens with, which is what traffic shapers match on.
        assert!(
            !seen.windows(20).any(|w| w == b"\x13BitTorrent protocol"),
            "the protocol header went out in the clear"
        );
    }

    #[tokio::test]
    async fn a_peer_that_says_nothing_does_not_hold_the_line_for_ever() {
        // The scan has to give up. Without a bound, one silent peer is one
        // connection that never returns, and a client that meets a few of those
        // stops connecting to anybody.
        let (ours, mut theirs) = tokio::io::duplex(64 * 1024);
        let listening = tokio::spawn(async move {
            let (mut read, mut write) = tokio::io::split(ours);
            let mut carried = Vec::new();
            accept(&mut read, &mut write, || vec![HASH_A], &mut carried).await
        });

        // Padding for ever and never the marker -- while staying on the line
        // and reading what comes back. A peer that hangs up instead would break
        // the pipe, and a broken pipe is a different failure that would let
        // this pass whether the scan gives up or not.
        theirs.write_all(&vec![0x7F; MAX_PAD * 4]).await.expect("flood");
        let mut sink = vec![0_u8; 4096];
        let _ = timeout(Duration::from_millis(250), theirs.read(&mut sink)).await;

        let out = listening.await.expect("the responder panicked");
        assert!(matches!(out, Err(Failure::NoSync)), "the scan did not give up");
    }

    #[tokio::test]
    async fn padding_is_random_and_within_its_allowance() {
        // Fixed-length padding would give the handshake a fixed size, which is
        // the pattern this exists to not have.
        let mut lengths = std::collections::BTreeSet::new();
        for _ in 0..40 {
            let pad = padding().expect("random");
            assert!(pad.len() <= MAX_PAD, "{} bytes of padding", pad.len());
            lengths.insert(pad.len());
        }
        assert!(lengths.len() > 20, "only {} distinct lengths in forty", lengths.len());
    }

    #[tokio::test]
    async fn the_scan_finds_a_marker_that_arrives_late() {
        // The marker can sit behind up to 512 bytes of padding, and a scan that
        // only checks the front connects to peers that send none.
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        tokio::spawn(async move {
            let _ = b.write_all(&vec![0x00; MAX_PAD]).await;
            let _ = b.write_all(b"marker").await;
        });
        assert!(scan(&mut a, b"marker").await.is_ok());
    }

    #[test]
    fn the_verification_constant_is_what_the_specification_says() {
        assert_eq!(VC, [0; 8]);
    }
}

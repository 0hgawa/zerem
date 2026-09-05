//! RC4, which MSE calls its cipher.
//!
//! Written out rather than depended on. It is twenty lines with published test
//! vectors, and taking a crate for that is a supply chain in exchange for
//! nothing.
//!
//! # This is not a general-purpose cipher
//!
//! RC4 is broken for confidentiality and has been for years: its keystream is
//! biased in the first bytes and distinguishable well beyond them. It is here
//! because the BitTorrent protocol-encryption specification says RC4, every
//! client on the network implements exactly that, and a peer that offers
//! anything else is a peer nobody can talk to.
//!
//! What the encryption is actually for is worth being clear about, because it
//! decides how much the weakness matters: it makes the traffic **not look like
//! BitTorrent** to equipment that shapes it by pattern. It is obfuscation with
//! a key exchange, not privacy. Nothing in this application should treat a peer
//! connection as confidential because it went through here.

/// The state of one direction of a stream.
///
/// One per direction: RC4 is a stream cipher and its keystream cannot be
/// rewound or shared, so reading and writing each need their own.
pub struct Rc4 {
    s: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    /// Key it.
    #[must_use]
    pub fn new(key: &[u8]) -> Self {
        let mut s = [0_u8; 256];
        for (n, slot) in s.iter_mut().enumerate() {
            *slot = n as u8;
        }
        let mut j = 0_u8;
        for n in 0..256 {
            j = j.wrapping_add(s[n]).wrapping_add(key[n % key.len()]);
            s.swap(n, j as usize);
        }
        Self { s, i: 0, j: 0 }
    }

    /// Encrypt or decrypt in place — with a stream cipher they are one
    /// operation, which is also why applying it twice gives back what went in.
    pub fn apply(&mut self, buf: &mut [u8]) {
        for byte in buf {
            self.i = self.i.wrapping_add(1);
            self.j = self.j.wrapping_add(self.s[self.i as usize]);
            self.s.swap(self.i as usize, self.j as usize);
            let at = self.s[self.i as usize].wrapping_add(self.s[self.j as usize]);
            *byte ^= self.s[at as usize];
        }
    }

    /// Throw away `n` bytes of keystream.
    ///
    /// MSE discards a kilobyte before anything real goes through, and it is not
    /// ceremony: RC4's first output bytes leak information about the key, which
    /// is the weakness that broke WEP. Skipping past them is the mitigation the
    /// specification settled on.
    pub fn discard(&mut self, n: usize) {
        let mut sink = [0_u8; 64];
        let mut left = n;
        while left > 0 {
            let step = left.min(sink.len());
            self.apply(&mut sink[..step]);
            left -= step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Rc4;

    /// Encrypt `text` under `key` and hand back the bytes.
    fn run(key: &[u8], text: &[u8]) -> Vec<u8> {
        let mut out = text.to_vec();
        Rc4::new(key).apply(&mut out);
        out
    }

    #[test]
    fn the_published_vectors_come_out_right() {
        // The three everybody quotes. A cipher that is subtly wrong still
        // produces convincing-looking bytes and still completes a handshake
        // with itself — it simply cannot talk to any other client on Earth.
        assert_eq!(run(b"Key", b"Plaintext"), [0xBB, 0xF3, 0x16, 0xE8, 0xD9, 0x40, 0xAF, 0x0A, 0xD3]);
        assert_eq!(run(b"Wiki", b"pedia"), [0x10, 0x21, 0xBF, 0x04, 0x20]);
        assert_eq!(
            run(b"Secret", b"Attack at dawn"),
            [0x45, 0xA0, 0x1F, 0x64, 0x5F, 0xC3, 0x5B, 0x38, 0x35, 0x52, 0x54, 0x4B, 0x9B, 0xF5]
        );
    }

    #[test]
    fn the_keystream_matches_rfc_6229() {
        // The first sixteen bytes under a forty-bit key, from the RFC's table.
        // The vectors above are short; this one reaches past the point where a
        // mistake in the swap would still coincidentally agree.
        let mut out = [0_u8; 16];
        Rc4::new(&[0x01, 0x02, 0x03, 0x04, 0x05]).apply(&mut out);
        assert_eq!(
            out,
            [0xB2, 0x39, 0x63, 0x05, 0xF0, 0x3D, 0xC0, 0x27, 0xCC, 0xC3, 0x52, 0x4A, 0x0A, 0x11, 0x18, 0xA8]
        );
    }

    #[test]
    fn applying_it_twice_gives_back_what_went_in() {
        // Which is why one object cannot serve both directions of a connection,
        // and the reason each end of the handshake keys two of them.
        let text = b"the quick brown fox jumps over the lazy dog";
        let mut buf = text.to_vec();
        Rc4::new(b"zerem").apply(&mut buf);
        assert_ne!(buf.as_slice(), text.as_slice(), "it did not encrypt anything");
        Rc4::new(b"zerem").apply(&mut buf);
        assert_eq!(buf.as_slice(), text.as_slice());
    }

    #[test]
    fn discarding_is_the_same_as_encrypting_and_throwing_away() {
        // The kilobyte skip is on the hot path of every connection, so it is
        // done in blocks rather than a byte at a time — and a block loop that
        // drifts by one byte desynchronises the whole stream, which shows up as
        // a peer that connects and then talks nonsense.
        let mut skipped = Rc4::new(b"key");
        skipped.discard(1000);
        let mut long_way = Rc4::new(b"key");
        long_way.apply(&mut vec![0_u8; 1000]);

        let (mut a, mut b) = ([0_u8; 32], [0_u8; 32]);
        skipped.apply(&mut a);
        long_way.apply(&mut b);
        assert_eq!(a, b);
    }
}

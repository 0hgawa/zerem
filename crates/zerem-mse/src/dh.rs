//! The key exchange the two ends do before they can agree on anything.
//!
//! Diffie-Hellman over the 768-bit prime from RFC 2409, which is what the
//! BitTorrent protocol-encryption specification names. Both ends send a public
//! value in the clear, and both arrive at a shared secret that the value alone
//! does not give away.
//!
//! # 768 bits is not a lot, and that is on purpose
//!
//! It would be a poor choice for a secret worth keeping — the group is small
//! enough that a well-resourced attacker can break it. It is the right choice
//! here because the thing being protected is *what protocol this is*, against
//! equipment doing pattern matching at line rate, and because the number is
//! fixed by a specification every other client already implements. Picking a
//! stronger group would mean picking a group nobody can talk to.

use num_bigint::BigUint;
use ring::rand::{SecureRandom as _, SystemRandom};

/// The modulus: the 768-bit MODP group, RFC 2409 §6.1.
const P: [u8; 96] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xC9, 0x0F, 0xDA, 0xA2, 0x21, 0x68, 0xC2, 0x34, 0xC4,
    0xC6, 0x62, 0x8B, 0x80, 0xDC, 0x1C, 0xD1, 0x29, 0x02, 0x4E, 0x08, 0x8A, 0x67, 0xCC, 0x74, 0x02, 0x0B,
    0xBE, 0xA6, 0x3B, 0x13, 0x9B, 0x22, 0x51, 0x4A, 0x08, 0x79, 0x8E, 0x34, 0x04, 0xDD, 0xEF, 0x95, 0x19,
    0xB3, 0xCD, 0x3A, 0x43, 0x1B, 0x30, 0x2B, 0x0A, 0x6D, 0xF2, 0x5F, 0x14, 0x37, 0x4F, 0xE1, 0x35, 0x6D,
    0x6D, 0x51, 0xC2, 0x45, 0xE4, 0x85, 0xB5, 0x76, 0x62, 0x5E, 0x7E, 0xC6, 0xF4, 0x4C, 0x42, 0xE9, 0xA6,
    0x3A, 0x36, 0x21, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x05, 0x63,
];

/// The generator. Two, as the specification says.
const G: u8 = 2;

/// How wide every public value and every shared secret is, in bytes.
///
/// Always this, never "however many the number needed": a secret that happens
/// to start with a zero byte is a 95-byte number, and hashing 95 bytes where
/// the other end hashed 96 produces two ends that agree on the maths and
/// disagree on every key drawn from it. It is the classic way to write a
/// Diffie-Hellman that works nineteen times out of twenty.
pub const WIDTH: usize = 96;

/// One end's half of the exchange.
pub struct Half {
    private: BigUint,
    public: [u8; WIDTH],
}

impl Half {
    /// Draw a private value and work out what to send.
    ///
    /// # Errors
    ///
    /// When the operating system will not produce random bytes, which is a
    /// machine with no entropy source and not a condition to paper over: a
    /// predictable private value makes the whole exchange theatre.
    pub fn new() -> Result<Self, ring::error::Unspecified> {
        // 160 bits, which is what the specification asks for. Wider buys
        // nothing against a 768-bit group and costs time on every connection.
        let mut bytes = [0_u8; 20];
        SystemRandom::new().fill(&mut bytes)?;
        let private = BigUint::from_bytes_be(&bytes);
        Ok(Self { public: pad(&BigUint::from(G).modpow(&private, &prime())), private })
    }

    /// What to put on the wire.
    #[must_use]
    pub const fn public(&self) -> &[u8; WIDTH] {
        &self.public
    }

    /// What both ends end up holding, from the other end's public value.
    #[must_use]
    pub fn secret(&self, theirs: &[u8; WIDTH]) -> [u8; WIDTH] {
        pad(&BigUint::from_bytes_be(theirs).modpow(&self.private, &prime()))
    }
}

fn prime() -> BigUint {
    BigUint::from_bytes_be(&P)
}

/// A number as exactly `WIDTH` bytes, big-endian, zero-filled at the front.
fn pad(n: &BigUint) -> [u8; WIDTH] {
    let bytes = n.to_bytes_be();
    let mut out = [0_u8; WIDTH];
    // A number wider than the modulus cannot happen — everything here is a
    // residue — so the tail is where it goes and the front stays zero.
    let at = WIDTH.saturating_sub(bytes.len());
    out[at..].copy_from_slice(&bytes[bytes.len().saturating_sub(WIDTH)..]);
    out
}

#[cfg(test)]
mod tests {
    use super::{pad, prime, Half, G, WIDTH};
    use num_bigint::BigUint;

    #[test]
    fn both_ends_arrive_at_the_same_secret() {
        // The whole point of the exchange, and the one thing that cannot be
        // checked against a published vector: the values are random every time.
        let (a, b) = (Half::new().expect("random"), Half::new().expect("random"));
        assert_eq!(a.secret(b.public()), b.secret(a.public()));
    }

    #[test]
    fn two_exchanges_are_not_the_same_exchange() {
        // A private value that came out the same twice would mean the random
        // source is not one, and every connection would share a secret.
        let (a, b) = (Half::new().expect("random"), Half::new().expect("random"));
        assert_ne!(a.public(), b.public());
    }

    #[test]
    fn everything_on_the_wire_is_the_full_width() {
        // Roughly one exchange in every two hundred and fifty produces a value
        // with a leading zero byte, and an implementation that sends it short
        // fails only against those. That is a bug that ships.
        for _ in 0..8 {
            let half = Half::new().expect("random");
            assert_eq!(half.public().len(), WIDTH);
            assert_eq!(half.secret(half.public()).len(), WIDTH);
        }
    }

    #[test]
    fn a_short_number_is_padded_at_the_front() {
        // Where the zeroes go decides the number, so this is the direction that
        // matters: 258 is a two-byte number sitting at the end of ninety-six.
        let padded = pad(&BigUint::from(258_u32));
        assert_eq!(padded[WIDTH - 2..], [0x01, 0x02]);
        assert!(padded[..WIDTH - 2].iter().all(|&b| b == 0), "the front is not clear");
    }

    #[test]
    fn the_prime_is_the_one_the_specification_names() {
        // Transcribed by hand from the RFC, and a single wrong nibble gives a
        // group that works perfectly against itself and against nothing else.
        // Both ends of the published shape: it is 768 bits, and the last four
        // bytes are the ones that make it prime rather than round.
        let p = prime();
        assert_eq!(p.bits(), 768);
        assert_eq!(&p.to_bytes_be()[92..], &[0x00, 0x09, 0x05, 0x63]);
        assert_eq!(&p.to_bytes_be()[..8], &[0xFF; 8]);
    }

    #[test]
    fn the_generator_actually_generates() {
        // Two raised to a private value has to land inside the group and not on
        // one of the two answers that would give the secret away.
        let one = BigUint::from(1_u32);
        let public = BigUint::from_bytes_be(Half::new().expect("random").public());
        assert!(public > one, "the public value is degenerate");
        assert!(public < prime() - one);
        assert_eq!(G, 2);
    }
}

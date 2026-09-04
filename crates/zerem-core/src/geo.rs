//! Which country an address was handed to.
//!
//! The table is built by `tools/geo` from what the five regional registries
//! publish daily, and carried inside the binary — a flag that arrives over the
//! network is a flag that is missing when the peer list is open. It answers at
//! the level a flag is about: a /24 for IPv4, a /32 for IPv6.
//!
//! It is a registry answer, not a geolocation. An address allocated to a German
//! ISP reads as Germany even if the machine using it is elsewhere, which is the
//! same thing every other client shows and is right far more often than not.
//! What it will never do is guess: an address in space nobody has been given
//! comes back as `None`, and no flag is drawn.
//!
//! # Reading it
//!
//! The stream is a chain of differences, so it cannot be searched directly. An
//! index of every five-hundred-and-twelfth key can be, which turns a lookup
//! into a binary search over a few hundred rows and a short walk from there.
//! Nothing is decoded until the first address is asked about, and the answer to
//! that is the whole file's parsing: a header of offsets, done once.

use std::net::IpAddr;
use std::sync::OnceLock;

/// The table, as `tools/geo` wrote it.
///
/// Refreshing it is running that tool again; there is no build step and no
/// generated source, because a table is data and data does not need to become
/// Rust to be read.
static BLOB: &[u8] = include_bytes!("../../../assets/geoip.bin");

/// What every file this reader understands starts with.
const MAGIC: &[u8; 5] = b"ZGEO1";

/// One entry in the skip index: a key and where its entry begins.
const INDEX_ROW: usize = 8;

/// The two-letter code for an address, if the registries have given it out.
///
/// `None` for unallocated space, for the private and link-local ranges nobody
/// is delegated, and for a table that failed to parse — all of which mean the
/// same thing to the caller, which is that there is nothing to draw.
#[must_use]
pub fn country(addr: IpAddr) -> Option<&'static str> {
    let table = table()?;
    let (key, family) = match addr {
        IpAddr::V4(v4) => (u64::from(u32::from_be_bytes(v4.octets())), &table.v4),
        IpAddr::V6(v6) => {
            let octets = v6.octets();
            let head = u32::from_be_bytes([octets[0], octets[1], octets[2], octets[3]]);
            (u64::from(head), &table.v6)
        }
    };
    let at = family.lookup(key >> family.shift)?;
    let start = (at as usize - 1) * 2;
    std::str::from_utf8(table.codes.get(start..start + 2)?).ok()
}

/// One address family's slice of the table.
struct Family {
    shift: u8,
    index: &'static [u8],
    stream: &'static [u8],
}

impl Family {
    /// The country byte covering `key`, or `None` where nobody is.
    ///
    /// A zero in the stream means unallocated, which is spelled out rather than
    /// left as a hole so that walking forward can stop at it — the alternative
    /// is a second structure listing the holes, and this is one byte.
    fn lookup(&self, key: u64) -> Option<u8> {
        let rows = self.index.len() / INDEX_ROW;
        let row_key = |row: usize| {
            let at = row * INDEX_ROW;
            u64::from(u32::from_le_bytes([
                self.index[at],
                self.index[at + 1],
                self.index[at + 2],
                self.index[at + 3],
            ]))
        };

        // The last index row at or before the key. Before the first one there
        // is no table to walk, which is an address below every allocation.
        let mut lo = 0usize;
        let mut hi = rows;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if row_key(mid) <= key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let row = lo.checked_sub(1)?;

        let at = row * INDEX_ROW;
        let offset = u32::from_le_bytes([
            self.index[at + 4],
            self.index[at + 5],
            self.index[at + 6],
            self.index[at + 7],
        ]) as usize;

        // The entry the index points at carries a delta from the entry before
        // it, which is not on this side of the search — its key is the index
        // row's, and the walk starts after it.
        let mut cursor = offset;
        let mut current = row_key(row);
        let mut country = *self.stream.get(skip_varint(self.stream, &mut cursor)?)?;
        cursor += 1;

        // Forward from there, up to the next index row.
        while cursor < self.stream.len() {
            let mut probe = cursor;
            let Some(value_at) = skip_varint(self.stream, &mut probe) else { break };
            let next = current + decode(self.stream, cursor);
            if next > key {
                break;
            }
            current = next;
            country = *self.stream.get(value_at)?;
            cursor = value_at + 1;
        }

        (country != 0).then_some(country)
    }
}

/// Advance `at` past one varint, returning where its value byte sits.
fn skip_varint(stream: &[u8], at: &mut usize) -> Option<usize> {
    while *stream.get(*at)? >= 0x80 {
        *at += 1;
    }
    *at += 1;
    Some(*at)
}

/// The varint beginning at `at`.
fn decode(stream: &[u8], mut at: usize) -> u64 {
    let mut value = 0u64;
    let mut shift = 0u32;
    while let Some(&byte) = stream.get(at) {
        value |= u64::from(byte & 0x7f) << shift;
        at += 1;
        if byte < 0x80 {
            break;
        }
        shift += 7;
    }
    value
}

struct Table {
    codes: &'static [u8],
    v4: Family,
    v6: Family,
}

/// Parsed once, on the first address anybody asks about.
///
/// `None` for a table that does not parse, which the caller reads as "no flag"
/// — a client that will not start because a decoration is malformed is worse
/// than one that draws no flags.
fn table() -> Option<&'static Table> {
    static PARSED: OnceLock<Option<Table>> = OnceLock::new();
    PARSED.get_or_init(|| parse(BLOB)).as_ref()
}

fn parse(blob: &'static [u8]) -> Option<Table> {
    if blob.get(..5)? != MAGIC {
        return None;
    }
    let mut at = 5;
    let count = usize::from(u16::from_le_bytes([blob[at], blob[at + 1]]));
    at += 2;
    let codes = blob.get(at..at + count * 2)?;
    at += count * 2;

    let v4 = parse_family(blob, &mut at)?;
    let v6 = parse_family(blob, &mut at)?;
    Some(Table { codes, v4, v6 })
}

fn parse_family(blob: &'static [u8], at: &mut usize) -> Option<Family> {
    let shift = *blob.get(*at)?;
    *at += 1;
    // The entry count is written for the tool's own reporting. The walk is
    // bounded by the stream itself, so reading it here would only be a number
    // to disbelieve.
    *at += 4;
    let rows = word(blob, at)?;
    let index = blob.get(*at..*at + rows * INDEX_ROW)?;
    *at += rows * INDEX_ROW;
    let length = word(blob, at)?;
    let stream = blob.get(*at..*at + length)?;
    *at += length;
    Some(Family { shift, index, stream })
}

/// One little-endian `u32`, and past it.
fn word(blob: &[u8], at: &mut usize) -> Option<usize> {
    let value = u32::from_le_bytes(blob.get(*at..*at + 4)?.try_into().ok()?) as usize;
    *at += 4;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::{country, table};
    use std::net::IpAddr;

    fn at(text: &str) -> Option<&'static str> {
        country(text.parse::<IpAddr>().expect("a literal address"))
    }

    #[test]
    fn the_table_that_ships_parses() {
        assert!(table().is_some(), "assets/geoip.bin is malformed - rerun tools/geo");
    }

    #[test]
    fn known_addresses_land_in_the_country_the_registries_gave_them_to() {
        // Each of these was read out of the delegation files by hand, so this
        // tests the reader against the source rather than against itself.
        assert_eq!(at("8.8.8.8"), Some("US"), "8.8.8.0/24 is ARIN, US");
        assert_eq!(at("1.1.1.1"), Some("AU"), "1.1.1.0/24 is APNIC, AU");
        assert_eq!(at("200.160.2.3"), Some("BR"), "200.160.0.0/20 is LACNIC, BR");
        assert_eq!(at("195.201.0.1"), Some("DE"), "195.201.0.0/16 is RIPE, DE");
    }

    #[test]
    fn space_nobody_has_been_given_gets_no_flag() {
        // Rather than the country of whatever allocation happens to come
        // before it, which is what a table without holes in it would say.
        assert_eq!(at("10.0.0.1"), None, "private");
        assert_eq!(at("127.0.0.1"), None, "loopback");
        assert_eq!(at("240.0.0.1"), None, "reserved");
        assert_eq!(at("0.0.0.1"), None, "below every allocation");
    }

    #[test]
    fn ipv6_answers_too() {
        // 2804::/16 is LACNIC's, and 2804:14c::/32 is a Brazilian ISP.
        assert_eq!(at("2804:14c::1"), Some("BR"));
        assert_eq!(at("::1"), None, "loopback is nobody's");
    }

    #[test]
    fn every_answer_is_two_letters() {
        // A malformed code would show as a flag for a country that does not
        // exist, which is the one failure that looks deliberate.
        for probe in ["8.8.8.8", "1.1.1.1", "200.160.2.3", "195.201.0.1", "2804:14c::1"] {
            if let Some(code) = at(probe) {
                assert_eq!(code.len(), 2, "{probe} gave {code}");
                assert!(code.bytes().all(|b| b.is_ascii_uppercase()), "{probe} gave {code}");
            }
        }
    }
}

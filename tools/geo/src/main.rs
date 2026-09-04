//! Turns the five RIRs' delegation files into the table Zerem carries.
//!
//! # Where the data comes from
//!
//! The regional internet registries publish, daily, exactly which address
//! ranges they have handed to which country. It is the source of record rather
//! than a guess at one, it is free to redistribute, and it carries no licence
//! that reaches into what uses it — which is why it is here and MaxMind's
//! GeoLite2 is not. GeoLite2 needs an account, forbids shipping a copy older
//! than thirty days, and would put a licence agreement between Zerem and its
//! own users over a flag.
//!
//! Fetch them first (they are about 40 MB of text in total):
//!
//! ```text
//! curl -O https://ftp.afrinic.net/pub/stats/afrinic/delegated-afrinic-extended-latest
//! curl -O https://ftp.apnic.net/stats/apnic/delegated-apnic-extended-latest
//! curl -O https://ftp.arin.net/pub/stats/arin/delegated-arin-extended-latest
//! curl -O https://ftp.lacnic.net/pub/stats/lacnic/delegated-lacnic-extended-latest
//! curl -O https://ftp.ripe.net/pub/stats/ripencc/delegated-ripencc-extended-latest
//! ```
//!
//! Then `cargo run --release -- <folder-with-those> ../../assets/geoip.bin`.
//!
//! # What it costs, and why nothing is rounded away
//!
//! 504 KiB: 308 for IPv4, 196 for IPv6.
//!
//! A floor was tried first — absorb any range shorter than a /22 into its
//! neighbour — and it brought the table to 390 KiB. The saving is entirely made
//! of mistakes: the only ranges it removes are ones whose country differs from
//! the neighbour that swallows them, because same-country neighbours are
//! already fused. It moved 0.44 % of allocated space, which sounds small until
//! you notice what small allocations are: 1.1.1.0/24 is a /24, so Cloudflare's
//! resolver came out as Thailand.
//!
//! 114 KiB is a cheap price for a table that is never wrong, and "a wrong flag
//! is worse than no flag" was the argument for having this at all.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// A run of addresses belonging to one country: `[start, end)` and the code.
///
/// IPv4 keys are the address itself. IPv6 keys are the top 32 bits — no country
/// splits a /32, and carrying 128 bits to say so would quadruple the table.
type Range = (u64, u64, [u8; 2]);

/// How many entries a lookup may have to walk before the skip index helps.
///
/// The stream is delta-encoded, so it cannot be searched directly — but an
/// index of every 512th key can, and 512 varints is a few microseconds. Larger
/// wastes time on lookups; smaller wastes bytes on the index.
const SKIP: usize = 512;

/// IPv4 keys are /24s, not addresses.
///
/// See [`rescale`]: on this data it costs nothing and saves a third of the
/// table, because a peer's flag is a question about its network and never about
/// its host. It is the one reduction here that is free — the floor that was
/// tried alongside it was not, and is gone.
const V4_SHIFT: u8 = 8;

/// IPv6 keys are the top 32 bits, and no fewer.
///
/// A /32 is the smallest block a registry gives a country, so this is exact.
/// Going one step coarser was measured: it fuses twenty thousand distinct
/// allocations into their neighbours, which is not a smaller table, it is a
/// wrong one.
const V6_SHIFT: u8 = 0;

fn main() {
    let mut args = std::env::args().skip(1);
    let (input, output) = match (args.next(), args.next()) {
        (Some(i), Some(o)) => (i, o),
        _ => {
            eprintln!("usage: zerem-geo-build <folder-of-delegated-files> <out.bin>");
            std::process::exit(2);
        }
    };

    let (v4, v6, codes) = read(Path::new(&input));
    assert!(!v4.is_empty(), "no IPv4 records in {input} — wrong folder?");

    let (v4, v6) = (merge(v4), merge(v6));
    let codes: Vec<[u8; 2]> = codes.into_iter().collect();

    let blob = encode(&v4, &v6, &codes);
    fs::write(&output, &blob).expect("write the table");

    println!(
        "{output}: {} bytes — {} countries, {} IPv4 ranges, {} IPv6 ranges",
        blob.len(),
        codes.len(),
        v4.len(),
        v6.len()
    );
}

/// Every allocation the registries have published, by family.
fn read(dir: &Path) -> (Vec<Range>, Vec<Range>, BTreeSet<[u8; 2]>) {
    let (mut v4, mut v6, mut codes) = (Vec::new(), Vec::new(), BTreeSet::new());

    for entry in fs::read_dir(dir).expect("read the input folder") {
        let path = entry.expect("read a directory entry").path();
        if !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("delegated-")) {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read a delegation file");
        for line in text.lines() {
            if let Some((range, family)) = parse(line) {
                codes.insert(range.2);
                if family == Family::V4 {
                    v4.push(range);
                } else {
                    v6.push(range);
                }
            }
        }
    }
    (v4, v6, codes)
}

#[derive(PartialEq, Eq)]
enum Family {
    V4,
    V6,
}

/// One line of a delegation file, if it is an allocation to a real country.
///
/// The format is pipe-separated: registry|cc|type|start|value|date|status|…
/// Lines that are summaries, reserved, or available are skipped, as are the
/// ones whose "country" is not two letters — the registries use those for
/// their own bookkeeping.
fn parse(line: &str) -> Option<(Range, Family)> {
    if line.starts_with('#') {
        return None;
    }
    let f: Vec<&str> = line.split('|').collect();
    let (cc, kind, start, value, status) = (*f.get(1)?, *f.get(2)?, *f.get(3)?, *f.get(4)?, *f.get(6)?);

    if status != "allocated" && status != "assigned" {
        return None;
    }
    if cc.len() != 2 || !cc.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let code = [cc.as_bytes()[0].to_ascii_uppercase(), cc.as_bytes()[1].to_ascii_uppercase()];

    match kind {
        "ipv4" => {
            // `value` is a count of addresses here, not a prefix length — the
            // registries hand out ranges that are not always a whole power of
            // two, and the format says so.
            let octets: Vec<u64> = start.split('.').filter_map(|p| p.parse().ok()).collect();
            let [a, b, c, d] = octets[..] else { return None };
            let first = (a << 24) | (b << 16) | (c << 8) | d;
            let count: u64 = value.parse().ok()?;
            (count > 0).then_some(((first, first + count, code), Family::V4))
        }
        "ipv6" => {
            // And here it is a prefix length. Only the top 32 bits are kept.
            let bits: u32 = value.parse().ok()?;
            let mut head = 0u64;
            let mut seen = 0;
            for part in start.split(':').take(2) {
                if part.is_empty() {
                    break;
                }
                head = (head << 16) | u64::from_str_radix(part, 16).ok()?;
                seen += 1;
            }
            head <<= 16 * (2 - seen);
            let span = if bits >= 32 { 1 } else { 1u64 << (32 - bits) };
            Some(((head, head + span, code), Family::V6))
        }
        _ => None,
    }
}

/// Sort, and fuse neighbours that name the same country.
fn merge(mut ranges: Vec<Range>) -> Vec<Range> {
    ranges.sort_unstable();
    let mut out: Vec<Range> = Vec::with_capacity(ranges.len());
    for (start, end, code) in ranges {
        match out.last_mut() {
            Some(last) if last.2 == code && last.1 >= start => last.1 = last.1.max(end),
            _ => out.push((start, end, code)),
        }
    }
    out
}

/// The whole table, in the shape [`zerem_core::geo`] reads.
///
/// ```text
/// "ZGEO1"                     magic
/// u16                         how many country codes follow
/// [u8; 2] * n                 the codes, sorted; 0 is the first real one
/// then, IPv4 first and IPv6 second:
///   u32                       how many entries
///   u32                       how many skip-index rows
///   [u32 key, u32 offset]     the index
///   u32                       how many bytes of stream
///   stream                    pairs of (varint key delta, u8 country + 1)
/// ```
///
/// A country byte of zero means "nobody has this range", which is how the gaps
/// between allocations are spelled out — the alternative is a second structure
/// saying where the gaps are, and this is one byte.
fn encode(v4: &[Range], v6: &[Range], codes: &[[u8; 2]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(512 * 1024);
    out.extend_from_slice(b"ZGEO1");
    out.extend_from_slice(&u16::try_from(codes.len()).expect("fewer than 65536 countries").to_le_bytes());
    for code in codes {
        out.extend_from_slice(code);
    }
    for (name, family, shift) in [("IPv4", v4, V4_SHIFT), ("IPv6", v6, V6_SHIFT)] {
        let scaled = rescale(family, shift);
        let before = out.len();
        out.push(shift);
        encode_family(&mut out, &scaled, codes);
        println!("  {name}: {} ranges, {} bytes", scaled.len(), out.len() - before);
    }
    out
}

/// Drop the low bits of every key, and keep the result strictly ordered.
///
/// A lookup only ever asks which country a /24 is in, so carrying the host bits
/// costs a byte a row for a distinction nothing reads. On this data it is free:
/// the range count does not move, because the registries hand out CIDR blocks
/// and a block of a /22 or more already starts on a /24 boundary.
///
/// Free is not the same as guaranteed, though. The format allows a range that
/// is not a whole power of two, and rounding one of those can push its end past
/// the next range's start. Where that happens the earlier range keeps the
/// shared /24 and the later one begins after it — the alternative is a key that
/// goes backwards, which the delta encoding cannot express at all.
fn rescale(ranges: &[Range], shift: u8) -> Vec<Range> {
    let round = 1u64 << shift;
    let mut out: Vec<Range> = Vec::with_capacity(ranges.len());
    let mut reached = 0u64;
    for &(start, end, code) in ranges {
        let (start, end) = ((start >> shift).max(reached), (end + round - 1) >> shift);
        if end <= start {
            continue;
        }
        reached = end;
        match out.last_mut() {
            Some(last) if last.2 == code && last.1 >= start => last.1 = end,
            _ => out.push((start, end, code)),
        }
    }
    out
}

fn encode_family(out: &mut Vec<u8>, ranges: &[Range], codes: &[[u8; 2]]) {
    // Every point where the answer changes, in order. Built before anything is
    // written because the encoding is a chain of differences: an entry cannot
    // be emitted until the one before it is settled.
    let mut events: Vec<(u64, u8)> = Vec::with_capacity(ranges.len() * 2);
    let mut reached = 0u64;
    for &(start, end, code) in ranges {
        // The hole this range opens up after the last one. A lookup landing in
        // it gets no flag, rather than whichever country happened to come
        // before — nobody has been given this space.
        if start > reached {
            events.push((reached, 0));
        }
        let at = codes.iter().position(|c| *c == code).expect("every code was collected");
        events.push((start, u8::try_from(at + 1).expect("under 255 countries")));
        reached = end;
    }
    // And everything past the last allocation.
    events.push((reached, 0));

    let mut stream: Vec<u8> = Vec::new();
    let mut index: Vec<(u64, u32)> = Vec::new();
    let mut cursor = 0u64;
    for (at, (key, country)) in events.iter().enumerate() {
        if at % SKIP == 0 {
            index.push((*key, u32::try_from(stream.len()).expect("stream under 4 GiB")));
        }
        varint(&mut stream, key - cursor);
        stream.push(*country);
        cursor = *key;
    }

    out.extend_from_slice(&u32::try_from(events.len()).expect("entries fit").to_le_bytes());
    out.extend_from_slice(&u32::try_from(index.len()).expect("index fits").to_le_bytes());
    for (key, offset) in index {
        out.extend_from_slice(&u32::try_from(key).expect("keys are 32-bit").to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend_from_slice(&u32::try_from(stream.len()).expect("stream fits").to_le_bytes());
    out.extend_from_slice(&stream);
}

/// LEB128, the same shape the reader in core decodes.
fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

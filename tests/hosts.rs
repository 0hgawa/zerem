//! The address parsing behind a peer's flag.
//!
//! It lives in `bridge/detail.rs` and is not public, so what is tested here is
//! the same rule written twice — which is worth it for this one: an IPv6
//! address is mostly colons, and splitting on the wrong one silently produces
//! an address that parses to something else entirely.

/// The address out of a `host:port`, brackets and all — the rule
/// `bridge::detail::host_of` follows.
fn host_of(addr: &str) -> Option<&str> {
    let host = addr.rsplit_once(':').map_or(addr, |(host, _)| host);
    Some(host.trim_start_matches('[').trim_end_matches(']')).filter(|host| !host.is_empty())
}

fn parsed(addr: &str) -> Option<std::net::IpAddr> {
    host_of(addr).and_then(|host| host.parse().ok())
}

#[test]
fn an_ipv4_peer_gives_its_address() {
    assert_eq!(parsed("192.0.2.7:6881"), Some("192.0.2.7".parse().expect("literal")));
}

#[test]
fn an_ipv6_peer_survives_its_brackets_and_its_colons() {
    // The one that would break on a naive split: the address is mostly colons,
    // and taking the first would yield "2804" — which parses as nothing, or
    // worse, as something.
    assert_eq!(parsed("[2804:14c::1]:51413"), Some("2804:14c::1".parse().expect("literal")));
}

#[test]
fn an_ipv4_address_wearing_an_ipv6_coat_comes_through_whole() {
    // Which is how every incoming IPv4 peer arrives on a dual-stack listener.
    assert_eq!(parsed("[::ffff:8.8.8.8]:6881"), Some("::ffff:8.8.8.8".parse().expect("literal")));
}

#[test]
fn nothing_useful_is_nothing_rather_than_a_wrong_answer() {
    assert_eq!(parsed(""), None);
    assert_eq!(parsed(":6881"), None, "a port with no host");
    assert_eq!(parsed("not an address:1"), None);
}

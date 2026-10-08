//! Client identification for limits and logs (FR-019, research R5, data-model §3.1).

use std::fmt;
use std::net::IpAddr;

use actix_web::http::header::HeaderMap;

/// The key limits are counted under: an IPv4 address or an IPv6 /64 prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientKey {
    V4([u8; 4]),
    V6([u8; 8]),
}

impl ClientKey {
    /// Key of requests without a peer address (in-process tests).
    pub const UNKNOWN: ClientKey = ClientKey::V4([0, 0, 0, 0]);

    /// Folds IPv4-mapped IPv6 to IPv4 and IPv6 to its /64 prefix.
    pub fn from_ip(_ip: IpAddr) -> Self {
        todo!()
    }
}

impl fmt::Display for ClientKey {
    /// `203.0.113.7` or `2001:db8:1:2::/64` (contracts/security-events.md).
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// Reverse proxies whose `X-Forwarded-For` is trusted (`TRUSTED_PROXIES`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies {
    nets: Vec<(IpAddr, u8)>,
}

impl TrustedProxies {
    /// Parses comma-separated IPs or CIDRs; empty means none.
    /// The error is the first entry that is not an address or CIDR range.
    pub fn parse(_value: &str) -> Result<Self, String> {
        todo!()
    }

    pub fn is_empty(&self) -> bool {
        self.nets.is_empty()
    }

    pub fn contains(&self, _ip: IpAddr) -> bool {
        todo!()
    }
}

/// The client of a request: the peer, or the rightmost untrusted `X-Forwarded-For`
/// entry when the peer is a trusted proxy (research R5).
pub fn resolve(
    _peer: Option<IpAddr>,
    _headers: &HeaderMap,
    _trusted: &TrustedProxies,
) -> ClientKey {
    todo!()
}

#[cfg(test)]
mod tests {
    use actix_web::http::header::{HeaderName, HeaderValue};

    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn key(s: &str) -> ClientKey {
        ClientKey::from_ip(ip(s))
    }

    fn xff(lines: &[&[u8]]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for line in lines {
            headers.append(
                HeaderName::from_static("x-forwarded-for"),
                HeaderValue::from_bytes(line).unwrap(),
            );
        }
        headers
    }

    fn proxies(value: &str) -> TrustedProxies {
        TrustedProxies::parse(value).unwrap()
    }

    #[test]
    fn ipv4_is_its_own_key() {
        assert_eq!(key("203.0.113.7"), ClientKey::V4([203, 0, 113, 7]));
        assert_ne!(key("203.0.113.7"), key("203.0.113.8"));
    }

    #[test]
    fn ipv4_mapped_ipv6_folds_to_ipv4() {
        assert_eq!(key("::ffff:203.0.113.7"), key("203.0.113.7"));
    }

    #[test]
    fn ipv6_is_keyed_by_its_64_prefix() {
        assert_eq!(
            key("2001:db8:1:2::1"),
            ClientKey::V6([0x20, 0x01, 0x0d, 0xb8, 0, 1, 0, 2])
        );
        assert_eq!(
            key("2001:db8:1:2::1"),
            key("2001:db8:1:2:ffff:ffff:ffff:ffff")
        );
        assert_ne!(key("2001:db8:1:2::1"), key("2001:db8:1:3::1"));
    }

    #[test]
    fn keys_display_as_in_the_event_log() {
        assert_eq!(key("203.0.113.7").to_string(), "203.0.113.7");
        assert_eq!(key("2001:db8:1:2:3:4:5:6").to_string(), "2001:db8:1:2::/64");
        assert_eq!(key("::1").to_string(), "::/64");
        assert_eq!(ClientKey::UNKNOWN.to_string(), "0.0.0.0");
    }

    #[test]
    fn no_peer_is_the_unknown_client() {
        let headers = xff(&[b"198.51.100.1"]);
        assert_eq!(
            resolve(None, &headers, &proxies("0.0.0.0/0")),
            ClientKey::UNKNOWN
        );
    }

    #[test]
    fn forwarded_for_from_an_untrusted_peer_is_ignored() {
        let headers = xff(&[b"198.51.100.1"]);
        let peer = Some(ip("203.0.113.7"));
        assert_eq!(
            resolve(peer, &headers, &TrustedProxies::default()),
            key("203.0.113.7")
        );
        assert_eq!(
            resolve(peer, &headers, &proxies("10.0.0.0/8")),
            key("203.0.113.7")
        );
    }

    #[test]
    fn trusted_peer_without_forwarded_for_is_the_client() {
        let peer = Some(ip("10.0.0.1"));
        assert_eq!(
            resolve(peer, &HeaderMap::new(), &proxies("10.0.0.0/8")),
            key("10.0.0.1")
        );
    }

    #[test]
    fn trusted_peer_uses_the_rightmost_untrusted_entry() {
        let trusted = proxies("10.0.0.0/8");
        let peer = Some(ip("10.0.0.1"));
        for (lines, client) in [
            (vec![&b"198.51.100.1"[..]], "198.51.100.1"),
            // A spoofed leftmost entry is never used.
            (vec![&b"6.6.6.6, 198.51.100.1"[..]], "198.51.100.1"),
            // Trusted hops are skipped.
            (
                vec![&b"198.51.100.1, 10.0.0.2, 10.9.9.9"[..]],
                "198.51.100.1",
            ),
            // Every field line, in order.
            (
                vec![&b"6.6.6.6"[..], &b"198.51.100.1, 10.0.0.2"[..]],
                "198.51.100.1",
            ),
            (vec![&b"198.51.100.1"[..], &b"10.0.0.2"[..]], "198.51.100.1"),
            // All hops trusted: the leftmost one.
            (vec![&b"10.0.0.3, 10.0.0.2"[..]], "10.0.0.3"),
            (vec![&b" 2001:db8:1:2::9 "[..]], "2001:db8:1:2::"),
        ] {
            assert_eq!(
                resolve(peer, &xff(&lines), &trusted),
                key(client),
                "{lines:?}"
            );
        }
    }

    #[test]
    fn malformed_entry_stops_the_walk_at_the_hop_to_its_right() {
        let trusted = proxies("10.0.0.0/8");
        let peer = Some(ip("10.0.0.1"));
        for (line, client) in [
            (&b"198.51.100.1, garbage, 10.0.0.2"[..], "10.0.0.2"),
            (&b"198.51.100.1, garbage"[..], "10.0.0.1"),
            (&b"198.51.100.1, unknown"[..], "10.0.0.1"),
            (&b"198.51.100.1, 198.51.100.2:443"[..], "10.0.0.1"),
            (&b"198.51.100.1,,10.0.0.2"[..], "10.0.0.2"),
            (&b""[..], "10.0.0.1"),
            (&b"198.51.100.1, \xff"[..], "10.0.0.1"),
        ] {
            assert_eq!(
                resolve(peer, &xff(&[line]), &trusted),
                key(client),
                "{line:?}"
            );
        }
    }

    #[test]
    fn mapped_peer_and_entries_match_ipv4_proxies() {
        let trusted = proxies("10.0.0.0/8");
        let headers = xff(&[b"::ffff:10.0.0.7, 2001:db8:1:2::9, ::ffff:10.0.0.2"]);
        assert_eq!(
            resolve(Some(ip("::ffff:10.0.0.1")), &headers, &trusted),
            key("2001:db8:1:2::")
        );
    }

    #[test]
    fn proxy_cidrs_match_by_prefix() {
        let trusted = proxies("192.168.0.0/16, 2001:db8::/32, 203.0.113.9, 10.1.2.3/8");
        for yes in [
            "192.168.255.1",
            "2001:db8:ffff::1",
            "203.0.113.9",
            "10.200.0.1",
        ] {
            assert!(trusted.contains(ip(yes)), "{yes}");
        }
        for no in [
            "192.169.0.1",
            "2001:db9::1",
            "203.0.113.10",
            "11.0.0.1",
            "::ffff:192.169.0.1",
        ] {
            assert!(!trusted.contains(ip(no)), "{no}");
        }
        assert!(trusted.contains(ip("::ffff:192.168.1.1")));
        assert!(proxies("0.0.0.0/0").contains(ip("8.8.8.8")));
        assert!(!proxies("0.0.0.0/0").contains(ip("2001:db8::1")));
        assert!(proxies("::/0").contains(ip("2001:db8::1")));
        assert!(TrustedProxies::parse("").unwrap().is_empty());
    }
}

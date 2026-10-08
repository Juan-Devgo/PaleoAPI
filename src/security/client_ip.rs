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

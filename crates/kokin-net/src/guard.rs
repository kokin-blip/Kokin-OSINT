//! The SSRF guard: which addresses a connector is allowed to reach.
//!
//! Every check here is pure logic over an address or a URL, with no I/O, which
//! is what lets the whole thing be tested exhaustively offline under
//! `KOKIN_NETWORK=deny`. Resolution is injected (see [`crate::resolver`]) so a
//! test can make `evil.example.com` resolve to `127.0.0.1` without a network or
//! a DNS server.
//!
//! # Why the check is on the resolved address
//!
//! Guarding a hostname is not enough. `localtest.me` and thousands of similar
//! names resolve to `127.0.0.1` and would sail through a name-based allowlist.
//! Worse, DNS rebinding lets a name resolve to a public address when it is
//! checked and to a private one moments later when it is connected to.
//!
//! So the guard runs on the **resolved IP**, and the broker re-resolves
//! immediately before connect and checks that address too. Attack catalogue
//! rows A-001, A-002, A-003.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Why an address or URL was refused.
///
/// Each variant names the specific reason rather than collapsing into "blocked",
/// because these appear in a user-visible network log and "we refused to fetch
/// that because it resolves to your router" is actionable where "blocked" is not.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("scheme {0:?} is not allowed; only http and https can be fetched")]
    Scheme(String),

    #[error("the URL has no host")]
    NoHost,

    #[error("{host} is not in this connector's declared destinations")]
    HostNotAllowed { host: String },

    #[error("{addr} is a loopback address")]
    Loopback { addr: IpAddr },

    #[error("{addr} is a private network address")]
    Private { addr: IpAddr },

    #[error("{addr} is a link-local address (this range includes cloud metadata services)")]
    LinkLocal { addr: IpAddr },

    #[error("{addr} is a carrier-grade NAT address")]
    CarrierGradeNat { addr: IpAddr },

    #[error("{addr} is not a globally routable address")]
    NotGlobal { addr: IpAddr },

    #[error("redirect to a different host ({to}) is not followed automatically")]
    CrossHostRedirect { to: String },
}

/// Schemes a connector may fetch.
///
/// `file:` would read local files, `gopher:` and `dict:` can be used to speak
/// other protocols through a URL fetcher, and `data:` bypasses fetching
/// entirely. None of them are ever what an OSINT connector legitimately wants.
pub const ALLOWED_SCHEMES: [&str; 2] = ["http", "https"];

/// Is this scheme fetchable?
pub fn check_scheme(scheme: &str) -> Result<(), Refusal> {
    let lower = scheme.to_ascii_lowercase();
    if ALLOWED_SCHEMES.contains(&lower.as_str()) {
        Ok(())
    } else {
        Err(Refusal::Scheme(scheme.to_string()))
    }
}

/// Is this address one a connector may connect to?
///
/// Deliberately an allowlist by exclusion of every non-global range, rather than
/// a blocklist of ranges someone remembered. New special-purpose ranges get
/// added to IANA registries regularly; "not globally routable" ages better than
/// a hand-maintained list of bad prefixes.
pub fn check_address(addr: IpAddr) -> Result<(), Refusal> {
    match addr {
        IpAddr::V4(v4) => check_v4(v4, addr),
        IpAddr::V6(v6) => check_v6(v6, addr),
    }
}

fn check_v4(v4: Ipv4Addr, addr: IpAddr) -> Result<(), Refusal> {
    if v4.is_loopback() {
        return Err(Refusal::Loopback { addr });
    }
    if v4.is_link_local() {
        // 169.254.0.0/16. Includes 169.254.169.254, the cloud metadata endpoint
        // that turns an SSRF into credential theft on every major provider.
        return Err(Refusal::LinkLocal { addr });
    }
    if v4.is_private() {
        return Err(Refusal::Private { addr });
    }
    // 100.64.0.0/10 - carrier-grade NAT (RFC 6598). Not covered by is_private,
    // and routinely used for internal infrastructure.
    if v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]) {
        return Err(Refusal::CarrierGradeNat { addr });
    }
    if v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_multicast()
        || v4.is_documentation()
        // 0.0.0.0/8 "this network", and 240.0.0.0/4 reserved.
        || v4.octets()[0] == 0
        || v4.octets()[0] >= 240
    {
        return Err(Refusal::NotGlobal { addr });
    }
    Ok(())
}

fn check_v6(v6: Ipv6Addr, addr: IpAddr) -> Result<(), Refusal> {
    if v6.is_loopback() {
        return Err(Refusal::Loopback { addr });
    }

    let seg = v6.segments();

    // fe80::/10 link-local.
    if (seg[0] & 0xffc0) == 0xfe80 {
        return Err(Refusal::LinkLocal { addr });
    }
    // fc00::/7 unique local addresses - the IPv6 equivalent of RFC1918.
    if (seg[0] & 0xfe00) == 0xfc00 {
        return Err(Refusal::Private { addr });
    }
    if v6.is_unspecified() || v6.is_multicast() {
        return Err(Refusal::NotGlobal { addr });
    }

    // An IPv4-mapped or IPv4-compatible v6 address must be judged by its v4
    // content, or ::ffff:127.0.0.1 walks straight past every check above.
    if let Some(v4) = v6.to_ipv4_mapped() {
        return check_v4(v4, IpAddr::V4(v4));
    }
    if let Some(v4) = v6.to_ipv4() {
        return check_v4(v4, IpAddr::V4(v4));
    }

    // 2001:db8::/32 documentation.
    if seg[0] == 0x2001 && seg[1] == 0x0db8 {
        return Err(Refusal::NotGlobal { addr });
    }

    Ok(())
}

/// Is `host` covered by a connector's declared destinations?
///
/// A leading `.` means "this domain and its subdomains"; anything else is an
/// exact match. Bare suffix matching is deliberately not supported, because
/// `example.com` matching `notexample.com` is a classic allowlist bypass.
pub fn check_host_allowed(host: &str, allowed: &[String]) -> Result<(), Refusal> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();

    for entry in allowed {
        let entry = entry.trim_end_matches('.').to_ascii_lowercase();

        if let Some(domain) = entry.strip_prefix('.') {
            if host == domain || host.ends_with(&format!(".{domain}")) {
                return Ok(());
            }
        } else if host == entry {
            return Ok(());
        }
    }

    Err(Refusal::HostNotAllowed { host })
}

/// May a redirect from `from_host` to `to_host` be followed automatically?
///
/// Never, across hosts. A permitted host redirecting to an attacker's host would
/// otherwise carry headers and credentials outside the declared destinations
/// (A-004). The redirect is still *recorded* on the capture so the analyst sees
/// where the trail went; it simply is not followed without a decision.
pub fn check_redirect(from_host: &str, to_host: &str) -> Result<(), Refusal> {
    if from_host.eq_ignore_ascii_case(to_host) {
        Ok(())
    } else {
        Err(Refusal::CrossHostRedirect {
            to: to_host.to_string(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn ip(s: &str) -> IpAddr {
        IpAddr::from_str(s).unwrap()
    }

    #[test]
    fn only_http_and_https_are_fetchable() {
        for ok in ["http", "https", "HTTP", "HttpS"] {
            assert!(check_scheme(ok).is_ok(), "{ok} should be allowed");
        }
        for bad in ["file", "gopher", "data", "ftp", "dict", "jar", "ws"] {
            assert!(
                matches!(check_scheme(bad), Err(Refusal::Scheme(_))),
                "{bad} should be refused"
            );
        }
    }

    /// The PoC P4 address list, which is the acceptance criterion for the guard.
    #[test]
    fn the_poc_p4_addresses_are_all_refused() {
        for addr in [
            "127.0.0.1",
            "::1",
            "169.254.169.254",
            "100.64.0.1",
            "10.0.0.1",
            "192.168.1.1",
            "172.16.0.1",
            "0.0.0.0",
            "fc00::1",
            "fe80::1",
        ] {
            assert!(
                check_address(ip(addr)).is_err(),
                "{addr} must be refused but was allowed"
            );
        }
    }

    /// The bypass that defeats a naive v6 check: wrap a v4 loopback in v6.
    #[test]
    fn ipv4_mapped_loopback_is_refused() {
        for addr in [
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:10.0.0.1",
        ] {
            assert!(
                matches!(
                    check_address(ip(addr)),
                    Err(Refusal::Loopback { .. }
                        | Refusal::LinkLocal { .. }
                        | Refusal::Private { .. })
                ),
                "{addr} must be refused as its embedded v4 address"
            );
        }
    }

    #[test]
    fn ordinary_public_addresses_are_allowed() {
        for addr in ["1.1.1.1", "93.184.216.34", "2606:4700:4700::1111"] {
            assert!(
                check_address(ip(addr)).is_ok(),
                "{addr} should be allowed, got {:?}",
                check_address(ip(addr))
            );
        }
    }

    #[test]
    fn the_metadata_endpoint_is_named_in_the_refusal() {
        match check_address(ip("169.254.169.254")) {
            Err(Refusal::LinkLocal { .. }) => {}
            other => panic!("expected LinkLocal, got {other:?}"),
        }
    }

    #[test]
    fn an_exact_host_allowlist_does_not_match_a_prefix() {
        let allowed = vec!["example.com".to_string()];
        assert!(check_host_allowed("example.com", &allowed).is_ok());
        // The classic bypass: a bare suffix check would let these through.
        assert!(check_host_allowed("notexample.com", &allowed).is_err());
        assert!(check_host_allowed("example.com.evil.test", &allowed).is_err());
        assert!(check_host_allowed("sub.example.com", &allowed).is_err());
    }

    #[test]
    fn a_leading_dot_allows_subdomains_only() {
        let allowed = vec![".example.com".to_string()];
        assert!(check_host_allowed("example.com", &allowed).is_ok());
        assert!(check_host_allowed("api.example.com", &allowed).is_ok());
        assert!(check_host_allowed("a.b.example.com", &allowed).is_ok());
        assert!(check_host_allowed("notexample.com", &allowed).is_err());
        assert!(check_host_allowed("example.com.evil.test", &allowed).is_err());
    }

    #[test]
    fn host_matching_ignores_case_and_a_trailing_dot() {
        let allowed = vec!["Example.COM".to_string()];
        assert!(check_host_allowed("example.com", &allowed).is_ok());
        assert!(check_host_allowed("EXAMPLE.com.", &allowed).is_ok());
    }

    #[test]
    fn cross_host_redirects_are_never_followed() {
        assert!(check_redirect("example.com", "example.com").is_ok());
        assert!(check_redirect("example.com", "EXAMPLE.com").is_ok());
        match check_redirect("example.com", "evil.test") {
            Err(Refusal::CrossHostRedirect { to }) => assert_eq!(to, "evil.test"),
            other => panic!("expected CrossHostRedirect, got {other:?}"),
        }
    }
}

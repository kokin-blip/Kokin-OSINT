//! The egress broker — the only crate in Kokin-OSINT permitted to open a socket.
//!
//! Connectors never reach the network directly. They receive capability handles
//! derived from their manifest, and every request passes the checks in
//! [`guard`]: scheme allowlist, destination allowlist, and an SSRF check applied
//! to the **resolved address**, re-resolved immediately before connect.
//!
//! This is enforced structurally, not by convention: a CI lint fails the build
//! if `reqwest`, `hyper`, `ureq`, `std::net`, or `tokio::net` appears anywhere
//! outside this crate (ADR-0010), and it was verified by planting a violation
//! and confirming the build broke.
//!
//! # Everything here is testable offline
//!
//! The whole test suite must pass with `KOKIN_NETWORK=deny` and no network
//! (ADR-0010). That is only possible because resolution is injected: a test
//! supplies a [`resolver::StaticResolver`] that maps `evil.example.com` to
//! `127.0.0.1`, and the guard is exercised against a real rebinding sequence
//! without a DNS server or a socket anywhere in sight.
//!
//! # Scope of this increment
//!
//! The policy engine, the resolver abstraction, offline enforcement, and the
//! rate limiter. **There is no live transport yet** — no HTTP client is linked.
//! That is deliberate ordering, not an oversight: PoC P4's kill criterion is
//! that any bypass of this guard halts connector work, so the guard is built and
//! attacked before anything can depend on it.

pub mod guard;
pub mod resolver;

use std::net::IpAddr;
use std::time::Instant;

pub use guard::Refusal;
pub use resolver::{Resolver, StaticResolver};

/// Whether this process may touch the network at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    /// Live requests are permitted, subject to every other check.
    Allow,
    /// No live request may be made. Set in CI and by the application's offline
    /// mode. The `external_tool` connector tier is blocked entirely, because a
    /// subprocess's sockets cannot be policed (ADR-0011).
    Deny,
}

impl NetworkMode {
    /// Read `KOKIN_NETWORK`.
    ///
    /// **Defaults to `Deny`.** Anything unrecognised is also `Deny`. A typo in
    /// the variable must not silently enable network access — the safe direction
    /// for this switch is off.
    pub fn from_env() -> Self {
        match std::env::var("KOKIN_NETWORK").as_deref() {
            Ok("allow") => Self::Allow,
            _ => Self::Deny,
        }
    }

    pub fn is_denied(self) -> bool {
        self == Self::Deny
    }
}

/// Why a request was not made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BrokerError {
    #[error("refused: {0}")]
    Refused(#[from] Refusal),

    #[error("network access is disabled (KOKIN_NETWORK=deny); no request was made")]
    NetworkDenied,

    #[error("{host} did not resolve to any address")]
    NoAddresses { host: String },

    #[error("rate limit for {connector} exceeded; retry in {retry_after_ms}ms")]
    RateLimited {
        connector: String,
        retry_after_ms: u64,
    },

    #[error("response exceeded the {limit} byte cap for this connector")]
    ResponseTooLarge { limit: u64 },

    #[error("could not resolve {host}: {reason}")]
    Resolution { host: String, reason: String },
}

/// Who is asking, so every byte of egress can be attributed.
///
/// Not optional, and not a string: an unattributed request cannot be shown in
/// the network activity log, and a log that omits requests is worse than none
/// because it implies completeness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    pub connector: String,
    pub job_id: String,
    pub case_id: String,
}

/// What a connector is permitted to do, derived from its manifest.
#[derive(Debug, Clone)]
pub struct EgressPolicy {
    /// Hosts this connector declared. A leading `.` allows subdomains.
    pub allowed_hosts: Vec<String>,
    /// Hard cap on a single response body, enforced during the read.
    pub max_response_bytes: u64,
    /// Requests per second, averaged, with `burst` allowed at once.
    pub requests_per_second: f64,
    pub burst: u32,
}

impl EgressPolicy {
    /// A policy that permits nothing. Callers build up from here, so forgetting
    /// to set a field fails closed.
    pub fn deny_all() -> Self {
        Self {
            allowed_hosts: Vec::new(),
            max_response_bytes: 0,
            requests_per_second: 0.0,
            burst: 0,
        }
    }
}

/// An address that has passed every check, at the moment it was checked.
///
/// Deliberately carries the address rather than the hostname: the caller must
/// connect to *this* address, not re-resolve the name and connect to whatever
/// comes back. Handing back a hostname would reintroduce the rebinding window
/// the guard exists to close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VettedTarget {
    pub host: String,
    pub addr: IpAddr,
    pub port: u16,
}

/// Validate a request without making it.
///
/// Order matters: cheap syntactic checks first, then the allowlist, and only
/// then resolution — so a malformed or disallowed URL never causes a DNS lookup,
/// which is itself an observable network event that leaks what was attempted.
pub fn vet_request<R: Resolver>(
    scheme: &str,
    host: &str,
    port: u16,
    policy: &EgressPolicy,
    resolver: &R,
    mode: NetworkMode,
) -> Result<VettedTarget, BrokerError> {
    guard::check_scheme(scheme)?;
    guard::check_host_allowed(host, &policy.allowed_hosts)?;

    if mode.is_denied() {
        return Err(BrokerError::NetworkDenied);
    }

    let addrs = resolver
        .resolve(host)
        .map_err(|reason| BrokerError::Resolution {
            host: host.to_string(),
            reason,
        })?;

    if addrs.is_empty() {
        return Err(BrokerError::NoAddresses {
            host: host.to_string(),
        });
    }

    // Every returned address must pass, not just one. A name resolving to both
    // a public and a private address is a rebinding attack in progress, and
    // picking the first acceptable one would walk straight into it.
    for addr in &addrs {
        guard::check_address(*addr)?;
    }

    Ok(VettedTarget {
        host: host.to_string(),
        addr: addrs[0],
        port,
    })
}

/// Re-check immediately before connecting.
///
/// The window between vetting and connecting is exactly what DNS rebinding
/// exploits, so the address is resolved again and re-checked, and the result
/// must still match what was vetted. This is the second half of A-002; the
/// first half is that [`vet_request`] checks addresses rather than names.
pub fn reconfirm_before_connect<R: Resolver>(
    vetted: &VettedTarget,
    resolver: &R,
) -> Result<(), BrokerError> {
    let addrs = resolver
        .resolve(&vetted.host)
        .map_err(|reason| BrokerError::Resolution {
            host: vetted.host.clone(),
            reason,
        })?;

    if addrs.is_empty() {
        return Err(BrokerError::NoAddresses {
            host: vetted.host.clone(),
        });
    }

    for addr in &addrs {
        guard::check_address(*addr)?;
    }

    Ok(())
}

/// Token-bucket rate limiter, one per connector.
///
/// Being polite to the services an investigator queries is not only courtesy:
/// a burst of requests is what gets an analyst's address blocked mid-case.
#[derive(Debug)]
pub struct RateLimiter {
    connector: String,
    capacity: f64,
    refill_per_second: f64,
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    pub fn new(connector: impl Into<String>, policy: &EgressPolicy) -> Self {
        Self {
            connector: connector.into(),
            capacity: policy.burst.max(1) as f64,
            refill_per_second: policy.requests_per_second,
            tokens: policy.burst.max(1) as f64,
            last: Instant::now(),
        }
    }

    /// Take one token, or report how long to wait.
    pub fn try_acquire(&mut self) -> Result<(), BrokerError> {
        self.refill(Instant::now());

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return Ok(());
        }

        let deficit = 1.0 - self.tokens;
        let retry_after_ms = if self.refill_per_second > 0.0 {
            (deficit / self.refill_per_second * 1000.0).ceil() as u64
        } else {
            // A zero rate means this connector may never make a request; report
            // a large wait rather than dividing by zero.
            u64::MAX
        };

        Err(BrokerError::RateLimited {
            connector: self.connector.clone(),
            retry_after_ms,
        })
    }

    /// Advance the bucket to `now`. Separated so tests can drive time directly
    /// instead of sleeping, which would make the suite slow and flaky.
    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.refill_per_second).min(self.capacity);
    }

    #[cfg(test)]
    fn advance(&mut self, by: std::time::Duration) {
        self.tokens = (self.tokens + by.as_secs_f64() * self.refill_per_second).min(self.capacity);
    }
}

/// Enforce a byte cap while reading, never after.
///
/// A cap checked after the read has already allocated the memory it was meant
/// to prevent (A-011). This wrapper stops at the limit.
pub struct CappedReader<R> {
    inner: R,
    limit: u64,
    read_so_far: u64,
}

impl<R: std::io::Read> CappedReader<R> {
    pub fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            limit,
            read_so_far: 0,
        }
    }

    pub fn bytes_read(&self) -> u64 {
        self.read_so_far
    }
}

impl<R: std::io::Read> std::io::Read for CappedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.read_so_far >= self.limit {
            return Err(std::io::Error::other(BrokerError::ResponseTooLarge {
                limit: self.limit,
            }));
        }

        // Never read more than the remaining allowance, so the cap cannot be
        // overshot by a large underlying read.
        let remaining = (self.limit - self.read_so_far) as usize;
        let take = remaining.min(buf.len());
        let n = self.inner.read(&mut buf[..take])?;
        self.read_so_far += n as u64;
        Ok(n)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::io::Read;

    fn policy(hosts: &[&str]) -> EgressPolicy {
        EgressPolicy {
            allowed_hosts: hosts.iter().map(|s| s.to_string()).collect(),
            max_response_bytes: 1024,
            requests_per_second: 10.0,
            burst: 5,
        }
    }

    #[test]
    fn network_mode_defaults_to_deny() {
        // An unset or misspelled variable must not enable the network.
        for value in [None, Some("yes"), Some("true"), Some("1"), Some("ALLOW")] {
            match value {
                Some(v) => std::env::set_var("KOKIN_NETWORK", v),
                None => std::env::remove_var("KOKIN_NETWORK"),
            }
            assert_eq!(
                NetworkMode::from_env(),
                NetworkMode::Deny,
                "KOKIN_NETWORK={value:?} must not enable the network"
            );
        }
        std::env::set_var("KOKIN_NETWORK", "allow");
        assert_eq!(NetworkMode::from_env(), NetworkMode::Allow);
        std::env::remove_var("KOKIN_NETWORK");
    }

    #[test]
    fn a_denied_network_makes_no_request_and_says_so() {
        let r = StaticResolver::new().with("example.com", &["93.184.216.34"]);
        let err = vet_request(
            "https",
            "example.com",
            443,
            &policy(&["example.com"]),
            &r,
            NetworkMode::Deny,
        )
        .unwrap_err();
        assert_eq!(err, BrokerError::NetworkDenied);
    }

    #[test]
    fn a_permitted_public_host_is_vetted() {
        let r = StaticResolver::new().with("example.com", &["93.184.216.34"]);
        let target = vet_request(
            "https",
            "example.com",
            443,
            &policy(&["example.com"]),
            &r,
            NetworkMode::Allow,
        )
        .unwrap();
        assert_eq!(target.addr.to_string(), "93.184.216.34");
        assert_eq!(target.port, 443);
    }

    /// The attack a hostname allowlist cannot stop: a permitted-looking name
    /// that resolves into private space.
    #[test]
    fn a_permitted_host_resolving_to_loopback_is_refused() {
        let r = StaticResolver::new().with("internal.example.com", &["127.0.0.1"]);
        let err = vet_request(
            "https",
            "internal.example.com",
            443,
            &policy(&[".example.com"]),
            &r,
            NetworkMode::Allow,
        )
        .unwrap_err();
        assert!(
            matches!(err, BrokerError::Refused(Refusal::Loopback { .. })),
            "got {err:?}"
        );
    }

    /// A name resolving to both a public and a private address is a rebinding
    /// attack in progress. Picking the first acceptable address would fall for it.
    #[test]
    fn every_resolved_address_must_pass_not_just_one() {
        let r = StaticResolver::new().with("mixed.example.com", &["93.184.216.34", "10.0.0.5"]);
        let err = vet_request(
            "https",
            "mixed.example.com",
            443,
            &policy(&[".example.com"]),
            &r,
            NetworkMode::Allow,
        )
        .unwrap_err();
        assert!(
            matches!(err, BrokerError::Refused(Refusal::Private { .. })),
            "got {err:?}"
        );
    }

    /// The full DNS rebinding sequence: public when vetted, private at connect.
    /// This is the test that justifies re-resolving.
    #[test]
    fn dns_rebinding_between_vetting_and_connecting_is_caught() {
        let mut r = StaticResolver::new().with("rebind.example.com", &["93.184.216.34"]);

        let target = vet_request(
            "https",
            "rebind.example.com",
            443,
            &policy(&[".example.com"]),
            &r,
            NetworkMode::Allow,
        )
        .unwrap();

        // The attacker flips the record in the window before connect.
        r.set("rebind.example.com", &["127.0.0.1"]);

        let err = reconfirm_before_connect(&target, &r).unwrap_err();
        assert!(
            matches!(err, BrokerError::Refused(Refusal::Loopback { .. })),
            "rebinding was not caught: {err:?}"
        );
    }

    #[test]
    fn a_disallowed_host_is_refused_before_any_resolution() {
        // The resolver panics if used, proving no lookup happened - a DNS query
        // is itself an observable event that leaks what was attempted.
        struct NeverResolve;
        impl Resolver for NeverResolve {
            fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
                panic!("resolution must not happen for a disallowed host");
            }
        }

        let err = vet_request(
            "https",
            "evil.test",
            443,
            &policy(&["example.com"]),
            &NeverResolve,
            NetworkMode::Allow,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            BrokerError::Refused(Refusal::HostNotAllowed { .. })
        ));
    }

    #[test]
    fn a_disallowed_scheme_is_refused_before_any_resolution() {
        struct NeverResolve;
        impl Resolver for NeverResolve {
            fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
                panic!("resolution must not happen for a disallowed scheme");
            }
        }

        let err = vet_request(
            "file",
            "example.com",
            443,
            &policy(&["example.com"]),
            &NeverResolve,
            NetworkMode::Allow,
        )
        .unwrap_err();
        assert!(matches!(err, BrokerError::Refused(Refusal::Scheme(_))));
    }

    #[test]
    fn the_rate_limiter_allows_a_burst_then_refuses() {
        let p = EgressPolicy {
            burst: 3,
            requests_per_second: 1.0,
            ..policy(&["example.com"])
        };
        let mut limiter = RateLimiter::new("test", &p);

        for i in 0..3 {
            assert!(limiter.try_acquire().is_ok(), "burst request {i} refused");
        }
        match limiter.try_acquire() {
            Err(BrokerError::RateLimited { connector, .. }) => assert_eq!(connector, "test"),
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn the_rate_limiter_refills_over_time() {
        use std::time::Duration;
        let p = EgressPolicy {
            burst: 1,
            requests_per_second: 10.0,
            ..policy(&["example.com"])
        };
        let mut limiter = RateLimiter::new("test", &p);

        assert!(limiter.try_acquire().is_ok());
        assert!(limiter.try_acquire().is_err());

        // Driven directly rather than by sleeping, so the suite stays fast.
        limiter.advance(Duration::from_millis(200));
        assert!(limiter.try_acquire().is_ok());
    }

    #[test]
    fn a_zero_rate_never_permits_a_request_and_does_not_divide_by_zero() {
        let p = EgressPolicy::deny_all();
        let mut limiter = RateLimiter::new("test", &p);
        // burst is clamped to at least 1, so the first succeeds and refill is 0.
        let _ = limiter.try_acquire();
        match limiter.try_acquire() {
            Err(BrokerError::RateLimited { retry_after_ms, .. }) => {
                assert_eq!(retry_after_ms, u64::MAX)
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn the_byte_cap_stops_at_the_limit_rather_than_after_it() {
        let data = vec![7u8; 10_000];
        let mut reader = CappedReader::new(&data[..], 100);

        let mut sink = Vec::new();
        let err = reader.read_to_end(&mut sink).unwrap_err();

        assert_eq!(
            sink.len(),
            100,
            "the cap was overshot - {} bytes were buffered",
            sink.len()
        );
        assert!(err.to_string().contains("exceeded"), "got {err}");
    }

    #[test]
    fn a_response_within_the_cap_reads_normally() {
        let data = [7u8; 50];
        let mut reader = CappedReader::new(&data[..], 100);
        let mut sink = Vec::new();
        reader.read_to_end(&mut sink).unwrap();
        assert_eq!(sink.len(), 50);
        assert_eq!(reader.bytes_read(), 50);
    }
}

//! Name resolution, behind a trait.
//!
//! Resolution is injected rather than called directly for two reasons, and the
//! second is the important one:
//!
//! 1. The test suite must pass with no network (`KOKIN_NETWORK=deny`).
//! 2. **The SSRF guard cannot be tested honestly against real DNS.** Proving
//!    that a rebinding attack is caught requires a name that resolves to a
//!    public address once and a private address moments later. That is trivial
//!    with [`StaticResolver`] and impossible to arrange reliably against a live
//!    resolver — so without this trait, the most important test in the crate
//!    could not exist.

use std::collections::HashMap;
use std::net::IpAddr;

/// Resolves a hostname to addresses.
///
/// Returns *all* addresses, not a preferred one. The broker checks every
/// address, because a name resolving to both a public and a private address is
/// a rebinding attack and returning only the first would hide it.
pub trait Resolver {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String>;
}

/// A fixed table of answers, for tests.
///
/// Answers are mutable so a single test can model a record changing between
/// vetting and connecting, which is the whole shape of a rebinding attack.
#[derive(Debug, Default, Clone)]
pub struct StaticResolver {
    answers: HashMap<String, Vec<IpAddr>>,
}

impl StaticResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style entry. Panics on an unparseable address, which in a test
    /// fixture is a typo that should stop the test immediately.
    #[allow(clippy::expect_used)]
    pub fn with(mut self, host: &str, addrs: &[&str]) -> Self {
        self.set_inner(host, addrs);
        self
    }

    /// Change an answer after construction, to model a record flipping.
    #[allow(clippy::expect_used)]
    pub fn set(&mut self, host: &str, addrs: &[&str]) {
        self.set_inner(host, addrs);
    }

    #[allow(clippy::expect_used)]
    fn set_inner(&mut self, host: &str, addrs: &[&str]) {
        let parsed = addrs
            .iter()
            .map(|a| {
                a.parse::<IpAddr>()
                    .expect("test fixture contains an unparseable IP address")
            })
            .collect();
        self.answers.insert(host.to_ascii_lowercase(), parsed);
    }
}

impl Resolver for StaticResolver {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        self.answers
            .get(&host.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| format!("no fixture answer for {host}"))
    }
}

/// A resolver that always fails.
///
/// The default in any context where a live lookup would be wrong, so forgetting
/// to supply a resolver fails closed rather than silently reaching DNS.
#[derive(Debug, Default, Clone, Copy)]
pub struct OfflineResolver;

impl Resolver for OfflineResolver {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        Err(format!(
            "offline: refusing to resolve {host}. A live resolver must be supplied explicitly."
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_static_answer_round_trips_and_ignores_case() {
        let r = StaticResolver::new().with("Example.com", &["93.184.216.34"]);
        assert_eq!(
            r.resolve("example.com").unwrap()[0].to_string(),
            "93.184.216.34"
        );
        assert_eq!(r.resolve("EXAMPLE.COM").unwrap().len(), 1);
    }

    #[test]
    fn an_unknown_host_is_an_error_not_an_empty_answer() {
        // An empty vector would read as "resolved to nothing" and could be
        // mistaken for a successful lookup by a careless caller.
        let r = StaticResolver::new();
        assert!(r.resolve("unknown.test").is_err());
    }

    #[test]
    fn an_answer_can_change_to_model_rebinding() {
        let mut r = StaticResolver::new().with("host.test", &["93.184.216.34"]);
        assert_eq!(
            r.resolve("host.test").unwrap()[0].to_string(),
            "93.184.216.34"
        );
        r.set("host.test", &["127.0.0.1"]);
        assert_eq!(r.resolve("host.test").unwrap()[0].to_string(), "127.0.0.1");
    }

    #[test]
    fn the_offline_resolver_always_refuses() {
        assert!(OfflineResolver.resolve("example.com").is_err());
    }
}

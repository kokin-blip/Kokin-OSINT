//! URL normalisation: one canonical form, one verbatim form, never just one.
//!
//! Two different questions get asked of a URL, and answering both with the same
//! string is where evidence tools go wrong:
//!
//! - **What did we actually fetch?** Verbatim, including every tracking
//!   parameter. This is provenance; altering it would be falsifying the record.
//! - **Is this the same page as that one?** Canonical, with campaign junk
//!   removed, so `?utm_source=twitter` and `?utm_source=email` do not fragment
//!   one page into two entities.
//!
//! So [`normalise`] returns both. `raw_locator` is kept byte-for-byte on the
//! capture; `canonical_locator` is what identity and deduplication use
//! (attack catalogue A-013).
//!
//! **The same function produces fixture replay keys** (see [`crate::http`]).
//! That is deliberate: if replay used a different normaliser, a fixture could
//! match a request that the evidence model considers a different page, and the
//! offline suite would quietly stop testing what it claims to.

use url::Url;

/// Query parameters removed from the canonical form.
///
/// All are campaign, referral, or click-tracking parameters that identify *how
/// someone arrived*, never *what they arrived at*. Removing them is what stops
/// one page becoming five entities; preserving them in `raw_locator` is what
/// stops us falsifying what was fetched.
///
/// Prefix families (`utm_`, `pk_`, `mtm_`, `_hs`) are handled by prefix, since
/// they are open-ended.
const TRACKING_PARAMS: &[&str] = &[
    "gclid",
    "dclid",
    "gbraid",
    "wbraid",
    "fbclid",
    "msclkid",
    "twclid",
    "igshid",
    "mc_cid",
    "mc_eid",
    "yclid",
    "ttclid",
    "li_fat_id",
    "vero_id",
    "s_cid",
    "ref_src",
    "ref_url",
    "spm",
    "scm",
];

const TRACKING_PREFIXES: &[&str] = &["utm_", "pk_", "mtm_", "piwik_", "_hs", "hsa_"];

/// Both forms of a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalisedUrl {
    /// Exactly what was requested. Provenance; never edited.
    pub raw_locator: String,
    /// Identity. Tracking parameters stripped, host lowercased, default port
    /// and fragment removed, query parameters sorted.
    pub canonical_locator: String,
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UrlError {
    #[error("not a valid URL: {0}")]
    Invalid(String),
    #[error("URL has no host")]
    NoHost,
    #[error("URL scheme {0:?} has no default port and none was given")]
    NoPort(String),
}

fn is_tracking(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    TRACKING_PARAMS.contains(&lower.as_str())
        || TRACKING_PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// Produce the raw and canonical forms of a URL.
pub fn normalise(input: &str) -> Result<NormalisedUrl, UrlError> {
    let parsed = Url::parse(input).map_err(|e| UrlError::Invalid(e.to_string()))?;

    let host = parsed
        .host_str()
        .ok_or(UrlError::NoHost)?
        .to_ascii_lowercase();
    let scheme = parsed.scheme().to_ascii_lowercase();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| UrlError::NoPort(scheme.clone()))?;

    let mut canonical = parsed.clone();

    // A fragment is never sent to the server, so two URLs differing only by
    // fragment fetched the identical resource.
    canonical.set_fragment(None);

    // Sorted so parameter order cannot fragment identity; the server receives
    // the raw order, and this form is only ever used for identity.
    let mut kept: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| !is_tracking(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    kept.sort();

    if kept.is_empty() {
        canonical.set_query(None);
    } else {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (k, v) in &kept {
            serializer.append_pair(k, v);
        }
        canonical.set_query(Some(&serializer.finish()));
    }

    // Setting the host again normalises case.
    //
    // The port needs no handling: the WHATWG URL parser already drops a port
    // that equals the scheme default, so `https://x:443/` parses as
    // `https://x/`. An earlier version stripped it manually by comparing
    // `port()` with `port_or_known_default()` — which returns the *explicit*
    // port when there is one, making the comparison always true and silently
    // erasing `:8443`. The test for a non-default port caught it.
    let _ = canonical.set_host(Some(&host));

    // An empty path is the root path; without this "http://x" and "http://x/"
    // would be two identities for one page.
    if canonical.path().is_empty() {
        canonical.set_path("/");
    }

    Ok(NormalisedUrl {
        raw_locator: input.to_string(),
        canonical_locator: canonical.to_string(),
        scheme,
        host,
        port,
    })
}

// form_urlencoded ships with the url crate.
use url::form_urlencoded;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn canon(s: &str) -> String {
        normalise(s).unwrap().canonical_locator
    }

    /// The core requirement: identity ignores tracking, provenance does not.
    #[test]
    fn tracking_parameters_are_stripped_from_identity_but_kept_verbatim() {
        let input = "https://example.com/article?id=7&utm_source=twitter&fbclid=abc123";
        let n = normalise(input).unwrap();

        assert_eq!(
            n.raw_locator, input,
            "the raw locator must be byte-for-byte what was fetched"
        );
        assert_eq!(n.canonical_locator, "https://example.com/article?id=7");
    }

    #[test]
    fn the_same_page_reached_two_ways_has_one_identity() {
        let a = canon("https://example.com/p?utm_source=twitter&id=1");
        let b = canon("https://example.com/p?id=1&utm_medium=email&gclid=xyz");
        assert_eq!(a, b, "one page fragmented into two identities");
    }

    #[test]
    fn utm_style_prefixes_are_handled_as_families() {
        for param in [
            "utm_source=a",
            "utm_content=b",
            "utm_anything_new=c",
            "pk_campaign=d",
            "mtm_source=e",
            "hsa_acc=f",
        ] {
            assert_eq!(
                canon(&format!("https://example.com/p?{param}")),
                "https://example.com/p",
                "{param} should have been stripped"
            );
        }
    }

    #[test]
    fn a_meaningful_parameter_is_never_stripped() {
        // The failure mode in the other direction: over-eager stripping that
        // silently changes which page the evidence points at.
        assert_eq!(
            canon("https://example.com/search?q=utm_source"),
            "https://example.com/search?q=utm_source"
        );
        assert_eq!(
            canon("https://example.com/p?id=7&page=2"),
            "https://example.com/p?id=7&page=2"
        );
    }

    #[test]
    fn parameter_order_does_not_change_identity() {
        assert_eq!(
            canon("https://example.com/p?b=2&a=1"),
            canon("https://example.com/p?a=1&b=2")
        );
    }

    #[test]
    fn host_case_and_default_ports_are_normalised() {
        assert_eq!(canon("https://EXAMPLE.com/p"), "https://example.com/p");
        assert_eq!(canon("https://example.com:443/p"), "https://example.com/p");
        assert_eq!(canon("http://example.com:80/p"), "http://example.com/p");
        // A non-default port is significant and must survive.
        assert_eq!(
            canon("https://example.com:8443/p"),
            "https://example.com:8443/p"
        );
    }

    #[test]
    fn fragments_are_dropped_because_they_never_reach_the_server() {
        assert_eq!(
            canon("https://example.com/p#section-2"),
            "https://example.com/p"
        );
    }

    #[test]
    fn an_empty_path_becomes_the_root_path() {
        assert_eq!(canon("https://example.com"), "https://example.com/");
        assert_eq!(canon("https://example.com/"), "https://example.com/");
    }

    #[test]
    fn scheme_host_and_port_are_reported_for_the_guard() {
        let n = normalise("https://Example.com/a/b?x=1").unwrap();
        assert_eq!(n.scheme, "https");
        assert_eq!(n.host, "example.com");
        assert_eq!(n.port, 443);
    }

    #[test]
    fn a_malformed_url_is_an_error_not_a_guess() {
        assert!(normalise("not a url").is_err());
        assert!(normalise("https://").is_err());
    }

    /// Schemes without a host must not silently produce a usable result that
    /// the guard would then have to catch.
    #[test]
    fn a_url_without_a_host_is_rejected_here() {
        assert!(matches!(
            normalise("file:///etc/passwd"),
            Err(UrlError::NoHost)
        ));
    }
}

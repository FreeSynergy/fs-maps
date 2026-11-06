//! Usage-policy checks for HTTP sources (RESEARCH.md section 1).
//!
//! Pattern: Strategy (validation rules) — small pure functions applied before any request is
//! sent, so a forbidden configuration never reaches the network.

use pmtiles::reqwest::Url;

use crate::error::{Error, Result};

/// Hosts for which tile requests are refused: the OSMF raster tile servers forbid bulk/offline
/// use and are not PMTiles anyway.
const FORBIDDEN_HOSTS: &[&str] = &["tile.openstreetmap.org", "tile.osm.org"];

/// Parse and validate a source URL (scheme, forbidden hosts).
pub(super) fn check_url(url: &str) -> Result<Url> {
    let url = Url::parse(url).map_err(|e| Error::Policy(format!("bad URL: {e}")))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Policy(
            "a user and password in the URL are not supported; a token in its query is".into(),
        ));
    }
    if !matches!(url.scheme(), "https" | "http") {
        return Err(Error::Policy(format!(
            "unsupported scheme {}",
            url.scheme()
        )));
    }
    if let Some(host) = url.host_str() {
        // `example.org.` is the same host as `example.org`.
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if FORBIDDEN_HOSTS
            .iter()
            .any(|h| host == *h || host.ends_with(&format!(".{h}")))
        {
            return Err(Error::Policy(format!(
                "{host}: OSMF tile servers forbid offline/prefetch use; use a PMTiles source"
            )));
        }
    }
    Ok(url)
}

/// The URL as it may be shown: what it names, without credentials, query or fragment.
pub(super) fn public_id(url: &Url) -> String {
    let mut shown = url.clone();
    let _ = shown.set_username("");
    let _ = shown.set_password(None);
    shown.set_query(None);
    shown.set_fragment(None);
    shown.to_string()
}

/// A redirect may stay on its host and port and not lose TLS, or go from http on 80 to https on 443;
/// the target passes the same checks as the start.
pub(super) fn redirect_allowed(from: &Url, to: &Url) -> bool {
    let host = |u: &Url| {
        u.host_str()
            .map(|h| h.trim_end_matches('.').to_ascii_lowercase())
    };
    let (ports, schemes) = (
        (from.port_or_known_default(), to.port_or_known_default()),
        (from.scheme(), to.scheme()),
    );
    let same_service = ports.0 == ports.1 && schemes.0 == schemes.1;
    let upgrade = schemes == ("http", "https") && ports == (Some(80), Some(443));
    host(from) == host(to) && (same_service || upgrade) && check_url(to.as_str()).is_ok()
}

/// Reject empty or library-default User-Agents.
pub(super) fn check_user_agent(ua: &str) -> Result<()> {
    let l = ua.trim().to_ascii_lowercase();
    let generic = [
        "reqwest",
        "curl/",
        "python-requests",
        "okhttp",
        "go-http-client",
        "java/",
    ];
    if l.len() < 5 || generic.iter().any(|g| l.starts_with(g)) {
        return Err(Error::Policy(
            "a clear application User-Agent is required (no library defaults)".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_dot_is_the_same_host() {
        assert!(check_url("https://tile.openstreetmap.org./x.pmtiles").is_err());
        assert!(check_url("https://a.tile.osm.org../x.pmtiles").is_err());
        assert!(check_url("https://example.org./x.pmtiles").is_ok());
    }

    #[test]
    fn only_a_redirect_on_the_same_host_passes() {
        let url = |u: &str| Url::parse(u).unwrap();
        let from = url("https://example.org/a.pmtiles");
        assert!(redirect_allowed(
            &from,
            &url("https://example.org/b.pmtiles")
        ));
        assert!(redirect_allowed(
            &from,
            &url("https://EXAMPLE.org./b.pmtiles")
        ));
        assert!(!redirect_allowed(
            &from,
            &url("https://other.org/a.pmtiles")
        ));
        assert!(
            !redirect_allowed(&from, &url("http://example.org/a.pmtiles")),
            "lost TLS"
        );
        let plain = url("http://example.org/a.pmtiles");
        assert!(redirect_allowed(
            &plain,
            &url("https://example.org/a.pmtiles")
        ));
        let other_port = "https://example.org:8443/a.pmtiles";
        assert!(!redirect_allowed(&from, &url(other_port)));
        assert!(!redirect_allowed(&plain, &url(other_port)), "not 80 to 443");
        assert!(!redirect_allowed(&plain, &url("http://example.org:8080/a")));
        let plain_8080 = url("http://example.org:8080/a");
        assert!(!redirect_allowed(
            &plain_8080,
            &url("https://example.org/a")
        ));
        assert!(!redirect_allowed(&from, &url("https://me:x@example.org/a")));
        let osm = url("https://tile.openstreetmap.org/a");
        assert!(!redirect_allowed(
            &osm,
            &url("https://tile.openstreetmap.org/b")
        ));
    }

    #[test]
    fn a_user_in_the_url_is_refused_without_repeating_it() {
        for url in [
            "https://me:secret@example.org/a",
            "https://secret@example.org/a",
        ] {
            let text = check_url(url).unwrap_err().to_string();
            assert!(
                text.contains("not supported") && !text.contains("secret"),
                "{text}"
            );
        }
        assert!(check_url("https://example.org/a?token=secret").is_ok());
    }

    #[test]
    fn what_is_shown_holds_no_credentials() {
        let url = Url::parse("https://me:secret@example.org/a.pmtiles?key=secret#x").unwrap();
        assert_eq!(public_id(&url), "https://example.org/a.pmtiles");
    }

    #[test]
    fn user_agent_rules() {
        assert!(check_user_agent("").is_err());
        assert!(check_user_agent("reqwest/0.13").is_err());
        assert!(check_user_agent("curl/8.0").is_err());
        assert!(check_user_agent("FreeSynergy-Maps/0.1 (+https://example.org)").is_ok());
    }

    #[test]
    fn urls() {
        assert!(check_url("https://tile.openstreetmap.org/0/0/0.png").is_err());
        assert!(check_url("https://a.tile.openstreetmap.org/x").is_err());
        assert!(check_url("ftp://example.org/a.pmtiles").is_err());
        assert!(check_url("https://example.org/a.pmtiles").is_ok());
    }
}

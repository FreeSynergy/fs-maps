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
    if !matches!(url.scheme(), "https" | "http") {
        return Err(Error::Policy(format!(
            "unsupported scheme {}",
            url.scheme()
        )));
    }
    if let Some(host) = url.host_str() {
        let host = host.to_ascii_lowercase();
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

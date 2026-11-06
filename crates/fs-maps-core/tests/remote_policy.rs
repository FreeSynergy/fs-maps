//! The usage policy against a remote archive on the loopback, and what is shown of its URL.

use std::sync::atomic::Ordering;

use fs_maps_core::testutil::{RangeServer, write_labelled};
use fs_maps_core::{Error, LazyHttpSource, PmtilesHttpSource, TileSource};

const UA: &str = "FreeSynergy-Maps-test/0.2 (+https://freesynergy.example)";

fn labelled(label: &str, z: u8, x: u32, y: u32) -> Vec<u8> {
    format!("{label} {z}/{x}/{y} {}", "~".repeat(200)).into_bytes()
}

async fn served(dir: &std::path::Path, label: &str) -> RangeServer {
    let file = dir.join(format!("{label}.pmtiles"));
    write_labelled(&file, 2, label).await.unwrap();
    RangeServer::start(&file).await.unwrap()
}

#[tokio::test]
async fn a_redirect_stays_on_its_host() {
    let dir = tempfile::tempdir().unwrap();
    let start = served(dir.path(), "A").await;
    let other = served(dir.path(), "B").await;
    // The same machine under another name is another host for the policy.
    let elsewhere = other
        .url
        .replace("127.0.0.1", "localhost")
        .replace("map.pmtiles", "elsewhere.pmtiles");
    start.redirect_to(&elsewhere);
    assert!(PmtilesHttpSource::open(&start.url, UA).await.is_err());
    assert_eq!(other.requests.load(Ordering::SeqCst), 0, "it left the host");

    // Another port on the same host is another service, and nothing tells it where we came from.
    let port = other.url.replace("map.pmtiles", "elsewhere.pmtiles");
    start.redirect_to(&port);
    assert!(PmtilesHttpSource::open(&start.url, UA).await.is_err());
    assert_eq!(other.requests.load(Ordering::SeqCst), 0, "it left the port");

    let forbidden = "https://tile.openstreetmap.org/x.pmtiles";
    start.redirect_to(forbidden);
    assert!(PmtilesHttpSource::open(&start.url, UA).await.is_err());

    let moved = start.url.replace("map.pmtiles", "moved.pmtiles");
    start.redirect_to(&moved);
    let with_token = format!("{}?token=secret", start.url);
    let source = PmtilesHttpSource::open(&with_token, UA).await.unwrap();
    assert_eq!(
        *start.referers.lock().unwrap(),
        [] as [String; 0],
        "a Referer went out"
    );
    assert_eq!(
        source.get_tile(1, 0, 0).await.unwrap().unwrap(),
        labelled("A", 1, 0, 0)
    );
}

#[tokio::test]
async fn a_forbidden_host_or_user_agent_fails_at_construction() {
    for url in [
        "https://tile.openstreetmap.org/x.pmtiles",
        "https://tile.openstreetmap.org./x.pmtiles",
        "https://a.tile.osm.org./x.pmtiles",
    ] {
        let e = LazyHttpSource::new(url, UA).err().unwrap();
        assert!(matches!(e, Error::Policy(_)), "{url}: {e:?}");
    }
    let url = "https://example.invalid/x.pmtiles";
    assert!(matches!(
        LazyHttpSource::new(url, "reqwest/1"),
        Err(Error::Policy(_))
    ));
    assert!(LazyHttpSource::new(url, "A long enough agent\nwith a newline").is_err());
}

#[tokio::test]
async fn a_token_in_the_url_is_not_shown() {
    let dir = tempfile::tempdir().unwrap();
    let server = served(dir.path(), "S").await;
    let url = format!("{}?token=secret", server.url);
    let public = server.url.clone();
    let lazy = LazyHttpSource::new(&url, UA).unwrap();
    assert_eq!(lazy.id(), public);
    assert_eq!(lazy.capabilities().provider(), format!("tiles:{public}"));
    assert!(lazy.get_tile(1, 0, 0).await.unwrap().is_some());
    assert_eq!(lazy.id(), public);

    let dead = "http://127.0.0.1:9/map.pmtiles?token=secret";
    let lazy = LazyHttpSource::new(dead, UA).unwrap();
    for _ in 0..2 {
        let text = lazy.get_tile(0, 0, 0).await.unwrap_err().to_string();
        assert!(
            !text.contains("secret") && !text.contains("token"),
            "{text}"
        );
    }
}

//! Tile source behaviour: policy refusal before any request, and fallback order.

use std::sync::Arc;

use fs_maps_core::{Error, FallbackSource, MemorySource, PmtilesHttpSource, TileSource};

#[tokio::test]
async fn osmf_tile_server_refused_before_any_request() {
    let ua = "FreeSynergy-Maps/0.1 (test)";
    for u in [
        "https://tile.openstreetmap.org/0/0/0.png",
        "https://a.tile.openstreetmap.org/x.pmtiles",
    ] {
        assert!(
            matches!(PmtilesHttpSource::open(u, ua).await, Err(Error::Policy(_))),
            "{u}"
        );
    }
    assert!(matches!(
        PmtilesHttpSource::open("https://example.org/a.pmtiles", "reqwest/1").await,
        Err(Error::Policy(_))
    ));
    assert!(matches!(
        PmtilesHttpSource::open("ftp://example.org/a.pmtiles", ua).await,
        Err(Error::Policy(_))
    ));
}

#[tokio::test]
async fn fallback_order() {
    let mut a = MemorySource::new("a");
    a.insert(1, 0, 0, &b"A"[..]);
    let mut b = MemorySource::new("b");
    b.insert(1, 0, 0, &b"B"[..]);
    b.insert(1, 1, 1, &b"B2"[..]);
    let f = FallbackSource::new(vec![Arc::new(a), Arc::new(b)]);
    assert_eq!(f.get_tile(1, 0, 0).await.unwrap().unwrap(), &b"A"[..]);
    assert_eq!(f.get_tile(1, 1, 1).await.unwrap().unwrap(), &b"B2"[..]);
    assert!(f.get_tile(1, 0, 1).await.unwrap().is_none());
}

//! The provider rule (ADR 0045): every provider has every method; what it does not offer answers
//! an explicit "not possible", and its report says the same.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::{ME, point, store};
use fs_maps_core::{
    Area, CacheConfig, CachedSource, Capabilities, Error, MemorySource, Paging, PlaceRepository,
    Places, Result, StoreView, TileCache, TileSource,
};
use fs_objects::{ObjectId, Store, StoredObject};

/// A read-only provider, for example a fixed list of places it cannot write to.
struct ReadOnly;

#[async_trait]
impl Places for ReadOnly {
    fn capabilities(&self) -> Capabilities {
        let mut c = Capabilities::new("read-only");
        c.supported("place.read");
        c.unsupported("place.create", "this list is fixed");
        c
    }
    async fn get(&self, _: ObjectId) -> Result<Option<StoredObject>> {
        Ok(None)
    }
    async fn list(&self, _: Paging) -> Result<Vec<StoredObject>> {
        Ok(vec![])
    }
}

fn capability<T: std::fmt::Debug>(r: Result<T>) -> String {
    match r {
        Err(Error::Unsupported(u)) => u.capability().to_string(),
        other => panic!("expected an explicit not possible, got {other:?}"),
    }
}

#[tokio::test]
async fn a_missing_method_says_not_possible() {
    let p: Arc<dyn Places> = Arc::new(ReadOnly);
    let f = point("x", 0.0, 0.0);
    let (id, area) = (ObjectId::new(), Area::new(0.0, 0.0, 1.0, 1.0).unwrap());
    assert!(p.list(Paging::default()).await.unwrap().is_empty());
    assert_eq!(capability(p.create(&f).await), "place.create");
    assert_eq!(capability(p.update(id, &f).await), "place.update");
    assert_eq!(capability(p.delete(id).await), "place.delete");
    assert_eq!(
        capability(p.within(&area, Paging::default()).await),
        "place.find"
    );
    assert_eq!(
        capability(p.find_by_name("x", Paging::default()).await),
        "place.find"
    );
    assert!(!p.capabilities().is_supported("place.create"));
}

#[tokio::test]
async fn the_store_backed_provider_reports_what_it_can() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    let caps = repo.capabilities();
    for id in [
        "place.create",
        "place.update",
        "place.delete",
        "place.read",
        "place.find",
    ] {
        assert!(caps.is_supported(id), "{id}");
    }
    assert_eq!(
        caps.why_not("place.subscribe").unwrap().reason(),
        "a borrowed repository has no live feed; StorePlaces has one"
    );
}

#[tokio::test]
async fn the_repository_works_behind_the_store_facade() {
    let s: Arc<dyn Store> = Arc::new(store().await);
    let view = StoreView::new(&*s);
    let repo = PlaceRepository::new(&view, ME);
    let p: &dyn Places = &repo;
    p.create(&point("Berlin", 13.4, 52.5)).await.unwrap();
    assert_eq!(p.list(Paging::default()).await.unwrap().len(), 1);
}

#[tokio::test]
async fn tile_sources_report_tile_read() {
    let mem = MemorySource::new("mem");
    assert!(mem.capabilities().is_supported("tile.read"));
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(TileCache::open(CacheConfig::new(dir.path(), 1 << 20)).unwrap());
    let cached = CachedSource::new(MemorySource::new("inner"), cache);
    assert_eq!(cached.capabilities().provider(), "tiles:inner");
}

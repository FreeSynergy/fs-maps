//! `within` at the edges of the world: the antimeridian, 180 = -180, the poles.

mod common;

use common::{ME, names, point, store};
use fs_maps_core::{Area, Paging, PlaceRepository, Places};

async fn within(repo: &PlaceRepository<'_>, a: [f64; 4]) -> Vec<String> {
    let area = Area::new(a[0], a[1], a[2], a[3]).unwrap();
    let found = repo.within(&area, Paging::default()).await.unwrap();
    names(&found).into_iter().map(String::from).collect()
}

#[tokio::test]
async fn longitude_excludes_inside_the_latitude_band_of_a_box_over_the_antimeridian() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    for (n, lon, lat) in [
        ("Auckland", 174.76, -36.85),
        ("Suva", 178.44, -18.14),
        ("Apia", -171.76, -13.83),
    ] {
        repo.create(&point(n, lon, lat)).await.unwrap();
    }
    repo.create(&point("Lima", -77.0, -12.0)).await.unwrap();
    repo.create(&point("Perth", 115.8, -31.9)).await.unwrap();
    let hits = within(&repo, [170.0, -40.0, -170.0, -10.0]).await;
    assert_eq!(hits, ["Apia", "Auckland", "Suva"]);
}

#[tokio::test]
async fn the_edges_of_the_world_are_found_from_the_other_side() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    repo.create(&point("East edge", 180.0, 10.0)).await.unwrap();
    repo.create(&point("West edge", -180.0, -10.0))
        .await
        .unwrap();
    repo.create(&point("Elsewhere", 0.0, 0.0)).await.unwrap();
    assert_eq!(
        within(&repo, [-180.0, 0.0, -170.0, 20.0]).await,
        ["East edge"]
    );
    assert_eq!(
        within(&repo, [170.0, -20.0, 180.0, 0.0]).await,
        ["West edge"]
    );
    assert_eq!(
        within(&repo, [-180.0, -20.0, -170.0, -5.0]).await,
        ["West edge"]
    );
    assert_eq!(
        within(&repo, [170.0, 0.0, 180.0, 20.0]).await,
        ["East edge"]
    );
    assert_eq!(
        within(&repo, [10.0, -20.0, 20.0, 20.0]).await,
        [] as [&str; 0]
    );
    assert_eq!(
        within(&repo, [-170.0, -20.0, 170.0, 20.0]).await,
        ["Elsewhere"]
    );
    assert_eq!(within(&repo, [-180.0, -1.0, 1.0, 1.0]).await, ["Elsewhere"]);

    repo.create(&point("North pole", 13.0, 90.0)).await.unwrap();
    repo.create(&point("South pole", -100.0, -90.0))
        .await
        .unwrap();
    assert_eq!(
        within(&repo, [100.0, 80.0, 110.0, 90.0]).await,
        ["North pole"]
    );
    assert_eq!(
        within(&repo, [100.0, -90.0, 110.0, -80.0]).await,
        ["South pole"]
    );
    assert_eq!(
        within(&repo, [100.0, 80.0, 110.0, 89.0]).await,
        [] as [&str; 0]
    );
    assert_eq!(
        within(&repo, [100.0, -5.0, 110.0, 5.0]).await,
        [] as [&str; 0]
    );
}

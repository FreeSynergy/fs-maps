//! `within` (bounding box, antimeridian, non-point geometries) and `findByName`, answered by
//! the store.

mod common;

use common::{ME, names, point, polygon, store};
use fs_maps_core::{Area, Error, Paging, PlaceRepository, Places};
use serde_json::json;

async fn world(repo: &PlaceRepository<'_>) {
    for (n, lon, lat) in [
        ("Berlin", 13.4, 52.5),
        ("Paris", 2.35, 48.85),
        ("Auckland", 174.76, -36.85),
        ("Suva", 178.44, -18.14),
        ("Apia", -171.76, -13.83),
        ("Honolulu", -157.86, 21.31),
    ] {
        repo.create(&point(n, lon, lat)).await.unwrap();
    }
}

async fn within(repo: &PlaceRepository<'_>, a: [f64; 4]) -> Vec<String> {
    let area = Area::new(a[0], a[1], a[2], a[3]).unwrap();
    let found = repo.within(&area, Paging::default()).await.unwrap();
    names(&found).into_iter().map(String::from).collect()
}

#[tokio::test]
async fn a_box_finds_the_points_inside_and_only_those() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    world(&repo).await;
    assert_eq!(
        within(&repo, [0.0, 40.0, 20.0, 60.0]).await,
        ["Berlin", "Paris"]
    );
    assert_eq!(within(&repo, [10.0, 50.0, 14.0, 53.0]).await, ["Berlin"]);
    assert!(within(&repo, [-10.0, -10.0, -5.0, -5.0]).await.is_empty());
    // Edges touch: a box whose corner is exactly on a point still finds it.
    assert_eq!(within(&repo, [13.4, 52.5, 20.0, 60.0]).await, ["Berlin"]);
    assert_eq!(within(&repo, [-180.0, -90.0, 180.0, 90.0]).await.len(), 6);
}

#[tokio::test]
async fn a_box_over_the_antimeridian_finds_both_sides() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    world(&repo).await;
    // Fiji, Samoa and New Zealand's far east lie on either side of the 180th meridian.
    let hits = within(&repo, [170.0, -40.0, -170.0, -10.0]).await;
    assert_eq!(hits, ["Apia", "Auckland", "Suva"]);
    // The same numbers read the other way round are the huge box through Europe, not that one.
    let europe = within(&repo, [-170.0, -40.0, 170.0, 60.0]).await;
    assert!(europe.contains(&"Berlin".to_string()) && !europe.contains(&"Suva".to_string()));
    // Latitude still limits it.
    assert_eq!(
        within(&repo, [170.0, 0.0, -150.0, 30.0]).await,
        ["Honolulu"]
    );
}

#[tokio::test]
async fn non_point_geometries_overlap_by_their_bounding_box() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    // An L-shaped polygon: its bounding box is [0,0]..[10,10], the notch [5,5]..[10,10] is empty.
    repo.create(&polygon(
        "L",
        &[
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 5.0],
            [5.0, 5.0],
            [5.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ],
    ))
    .await
    .unwrap();
    repo.create(&json!({"type":"Feature","properties":{"name":"Road"},
        "geometry":{"type":"LineString","coordinates":[[20.0,20.0],[30.0,25.0]]}}))
        .await
        .unwrap();
    repo.create(&json!({"type":"Feature","properties":{"name":"Nowhere"},"geometry":null}))
        .await
        .unwrap();
    // Inside the notch: no geometry point is there, but the bounding box is, so it overlaps.
    assert_eq!(within(&repo, [7.0, 7.0, 8.0, 8.0]).await, ["L"]);
    // A box that contains the whole polygon, and one that only clips a corner.
    assert_eq!(within(&repo, [-5.0, -5.0, 15.0, 15.0]).await, ["L"]);
    assert_eq!(within(&repo, [9.0, 9.0, 12.0, 12.0]).await, ["L"]);
    // The line is found by any box meeting its bounding box; a box beside it is not.
    assert_eq!(within(&repo, [24.0, 21.0, 26.0, 22.0]).await, ["Road"]);
    assert!(within(&repo, [31.0, 0.0, 40.0, 50.0]).await.is_empty());
    // Without a geometry a place is in no area.
    assert_eq!(
        within(&repo, [-180.0, -90.0, 180.0, 90.0]).await,
        ["L", "Road"]
    );
}

#[tokio::test]
async fn a_geometry_over_the_antimeridian_is_found_from_both_sides() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    // RFC 7946: positions on both sides of 180; the bounding box then spans all longitudes.
    repo.create(&json!({"type":"Feature","properties":{"name":"Strait"},
        "geometry":{"type":"LineString","coordinates":[[179.0,65.0],[-179.0,65.5]]}}))
        .await
        .unwrap();
    for a in [
        [178.0, 64.0, 180.0, 66.0],
        [-180.0, 64.0, -178.0, 66.0],
        [170.0, 60.0, -170.0, 70.0],
    ] {
        assert_eq!(within(&repo, a).await, ["Strait"], "{a:?}");
    }
}

#[tokio::test]
async fn within_is_paged_by_the_store() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    for i in 0..30 {
        repo.create(&point(&format!("p{i:02}"), f64::from(i), 10.0))
            .await
            .unwrap();
    }
    let area = Area::new(5.0, 0.0, 24.0, 20.0).unwrap();
    let all = repo.within(&area, Paging::default()).await.unwrap();
    assert_eq!(all.len(), 20);
    let page = repo.within(&area, Paging::new(5, 10)).await.unwrap();
    assert_eq!(names(&page), ["p15", "p16", "p17", "p18", "p19"]);
}

#[tokio::test]
async fn invalid_areas_are_refused() {
    for a in [
        [0.0, 10.0, 1.0, 5.0],
        [-181.0, 0.0, 0.0, 1.0],
        [0.0, -91.0, 1.0, 1.0],
        [f64::NAN, 0.0, 1.0, 1.0],
    ] {
        assert!(
            matches!(
                Area::new(a[0], a[1], a[2], a[3]),
                Err(Error::InvalidBBox(_))
            ),
            "{a:?}"
        );
    }
}

#[tokio::test]
async fn find_by_name_ignores_case_and_looks_at_all_names() {
    let s = store().await;
    let repo = PlaceRepository::new(&s, ME);
    repo.create(&json!({"type":"Feature","geometry":null,
        "properties":{"name":"Munich","name:de":"München","name:it":"Monaco di Baviera"}}))
        .await
        .unwrap();
    repo.create(&point("Monaco", 7.42, 43.73)).await.unwrap();
    repo.create(&point("100% Wasser_x", 1.0, 1.0))
        .await
        .unwrap();
    let find = |t: &'static str| {
        let repo = &repo;
        async move {
            names(&repo.find_by_name(t, Paging::default()).await.unwrap())
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(find("mon").await, ["Monaco", "Munich"]);
    assert_eq!(find("MÜNCHEN").await, ["Munich"]);
    assert_eq!(find("100%").await, ["100% Wasser_x"]);
    assert!(find("%").await.len() == 1, "% is a literal");
    assert!(find("zzz").await.is_empty());
}

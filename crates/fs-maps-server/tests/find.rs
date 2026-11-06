//! `list`, `within` and `findByName`: what they exclude, their order, and how they page.

mod common;

use common::{create, create_text, delete, list_all, page_of, point, with_server};
use fs_interfaces::places_capnp::places;
use fs_interfaces::rpc::{MAX_PAGE_BYTES, write_object_id};
use fs_store::ObjectId;

fn set_box(mut area: fs_interfaces::places_capnp::b_box::Builder<'_>, b: [f64; 4]) {
    area.set_west(b[0]);
    area.set_south(b[1]);
    area.set_east(b[2]);
    area.set_north(b[3]);
}

/// All pages of `within`.
async fn within_all(places: &places::Client, b: [f64; 4]) -> Vec<(ObjectId, String)> {
    let (mut out, mut cursor) = (Vec::new(), Vec::new());
    loop {
        let mut req = places.within_request();
        set_box(req.get().init_box(), b);
        req.get().init_cursor().set_opaque(&cursor);
        let answer = req.send().promise.await.unwrap();
        let (items, next) = page_of(answer.get().unwrap().get_page().unwrap());
        out.extend(items);
        cursor = next;
        if cursor.is_empty() {
            return out;
        }
    }
}

/// All pages of `findByName`.
async fn named_all(places: &places::Client, term: &str) -> Vec<(ObjectId, String)> {
    let (mut out, mut cursor) = (Vec::new(), Vec::new());
    loop {
        let mut req = places.find_by_name_request();
        req.get().set_term(term);
        req.get().init_cursor().set_opaque(&cursor);
        let answer = req.send().promise.await.unwrap();
        let (items, next) = page_of(answer.get().unwrap().get_page().unwrap());
        out.extend(items);
        cursor = next;
        if cursor.is_empty() {
            return out;
        }
    }
}

fn names(items: &[(ObjectId, String)]) -> Vec<&str> {
    items.iter().map(|(_, n)| n.as_str()).collect()
}

#[tokio::test(flavor = "current_thread")]
async fn within_finds_by_area_also_over_the_antimeridian() {
    with_server(|maps| async move {
        let p = maps.places;
        create(&p, "Berlin", 13.4, 52.5).await;
        create(&p, "Paris", 2.35, 48.85).await;
        create(&p, "Suva", 178.44, -18.14).await;
        create(&p, "Apia", -171.76, -13.83).await;
        create(&p, "Lima", -77.0, -12.0).await; // in the latitudes of the boxes below, not their longitudes
        create(&p, "Darwin", 130.8, -12.5).await;
        let unlocated = r#"{"type":"Feature","geometry":null,"properties":{"name":"Nowhere"}}"#;
        create_text(&p, unlocated).await;

        let found = async |b| names(&within_all(&p, b).await).join(",");
        assert_eq!(found([0.0, 40.0, 20.0, 60.0]).await, "Berlin,Paris");
        assert_eq!(found([10.0, 50.0, 20.0, 60.0]).await, "Berlin");
        assert_eq!(found([10.0, 53.0, 20.0, 60.0]).await, "", "north of it");
        assert_eq!(found([13.5, 50.0, 20.0, 60.0]).await, "", "east of it");
        assert_eq!(found([100.0, 0.0, 110.0, 10.0]).await, "");
        assert_eq!(found([170.0, -30.0, -170.0, 0.0]).await, "Apia,Suva");
        assert_eq!(found([175.0, -30.0, 179.0, 0.0]).await, "Suva");
        assert_eq!(found([-180.0, -30.0, -170.0, 0.0]).await, "Apia");
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_moved_or_deleted_place_is_found_only_where_it_is() {
    with_server(|maps| async move {
        let p = maps.places;
        let (here, there) = ([13.0, 52.0, 14.0, 53.0], [2.0, 48.0, 3.0, 49.0]);
        let id = create(&p, "Wanderer", 13.4, 52.5).await;
        assert_eq!(within_all(&p, here).await.len(), 1);
        assert!(within_all(&p, there).await.is_empty());

        let mut update = p.update_request();
        write_object_id(update.get().init_id(), id);
        update
            .get()
            .set_geojson(point("Settled", 2.35, 48.85).as_str());
        update.send().promise.await.unwrap();
        assert!(within_all(&p, here).await.is_empty(), "stale position");
        assert_eq!(within_all(&p, there).await, [(id, "Settled".into())]);
        assert!(named_all(&p, "Wanderer").await.is_empty(), "stale name");
        assert_eq!(named_all(&p, "settled").await, [(id, "Settled".into())]);

        delete(&p, id).await;
        assert!(within_all(&p, there).await.is_empty());
        assert!(named_all(&p, "settled").await.is_empty());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn find_by_name_ignores_case_and_sees_every_name() {
    with_server(|maps| async move {
        let p = maps.places;
        let berlin = create(&p, "Berlin", 13.4, 52.5).await;
        let text = r#"{"type":"Feature","geometry":null,"properties":{"name":"Munich","name:de":"München"}}"#;
        let munich = create_text(&p, text).await;
        create(&p, "Paris", 2.35, 48.85).await;
        assert_eq!(named_all(&p, "BERL").await, [(berlin, "Berlin".into())]);
        assert_eq!(named_all(&p, "  berl ").await, [(berlin, "Berlin".into())], "trimmed");
        assert_eq!(named_all(&p, "münch").await, [(munich, "Munich".into())]);
        assert!(named_all(&p, "100%").await.is_empty(), "% is no wildcard");
        assert!(named_all(&p, "_erlin").await.is_empty(), "_ is no wildcard");
        assert!(named_all(&p, "Rome").await.is_empty());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn every_list_gives_name_order_once_across_pages() {
    with_server(|maps| async move {
        let p = maps.places;
        // Case does not decide the order: `apple` before `Banana` before `cherry`.
        for name in ["cherry", "Banana", "apple"] {
            let id = create(&p, name, 13.0, 52.0).await;
            assert_eq!(named_all(&p, name).await, [(id, name.to_string())]);
        }
        let mut same = Vec::new();
        for _ in 0..105 {
            same.push(create(&p, "date", 13.0, 52.0).await);
        }
        same.sort(); // equal names: by id

        let (ids, pages) = list_all(&p).await;
        assert_eq!(pages, 2);
        assert_eq!(ids[3..], same[..], "the page boundary lost or repeated one");
        let expected = ["apple", "Banana", "cherry"];
        let around = within_all(&p, [12.0, 51.0, 14.0, 53.0]).await;
        assert_eq!(names(&around)[..3], expected);
        assert_eq!(around.len(), 108);
        let ordered: Vec<ObjectId> = around.iter().map(|(id, _)| *id).collect();
        assert_eq!(ordered, ids);
        let dates = named_all(&p, "date").await;
        let dates: Vec<ObjectId> = dates.into_iter().map(|(id, _)| id).collect();
        assert_eq!(dates, same);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_page_ends_before_it_outgrows_a_message() {
    with_server(|maps| async move {
        let p = maps.places;
        let filler = "x".repeat(1024 * 1024);
        let text = format!(
            r#"{{"type":"Feature","geometry":null,"properties":{{"name":"Heavy","note":"{filler}"}}}}"#
        );
        let count = MAX_PAGE_BYTES / filler.len() + 5;
        let mut all = Vec::new();
        for _ in 0..count {
            all.push(create_text(&p, &text).await);
        }
        all.sort();
        let (ids, pages) = list_all(&p).await;
        assert_eq!(pages, 2);
        assert_eq!(ids, all);
    })
    .await;
}

//! `list`, `within` and `findByName`, and how they page.

mod common;

use std::collections::HashSet;

use common::{create, create_text, list_all, page_of, with_server};
use fs_interfaces::places_capnp::places;
use fs_interfaces::rpc::MAX_PAGE_BYTES;

async fn within(places: &places::Client, b: [f64; 4]) -> Vec<String> {
    let mut req = places.within_request();
    let mut area = req.get().init_box();
    area.set_west(b[0]);
    area.set_south(b[1]);
    area.set_east(b[2]);
    area.set_north(b[3]);
    let answer = req.send().promise.await.unwrap();
    page_of(answer.get().unwrap().get_page().unwrap()).0
}

async fn named(places: &places::Client, term: &str) -> Vec<String> {
    let mut req = places.find_by_name_request();
    req.get().set_term(term);
    let answer = req.send().promise.await.unwrap();
    page_of(answer.get().unwrap().get_page().unwrap()).0
}

#[tokio::test(flavor = "current_thread")]
async fn within_finds_by_area_also_over_the_antimeridian() {
    with_server(|maps| async move {
        let p = maps.places;
        create(&p, "Berlin", 13.4, 52.5).await;
        create(&p, "Paris", 2.35, 48.85).await;
        create(&p, "Suva", 178.44, -18.14).await;
        create(&p, "Apia", -171.76, -13.83).await;
        let unlocated = r#"{"type":"Feature","geometry":null,"properties":{"name":"Nowhere"}}"#;
        create_text(&p, unlocated).await;

        assert_eq!(
            within(&p, [0.0, 40.0, 20.0, 60.0]).await,
            ["Berlin", "Paris"]
        );
        assert_eq!(within(&p, [10.0, 50.0, 20.0, 60.0]).await, ["Berlin"]);
        assert!(within(&p, [100.0, 0.0, 110.0, 10.0]).await.is_empty());
        assert_eq!(
            within(&p, [170.0, -30.0, -170.0, 0.0]).await,
            ["Apia", "Suva"]
        );
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn find_by_name_ignores_case_and_sees_every_name() {
    with_server(|maps| async move {
        let p = maps.places;
        create(&p, "Berlin", 13.4, 52.5).await;
        let text = r#"{"type":"Feature","geometry":null,"properties":{"name":"Munich","name:de":"München"}}"#;
        create_text(&p, text).await;
        assert_eq!(named(&p, "BERL").await, ["Berlin"]);
        assert_eq!(named(&p, "münch").await, ["Munich"]);
        assert!(named(&p, "100%").await.is_empty());
        assert!(named(&p, "Paris").await.is_empty());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn lists_page_through_everything_once() {
    with_server(|maps| async move {
        let p = maps.places;
        for _ in 0..105 {
            create(&p, "Same name", 13.0, 52.0).await;
        }
        let (ids, pages) = list_all(&p).await;
        assert_eq!((ids.len(), pages), (105, 2));
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 105);

        let mut first = p.within_request();
        let mut area = first.get().init_box();
        area.set_west(0.0);
        area.set_east(20.0);
        area.set_north(60.0);
        let first = first.send().promise.await.unwrap();
        let (items, next) = page_of(first.get().unwrap().get_page().unwrap());
        assert_eq!(items.len(), 100);

        let mut rest = p.within_request();
        rest.get().init_cursor().set_opaque(&next);
        let mut area = rest.get().init_box();
        area.set_west(0.0);
        area.set_east(20.0);
        area.set_north(60.0);
        let rest = rest.send().promise.await.unwrap();
        let (items, next) = page_of(rest.get().unwrap().get_page().unwrap());
        assert_eq!((items.len(), next.len()), (5, 0));

        let mut first = p.find_by_name_request();
        first.get().set_term("same");
        let first = first.send().promise.await.unwrap();
        let (_, next) = page_of(first.get().unwrap().get_page().unwrap());
        let mut rest = p.find_by_name_request();
        rest.get().set_term("same");
        rest.get().init_cursor().set_opaque(&next);
        let rest = rest.send().promise.await.unwrap();
        assert_eq!(page_of(rest.get().unwrap().get_page().unwrap()).0.len(), 5);
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
        for _ in 0..count {
            create_text(&p, &text).await;
        }
        let (ids, pages) = list_all(&p).await;
        assert_eq!((ids.len(), pages), (count, 2));
    })
    .await;
}

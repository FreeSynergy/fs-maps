//! Failures, as a consumer sees them: typed, and the connection lives on.

mod common;

use common::{create, create_text, envelope_of, list_all, point, problem, with_server};
use fs_interfaces::places_capnp::places;
use fs_interfaces::rpc::{MAX_PAGE_BYTES, write_object_id};
use fs_store::ObjectId;

const INVALID: &str = "https://freesynergy.net/problems/places/invalid";
const NOT_FOUND: &str = "https://freesynergy.net/problems/places/not-found";
const TOO_LARGE: &str = "https://freesynergy.net/problems/places/too-large";

async fn refused_create(places: &places::Client, text: &str) -> (u16, String) {
    let mut req = places.create_request();
    req.get().set_geojson(text);
    let e = req.send().promise.await.err().unwrap();
    let p = problem(&e);
    (p.status, p.problem_type)
}

#[tokio::test(flavor = "current_thread")]
async fn a_bad_feature_is_invalid() {
    with_server(|maps| async move {
        let p = maps.places;
        for text in [
            "{ not json",
            r#"{"type":"Point","coordinates":[1,2]}"#,
            r#"{"type":"Feature","geometry":null,"properties":{}}"#,
            &point("Off the map", 500.0, 0.0),
        ] {
            assert_eq!(
                refused_create(&p, text).await,
                (422, INVALID.into()),
                "{text}"
            );
        }
        assert!(list_all(&p).await.0.is_empty());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_unknown_place_is_not_found() {
    with_server(|maps| async move {
        let p = maps.places;
        let unknown = ObjectId::new();

        let mut update = p.update_request();
        write_object_id(update.get().init_id(), unknown);
        update.get().set_geojson(point("x", 0.0, 0.0).as_str());
        let e = update.send().promise.await.err().unwrap();
        assert_eq!(problem(&e).problem_type, NOT_FOUND);
        assert_eq!(problem(&e).status, 404);

        let mut delete = p.delete_request();
        write_object_id(delete.get().init_id(), unknown);
        let e = delete.send().promise.await.err().unwrap();
        assert_eq!(problem(&e).status, 404);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_id_box_or_broken_cursor_is_invalid() {
    with_server(|maps| async move {
        let p = maps.places;
        let e = p.get_request().send().promise.await.err().unwrap();
        assert_eq!(problem(&e).problem_type, INVALID);
        let e = p.delete_request().send().promise.await.err().unwrap();
        assert_eq!(problem(&e).status, 422);

        let e = p.within_request().send().promise.await.err().unwrap();
        assert_eq!(problem(&e).status, 422);
        for (west, south, east, north) in [
            (0.0, 10.0, 1.0, 5.0),
            (f64::NAN, 0.0, 1.0, 1.0),
            (-200.0, 0.0, 1.0, 1.0),
        ] {
            let mut req = p.within_request();
            let mut area = req.get().init_box();
            area.set_west(west);
            area.set_south(south);
            area.set_east(east);
            area.set_north(north);
            let e = req.send().promise.await.err().unwrap();
            assert_eq!(problem(&e).problem_type, INVALID);
        }

        create(&p, "Berlin", 13.4, 52.5).await;
        let mut list = p.list_request();
        list.get().init_cursor().set_opaque(b"not a cursor");
        let e = list.send().promise.await.err().unwrap();
        assert_eq!(problem(&e).status, 422);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn what_could_not_be_answered_is_refused_and_the_limit_itself_works() {
    with_server(|maps| async move {
        let p = maps.places;
        let named = |filler: usize| {
            format!(
                r#"{{"type":"Feature","geometry":null,"properties":{{"name":"{}"}}}}"#,
                "x".repeat(filler)
            )
        };
        let (status, kind) = refused_create(&p, &named(MAX_PAGE_BYTES)).await;
        assert_eq!((status, kind.as_str()), (413, TOO_LARGE));

        // The worst case: all of it is the name, which the envelope carries a second time.
        let filler = MAX_PAGE_BYTES - 2 * 4096 - named(0).len();
        let id = create_text(&p, &named(filler)).await;
        let mut get = p.get_request();
        write_object_id(get.get().init_id(), id);
        let got = get.send().promise.await.unwrap();
        let place = got.get().unwrap().get_place().unwrap();
        assert_eq!(envelope_of(place).name.len(), filler);
        assert_eq!(list_all(&p).await.0, [id]);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_place_is_measured_as_it_would_be_stored() {
    with_server(|maps| async move {
        let p = maps.places;
        // Fits as it arrives; every number more than doubles when written back.
        let numbers = vec!["1E5"; MAX_PAGE_BYTES / 6].join(",");
        let text = format!(
            r#"{{"type":"Feature","geometry":null,"properties":{{"name":"n","note":[{numbers}]}}}}"#
        );
        assert!(text.len() < MAX_PAGE_BYTES);
        let (status, kind) = refused_create(&p, &text).await;
        assert_eq!((status, kind.as_str()), (413, TOO_LARGE));
        assert!(list_all(&p).await.0.is_empty());
    })
    .await;
}

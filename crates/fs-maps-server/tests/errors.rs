//! Failures, as a consumer sees them: typed, with the exact type, and the connection lives on.

mod common;

use common::{
    INVALID, INVALID_REQUEST, NOT_FOUND, STORED_DATA, create, is, list_all, point, refused,
    with_server, with_store,
};
use fs_interfaces::rpc::write_object_id;
use fs_objects::{Envelope, ObjectId, StoredObject};
use serde_json::json;

#[tokio::test(flavor = "current_thread")]
async fn a_bad_feature_is_refused_and_nothing_is_stored() {
    with_server(|maps| async move {
        let p = maps.places;
        for (text, kind) in [
            ("", INVALID_REQUEST),
            ("{ not json", INVALID_REQUEST),
            (r#"{"type":"Point","coordinates":[1,2]}"#, INVALID),
            (
                r#"{"type":"Feature","geometry":null,"properties":{}}"#,
                INVALID,
            ),
            (
                r#"{"type":"Feature","properties":{"name":"No geometry member"}}"#,
                INVALID,
            ),
            (&point("Off the map", 500.0, 0.0), INVALID),
            (&point("Off the map", 0.0, 91.0), INVALID),
        ] {
            let mut req = p.create_request();
            req.get().set_geojson(text);
            assert_eq!(refused(req.send().promise.await), is(kind, 422), "{text}");
        }
        assert!(list_all(&p).await.0.is_empty());

        let id = create(&p, "Kept", 1.0, 2.0).await;
        let mut update = p.update_request();
        write_object_id(update.get().init_id(), id);
        update.get().set_geojson(point("Off", 500.0, 0.0).as_str());
        assert_eq!(refused(update.send().promise.await), is(INVALID, 422));
        assert_eq!(list_all(&p).await.0, [id]);
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
        assert_eq!(refused(update.send().promise.await), is(NOT_FOUND, 404));
        assert!(list_all(&p).await.0.is_empty(), "the update created it");

        let mut delete = p.delete_request();
        write_object_id(delete.get().init_id(), unknown);
        assert_eq!(refused(delete.send().promise.await), is(NOT_FOUND, 404));
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_value_or_broken_cursor_is_an_invalid_request() {
    with_server(|maps| async move {
        let p = maps.places;
        let wrong = is(INVALID_REQUEST, 422);
        assert_eq!(refused(p.get_request().send().promise.await), wrong);
        assert_eq!(refused(p.delete_request().send().promise.await), wrong);
        assert_eq!(refused(p.update_request().send().promise.await), wrong);
        assert_eq!(refused(p.within_request().send().promise.await), wrong);
        assert_eq!(
            refused(p.find_by_name_request().send().promise.await),
            wrong
        );
        assert_eq!(refused(p.subscribe_request().send().promise.await), wrong);
        let mut blank = p.find_by_name_request();
        blank.get().set_term("  ");
        assert_eq!(refused(blank.send().promise.await), wrong);

        for (west, south, east, north) in [
            (0.0, 10.0, 1.0, 5.0),
            (f64::NAN, 0.0, 1.0, 1.0),
            (f64::INFINITY, 0.0, 1.0, 1.0),
            (-200.0, 0.0, 1.0, 1.0),
            (0.0, -91.0, 1.0, 1.0),
        ] {
            let mut req = p.within_request();
            let mut area = req.get().init_box();
            area.set_west(west);
            area.set_south(south);
            area.set_east(east);
            area.set_north(north);
            assert_eq!(refused(req.send().promise.await), is(INVALID, 422));
        }

        create(&p, "Berlin", 13.4, 52.5).await;
        let mut list = p.list_request();
        list.get().init_cursor().set_opaque(b"not a cursor");
        assert_eq!(refused(list.send().promise.await), wrong);
        let mut named = p.find_by_name_request();
        named.get().set_term("b");
        named.get().init_cursor().set_opaque(&[9; 9]);
        assert_eq!(refused(named.send().promise.await), wrong);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_stored_place_that_no_longer_reads_is_not_the_callers_fault() {
    with_store(|maps, store| async move {
        let p = maps.places;
        let broken = StoredObject::new(
            Envelope::new("Place", "Broken"),
            "application/geo+json",
            json!({"type":"Nonsense"}),
        );
        let id = broken.envelope.id;
        store.put(broken, common::ME).await.unwrap();

        let mut get = p.get_request();
        write_object_id(get.get().init_id(), id);
        assert_eq!(refused(get.send().promise.await), is(STORED_DATA, 500));
        let list = p.list_request().send().promise.await;
        assert_eq!(refused(list), is(STORED_DATA, 500));
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_place_stored_before_a_rule_was_added_still_reads() {
    with_store(|maps, store| async move {
        let p = maps.places;
        // A ring of three positions, refused now and accepted by an earlier version.
        let geojson = json!({"type":"Feature",
            "geometry":{"type":"Polygon","coordinates":[[[0,0],[2,0],[0,0]]]},
            "properties":{"name":"Old"}});
        let old = StoredObject::new(
            Envelope::new("Place", "Old"),
            "application/geo+json",
            geojson.clone(),
        );
        let id = old.envelope.id;
        store.put(old, common::ME).await.unwrap();

        let mut get = p.get_request();
        write_object_id(get.get().init_id(), id);
        let got = get.send().promise.await.unwrap();
        let text = got
            .get()
            .unwrap()
            .get_place()
            .unwrap()
            .get_geojson()
            .unwrap();
        let read: serde_json::Value = serde_json::from_str(text.to_str().unwrap()).unwrap();
        assert_eq!(read, geojson);
        assert_eq!(list_all(&p).await.0, [id]);

        // Coming in again it meets today's rule, and it stays as it was.
        let mut update = p.update_request();
        write_object_id(update.get().init_id(), id);
        update.get().set_geojson(geojson.to_string().as_str());
        assert_eq!(refused(update.send().promise.await), is(INVALID, 422));
        let mut create = p.create_request();
        create.get().set_geojson(geojson.to_string().as_str());
        assert_eq!(refused(create.send().promise.await), is(INVALID, 422));
        assert_eq!(list_all(&p).await.0, [id]);
    })
    .await;
}

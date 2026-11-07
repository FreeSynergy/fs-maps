//! Create, get, update and delete over a real socket, against the store-backed provider.

mod common;

use common::{
    NOT_FOUND, create, create_text, delete, envelope_of, is, list_all, point, refused, with_server,
    with_store,
};
use fs_interfaces::places_capnp::places;
use fs_interfaces::rpc::write_object_id;
use fs_objects::{Envelope, ObjectId, StoredObject};
use serde_json::{Value, json};

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// `found` and, if found, the envelope and feature.
async fn get(places: &places::Client, id: ObjectId) -> Option<(Envelope, Value)> {
    let mut req = places.get_request();
    write_object_id(req.get().init_id(), id);
    let answer = req.send().promise.await.unwrap();
    let r = answer.get().unwrap();
    r.get_found().then(|| {
        let place = r.get_place().unwrap();
        let text = place.get_geojson().unwrap().to_str().unwrap();
        (envelope_of(place), parsed(text))
    })
}

#[tokio::test(flavor = "current_thread")]
async fn a_place_goes_through_its_life() {
    with_server(|maps| async move {
        let p = maps.places;
        let sent = point("Berlin", 13.4, 52.5);

        let mut req = p.create_request();
        req.get().set_geojson(sent.as_str());
        let answer = req.send().promise.await.unwrap();
        let created = answer.get().unwrap().get_place().unwrap();
        let envelope = envelope_of(created);
        let id = envelope.id;
        assert_eq!(envelope.object_type, "Place");
        assert_eq!(envelope.name, "Berlin");
        assert_eq!(envelope.context.as_deref(), Some("places"));
        assert_eq!(envelope.attributed_to.as_deref(), Some(common::ME));
        let iri = format!("https://local.invalid/places/{id}");
        assert_eq!(envelope.iri.as_deref(), Some(iri.as_str()));
        let text = created.get_geojson().unwrap().to_str().unwrap();
        assert_eq!(parsed(text), parsed(&sent));

        let (got, feature) = get(&p, id).await.unwrap();
        assert_eq!(got, envelope);
        assert_eq!(feature, parsed(&sent));

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let moved = point("Berlin Mitte", 13.39, 52.52);
        let mut update = p.update_request();
        write_object_id(update.get().init_id(), id);
        update.get().set_geojson(moved.as_str());
        let updated = update.send().promise.await.unwrap();
        let updated = updated.get().unwrap().get_place().unwrap();
        let after = envelope_of(updated);
        assert_eq!((after.id, after.name.as_str()), (id, "Berlin Mitte"));
        assert_eq!(after.published, envelope.published);
        assert_eq!(after.iri, envelope.iri);
        assert!(after.updated > envelope.updated, "updated did not move");
        let text = updated.get_geojson().unwrap().to_str().unwrap();
        assert_eq!(parsed(text), parsed(&moved));
        assert_eq!(get(&p, id).await.unwrap().1, parsed(&moved));

        delete(&p, id).await;
        assert!(get(&p, id).await.is_none());
        assert!(list_all(&p).await.0.is_empty());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn the_id_inside_a_feature_is_content_never_the_object() {
    with_server(|maps| async move {
        let p = maps.places;
        let with_id = |name: &str, member: &str| {
            json!({"type":"Feature","id":member,"geometry":null,"properties":{"name":name}})
                .to_string()
        };
        let first = create_text(&p, &with_id("First", "same")).await;
        let second = create_text(&p, &with_id("Second", "same")).await;
        let chosen = create_text(&p, &with_id("Third", &first.to_string())).await;
        assert_ne!(first, second);
        assert_ne!(first, chosen);
        let (kept, feature) = get(&p, first).await.unwrap();
        assert_eq!(kept.name, "First");
        assert_eq!(feature["id"], "same");
        assert_eq!(get(&p, second).await.unwrap().0.name, "Second");
        assert_eq!(list_all(&p).await.0.len(), 3);

        let mut update = p.update_request();
        write_object_id(update.get().init_id(), second);
        update
            .get()
            .set_geojson(with_id("Renamed", &first.to_string()).as_str());
        update.send().promise.await.unwrap();
        assert_eq!(get(&p, first).await.unwrap().0.name, "First");
        assert_eq!(get(&p, second).await.unwrap().0.name, "Renamed");
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn the_edges_of_the_world_are_places() {
    with_server(|maps| async move {
        let p = maps.places;
        for (lon, lat) in [
            (180.0, 90.0),
            (-180.0, -90.0),
            (0.0, 0.0),
            (179.9999999, 89.9999999),
        ] {
            let id = create(&p, "Edge", lon, lat).await;
            let (_, feature) = get(&p, id).await.unwrap();
            let at = &feature["geometry"]["coordinates"];
            assert_eq!((at[0].as_f64(), at[1].as_f64()), (Some(lon), Some(lat)));
        }
        let control =
            json!({"type":"Feature","geometry":null,"properties":{"name":"a\u{1f}b\u{0}c"}});
        let id = create_text(&p, &control.to_string()).await;
        assert_eq!(get(&p, id).await.unwrap().0.name, "a\u{1f}b\u{0}c");
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn another_kind_of_object_in_the_store_is_no_place() {
    with_store(|maps, store| async move {
        let p = maps.places;
        let note = StoredObject::new(
            Envelope::new("Note", "Not a place"),
            "text/plain",
            json!("x"),
        );
        let id = note.envelope.id;
        store.put(note, common::ME).await.unwrap();

        assert!(get(&p, id).await.is_none());
        assert!(list_all(&p).await.0.is_empty());
        let mut remove = p.delete_request();
        write_object_id(remove.get().init_id(), id);
        assert_eq!(refused(remove.send().promise.await), is(NOT_FOUND, 404));
        let mut update = p.update_request();
        write_object_id(update.get().init_id(), id);
        update.get().set_geojson(point("x", 0.0, 0.0).as_str());
        assert_eq!(refused(update.send().promise.await), is(NOT_FOUND, 404));
        let kept = store.get(id).await.unwrap().unwrap();
        assert_eq!(kept.envelope.name, "Not a place");
    })
    .await;
}

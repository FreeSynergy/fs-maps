//! Create, get, update and delete over a real socket, against the store-backed provider.

mod common;

use common::{create, delete, envelope_of, point, with_server};
use fs_interfaces::rpc::write_object_id;
use serde_json::Value;

#[tokio::test(flavor = "current_thread")]
async fn a_place_goes_through_its_life() {
    with_server(|maps| async move {
        let places = maps.places;

        let mut req = places.create_request();
        req.get().set_geojson(point("Berlin", 13.4, 52.5).as_str());
        let answer = req.send().promise.await.unwrap();
        let created = answer.get().unwrap().get_place().unwrap();
        let envelope = envelope_of(created);
        assert_eq!(envelope.object_type, "Place");
        assert_eq!(envelope.name, "Berlin");
        assert_eq!(envelope.context.as_deref(), Some("places"));
        assert_eq!(envelope.attributed_to.as_deref(), Some(common::ME));
        let id = envelope.id;
        let iri = format!("https://local.invalid/places/{id}");
        assert_eq!(envelope.iri.as_deref(), Some(iri.as_str()));

        let mut get = places.get_request();
        write_object_id(get.get().init_id(), id);
        let got = get.send().promise.await.unwrap();
        let got = got.get().unwrap();
        assert!(got.get_found());
        let text = got.get_place().unwrap().get_geojson().unwrap();
        let feature: Value = serde_json::from_str(text.to_str().unwrap()).unwrap();
        assert_eq!(feature["geometry"]["coordinates"][0], 13.4);

        let mut update = places.update_request();
        write_object_id(update.get().init_id(), id);
        let moved = point("Berlin Mitte", 13.39, 52.52);
        update.get().set_geojson(moved.as_str());
        let updated = update.send().promise.await.unwrap();
        let updated = updated.get().unwrap().get_place().unwrap();
        let after = envelope_of(updated);
        assert_eq!((after.id, after.name.as_str()), (id, "Berlin Mitte"));
        assert_eq!(after.published, envelope.published);
        assert!(after.updated >= envelope.updated);
        let text = updated.get_geojson().unwrap().to_str().unwrap();
        assert!(text.contains("13.39"), "{text}");

        delete(&places, id).await;
        let mut get = places.get_request();
        write_object_id(get.get().init_id(), id);
        let gone = get.send().promise.await.unwrap();
        assert!(!gone.get().unwrap().get_found());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_place_without_a_position_is_a_place() {
    with_server(|maps| async move {
        let text = r#"{"type":"Feature","geometry":null,"properties":{"title":"Nowhere"}}"#;
        let id = common::create_text(&maps.places, text).await;
        let mut get = maps.places.get_request();
        write_object_id(get.get().init_id(), id);
        let got = get.send().promise.await.unwrap();
        let place = got.get().unwrap().get_place().unwrap();
        assert_eq!(envelope_of(place).name, "Nowhere");
        let (ids, _) = common::list_all(&maps.places).await;
        assert_eq!(ids, [id]);
        let other = create(&maps.places, "Somewhere", 1.0, 1.0).await;
        assert_ne!(other, id);
    })
    .await;
}

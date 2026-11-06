//! What could not come back is refused before it is stored: it would block paging forever.

mod common;

use common::{TOO_LARGE, create_text, envelope_of, is, list_all, refused, with_server};
use fs_interfaces::rpc::{MAX_INTAKE_BYTES, write_object_id};
use serde_json::Value;

const ENVELOPE_ROOM: usize = 4096;

fn named(filler: usize) -> String {
    format!(
        r#"{{"type":"Feature","geometry":null,"properties":{{"name":"{}"}}}}"#,
        "x".repeat(filler)
    )
}

#[tokio::test(flavor = "current_thread")]
async fn the_limit_holds_exactly_in_the_worst_case() {
    with_server(|maps| async move {
        let p = maps.places;
        // All of it is the name, which the answer carries twice.
        let largest = MAX_INTAKE_BYTES - ENVELOPE_ROOM - named(0).len();

        let mut over = p.create_request();
        over.get().set_geojson(named(largest + 1).as_str());
        assert_eq!(refused(over.send().promise.await), is(TOO_LARGE, 413));
        assert!(list_all(&p).await.0.is_empty());

        let id = create_text(&p, &named(largest)).await;
        let mut get = p.get_request();
        write_object_id(get.get().init_id(), id);
        let got = get.send().promise.await.unwrap();
        let place = got.get().unwrap().get_place().unwrap();
        assert_eq!(envelope_of(place).name.len(), largest);
        let text = place.get_geojson().unwrap().to_str().unwrap();
        let feature: Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            feature["properties"]["name"].as_str().unwrap().len(),
            largest
        );
        assert_eq!(list_all(&p).await.0, [id]);

        let mut update = p.update_request();
        write_object_id(update.get().init_id(), id);
        update.get().set_geojson(named(largest + 1).as_str());
        assert_eq!(refused(update.send().promise.await), is(TOO_LARGE, 413));
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_place_is_measured_as_it_would_be_stored() {
    with_server(|maps| async move {
        let p = maps.places;
        // Fits as it arrives; every number more than doubles when written back.
        let numbers = vec!["1E5"; MAX_INTAKE_BYTES / 6].join(",");
        let text = format!(
            r#"{{"type":"Feature","geometry":null,"properties":{{"name":"n","note":[{numbers}]}}}}"#
        );
        assert!(text.len() < MAX_INTAKE_BYTES);
        let mut req = p.create_request();
        req.get().set_geojson(text.as_str());
        assert_eq!(refused(req.send().promise.await), is(TOO_LARGE, 413));
        assert!(list_all(&p).await.0.is_empty());
    })
    .await;
}

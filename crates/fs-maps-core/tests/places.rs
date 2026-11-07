//! Places: GeoJSON <-> store adapter, validation, and the repository over a real `TursoStore`.

use fs_maps_core::places::{
    PlaceRepository, feature_bbox, feature_from_place, place_from_feature, place_geo_uri,
};
use fs_objects::TursoStore;
use serde_json::{Value, json};

fn berlin() -> Value {
    json!({"type":"Feature","id":"node/1","geometry":{"type":"Point","coordinates":[13.4050,52.5200]},
           "properties":{"name":"Berlin","population":3700000}})
}

#[test]
fn point_to_place_and_back() {
    let o = place_from_feature(&berlin(), Some("https://example.org/actors/a")).unwrap();
    assert_eq!(o.envelope.object_type, "Place");
    assert_eq!(o.envelope.name, "Berlin");
    assert_eq!(
        o.envelope.attributed_to.as_deref(),
        Some("https://example.org/actors/a")
    );
    assert_eq!(o.index["geometry_type"], ["Point"]);
    assert_eq!(o.index["bbox"], ["13.405,52.52,13.405,52.52"]);
    assert_eq!(place_geo_uri(&o).unwrap().to_string(), "geo:52.52,13.405");
    assert_eq!(feature_from_place(&o).unwrap(), berlin());
}

#[test]
fn polygon_bbox_and_name_fallbacks() {
    let f = json!({"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[2,0],[2,1],[0,1],[0,0]]]},
                   "properties":{"name:de":"Feld","name:en":"Field"}});
    let o = place_from_feature(&f, None).unwrap();
    assert_eq!(o.envelope.name, "Field");
    assert_eq!(o.index["bbox"], ["0,0,2,1"]);
    assert!(place_geo_uri(&o).is_none());
    let f = json!({"type":"Feature","geometry":null,"properties":{"title":"Somewhere"}});
    let o = place_from_feature(&f, None).unwrap();
    assert_eq!(o.envelope.name, "Somewhere");
    assert!(!o.index.contains_key("bbox"));
}

#[test]
fn rejects_bad_features() {
    for f in [
        json!({"type":"FeatureCollection","features":[]}),
        json!({"type":"Feature","geometry":null,"properties":{}}), // no name
        json!({"type":"Feature","geometry":null,"properties":{"name":"  "}}),
        json!({"type":"Feature","properties":{"name":"x"}}), // no geometry member
        json!({"type":"Feature","geometry":{"type":"Point","coordinates":[200,0]},"properties":{"name":"x"}}),
        json!({"type":"Feature","geometry":{"type":"Point","coordinates":[1]},"properties":{"name":"x"}}),
        json!({"type":"Feature","geometry":{"type":"Circle","coordinates":[1,2]},"properties":{"name":"x"}}),
        json!({"type":"Feature","geometry":{"type":"Point","coordinates":["a",2]},"properties":{"name":"x"}}),
        json!({"type":"Feature","id":[1],"geometry":null,"properties":{"name":"x"}}),
    ] {
        assert!(place_from_feature(&f, None).is_err(), "{f}");
    }
}

#[test]
fn geometry_collection_bbox() {
    let f = json!({"type":"Feature","properties":{"name":"g"},"geometry":{"type":"GeometryCollection","geometries":[
        {"type":"Point","coordinates":[1,2]},{"type":"LineString","coordinates":[[-3,4],[5,-6]]}]}});
    let b = feature_bbox(&f).unwrap().unwrap();
    assert_eq!(
        (b.min_lon, b.min_lat, b.max_lon, b.max_lat),
        (-3.0, -6.0, 5.0, 4.0)
    );
}

#[tokio::test]
async fn through_the_real_store() {
    let store = TursoStore::open_in_memory().await.unwrap();
    let repo = PlaceRepository::new(&store, "https://example.org/actors/a");
    let id = repo.save(&berlin()).await.unwrap();
    assert_eq!(repo.load(id).await.unwrap().unwrap(), berlin());
    assert!(repo.save(&json!({"type":"Feature"})).await.is_err());
}

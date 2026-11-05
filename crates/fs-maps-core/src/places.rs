//! Places: GeoJSON (RFC 7946) `Feature` <-> `fs-store` object of AS2 type `Place`.
//!
//! The stored `content` is the Feature itself (`application/geo+json`, ADR 0020/0032); the
//! envelope `name` comes from `properties.name` (fallbacks below). Index fields filled for
//! queries: `geometry_type`, `bbox` (`minlon,minlat,maxlon,maxlat`) and, for points, `geo` (an
//! RFC 5870 URI). A GeoJSON `id`, if present, is kept inside the Feature.

use fs_store::{Envelope, StoredObject};
use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::geouri::GeoUri;
use crate::tile::BBox;

pub const GEOJSON_MEDIA_TYPE: &str = "application/geo+json";
pub const PLACE_TYPE: &str = "Place";

fn gj<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::GeoJson(msg.into()))
}

/// Name for the envelope: `name`, then `name:en`, then any `name:*`, then `title`.
pub fn feature_name(props: &Map<String, Value>) -> Option<String> {
    let s = |k: &str| {
        props
            .get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    s("name")
        .or_else(|| s("name:en"))
        .or_else(|| {
            let mut keys: Vec<_> = props.keys().filter(|k| k.starts_with("name:")).collect();
            keys.sort();
            keys.into_iter().find_map(|k| s(k))
        })
        .or_else(|| s("title"))
}

fn position(v: &Value) -> Result<(f64, f64)> {
    let a = v.as_array().filter(|a| (2..=3).contains(&a.len()));
    let Some(a) = a else { return gj("position must be [lon, lat(, alt)]") };
    let n = |i: usize| a[i].as_f64().filter(|x| x.is_finite());
    let (Some(lon), Some(lat)) = (n(0), n(1)) else { return gj("position is not numeric") };
    if a.len() == 3 && n(2).is_none() {
        return gj("altitude is not numeric");
    }
    if !(-180.0..=180.0).contains(&lon) || !(-90.0..=90.0).contains(&lat) {
        return gj(format!("position {lon},{lat} outside WGS84 range"));
    }
    Ok((lon, lat))
}

fn collect(coords: &Value, depth: usize, out: &mut Vec<(f64, f64)>) -> Result<()> {
    if depth == 0 {
        out.push(position(coords)?);
        return Ok(());
    }
    let Some(arr) = coords.as_array() else { return gj("coordinates must be an array") };
    arr.iter().try_for_each(|c| collect(c, depth - 1, out))
}

/// Validates a geometry and returns all its positions.
fn geometry_positions(g: &Value) -> Result<(String, Vec<(f64, f64)>)> {
    let Some(t) = g.get("type").and_then(Value::as_str) else { return gj("geometry without type") };
    let mut pts = Vec::new();
    if t == "GeometryCollection" {
        let Some(gs) = g.get("geometries").and_then(Value::as_array) else {
            return gj("GeometryCollection without geometries");
        };
        for sub in gs {
            pts.extend(geometry_positions(sub)?.1);
        }
        return Ok((t.to_owned(), pts));
    }
    let depth = match t {
        "Point" => 0,
        "MultiPoint" | "LineString" => 1,
        "MultiLineString" | "Polygon" => 2,
        "MultiPolygon" => 3,
        other => return gj(format!("unknown geometry type {other:?}")),
    };
    let Some(c) = g.get("coordinates") else { return gj("geometry without coordinates") };
    collect(c, depth, &mut pts)?;
    Ok((t.to_owned(), pts))
}

/// Bounding box of a feature's geometry, `None` for a null geometry. Antimeridian-crossing
/// geometries yield a box spanning the world's longitude (RFC 7946 section 5 allows that).
pub fn feature_bbox(feature: &Value) -> Result<Option<BBox>> {
    let Some(g) = feature.get("geometry").filter(|g| !g.is_null()) else { return Ok(None) };
    let (_, pts) = geometry_positions(g)?;
    if pts.is_empty() {
        return Ok(None);
    }
    let f = |sel: fn(&(f64, f64)) -> f64, init: f64, op: fn(f64, f64) -> f64| pts.iter().map(sel).fold(init, op);
    Ok(Some(BBox::new(
        f(|p| p.0, f64::INFINITY, f64::min),
        f(|p| p.1, f64::INFINITY, f64::min),
        f(|p| p.0, f64::NEG_INFINITY, f64::max),
        f(|p| p.1, f64::NEG_INFINITY, f64::max),
    )?))
}

/// GeoJSON `Feature` -> store object (AS2 `Place`).
pub fn place_from_feature(feature: &Value, attributed_to: Option<&str>) -> Result<StoredObject> {
    if feature.get("type").and_then(Value::as_str) != Some("Feature") {
        return gj("type must be \"Feature\"");
    }
    let props = match feature.get("properties") {
        Some(Value::Object(p)) => p.clone(),
        Some(Value::Null) | None => Map::new(),
        Some(_) => return gj("properties must be an object or null"),
    };
    match feature.get("id") {
        None | Some(Value::String(_) | Value::Number(_)) => {}
        Some(_) => return gj("id must be a string or number"),
    }
    let Some(name) = feature_name(&props) else {
        return gj("feature has no usable name (properties.name / name:* / title)");
    };
    let mut index = fs_store::IndexFields::new();
    if let Some(g) = feature.get("geometry").filter(|g| !g.is_null()) {
        let (t, pts) = geometry_positions(g)?;
        index.insert("geometry_type".into(), vec![t.clone()]);
        if let Some(b) = feature_bbox(feature)? {
            index.insert(
                "bbox".into(),
                vec![format!("{},{},{},{}", b.min_lon, b.min_lat, b.max_lon, b.max_lat)],
            );
        }
        if t == "Point" {
            let (lon, lat) = pts[0];
            index.insert("geo".into(), vec![GeoUri::new(lat, lon)?.to_string()]);
        }
    } else if !feature.as_object().is_some_and(|o| o.contains_key("geometry")) {
        return gj("feature has no geometry member (use null for unlocated)");
    }
    let mut env = Envelope::new(PLACE_TYPE, name);
    env.attributed_to = attributed_to.map(str::to_owned);
    let mut obj = StoredObject::new(env, GEOJSON_MEDIA_TYPE, feature.clone());
    obj.index = index;
    Ok(obj)
}

/// Store object -> GeoJSON `Feature` (validated again; the envelope name is written back into
/// `properties.name` if the feature does not carry one).
pub fn feature_from_place(obj: &StoredObject) -> Result<Value> {
    if obj.envelope.object_type != PLACE_TYPE || obj.content_type != GEOJSON_MEDIA_TYPE {
        return gj(format!(
            "not a Place/{GEOJSON_MEDIA_TYPE} object ({} / {})",
            obj.envelope.object_type, obj.content_type
        ));
    }
    let mut f = obj.content.clone();
    if f.get("type").and_then(Value::as_str) != Some("Feature") {
        return gj("stored content is not a Feature");
    }
    if let Some(g) = f.get("geometry").filter(|g| !g.is_null()) {
        geometry_positions(g)?;
    }
    let o = f.as_object_mut().expect("checked: has type member");
    let props = o.entry("properties").or_insert_with(|| Value::Object(Map::new()));
    if props.is_null() {
        *props = Value::Object(Map::new());
    }
    if let Some(p) = props.as_object_mut().filter(|p| feature_name(p).is_none()) {
        p.insert("name".into(), Value::String(obj.envelope.name.clone()));
    }
    Ok(f)
}

/// `geo:` URI of a Point place (None for other geometries).
pub fn place_geo_uri(obj: &StoredObject) -> Option<GeoUri> {
    obj.index.get("geo")?.first()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_store::{Store, TursoStore};
    use serde_json::json;

    fn berlin() -> Value {
        json!({"type":"Feature","id":"node/1","geometry":{"type":"Point","coordinates":[13.4050,52.5200]},
               "properties":{"name":"Berlin","population":3700000}})
    }

    #[test]
    fn point_to_place_and_back() {
        let o = place_from_feature(&berlin(), Some("https://example.org/actors/a")).unwrap();
        assert_eq!(o.envelope.object_type, "Place");
        assert_eq!(o.envelope.name, "Berlin");
        assert_eq!(o.envelope.attributed_to.as_deref(), Some("https://example.org/actors/a"));
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
            json!({"type":"Feature","geometry":null,"properties":{}}),                          // no name
            json!({"type":"Feature","geometry":null,"properties":{"name":"  "}}),
            json!({"type":"Feature","properties":{"name":"x"}}),                                // no geometry member
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
        assert_eq!((b.min_lon, b.min_lat, b.max_lon, b.max_lat), (-3.0, -6.0, 5.0, 4.0));
    }

    #[tokio::test]
    async fn through_the_real_store() {
        let store = TursoStore::open_in_memory().await.unwrap();
        let o = place_from_feature(&berlin(), None).unwrap();
        let id = o.envelope.id;
        store.put(o, "https://example.org/actors/a").await.unwrap();
        let back = store.get(id).await.unwrap().unwrap();
        assert_eq!(back.envelope.name, "Berlin");
        assert_eq!(feature_from_place(&back).unwrap(), berlin());
    }
}

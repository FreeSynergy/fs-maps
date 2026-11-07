//! GeoJSON `Feature` <-> `fs-objects` object of AS2 type `Place`.
//!
//! Pattern: Adapter — the foreign GeoJSON model on one side, `StoredObject` on the other. The
//! stored `content` is the Feature itself (`application/geo+json`, ADR 0020/0032); the envelope
//! `name` comes from `properties.name` (fallbacks in `name.rs`). Index fields: `geometry_type`,
//! `bbox` (`minlon,minlat,maxlon,maxlat`), the sortable edge keys of `bounds` (for `within`),
//! every name in `name` (for `findByName`) and, for points, `geo` (an RFC 5870 URI).

use fs_objects::{Envelope, StoredObject};
use serde_json::{Map, Value};

use super::bounds::bbox_index;
use super::geometry::{feature_bbox, geometry_positions};
use super::name::{feature_name, feature_names};
use super::search::NAME_KEY;
use crate::error::{Error, Result};
use crate::geouri::GeoUri;

/// Media type of stored features.
pub const GEOJSON_MEDIA_TYPE: &str = "application/geo+json";
/// AS2 type of stored places.
pub const PLACE_TYPE: &str = "Place";

fn gj<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::GeoJson(msg.into()))
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
    let mut index = fs_objects::IndexFields::new();
    if let Some(g) = feature.get("geometry").filter(|g| !g.is_null()) {
        let (t, pts) = geometry_positions(g)?;
        index.insert("geometry_type".into(), vec![t.clone()]);
        if let Some(b) = feature_bbox(feature)? {
            index.insert(
                "bbox".into(),
                vec![format!(
                    "{},{},{},{}",
                    b.min_lon, b.min_lat, b.max_lon, b.max_lat
                )],
            );
            index.extend(bbox_index(&b));
        }
        if t == "Point" {
            let (lon, lat) = pts[0];
            index.insert("geo".into(), vec![GeoUri::new(lat, lon)?.to_string()]);
        }
    } else if !feature
        .as_object()
        .is_some_and(|o| o.contains_key("geometry"))
    {
        return gj("feature has no geometry member (use null for unlocated)");
    }
    index.insert(NAME_KEY.into(), feature_names(&props));
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
    let props = o
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()));
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

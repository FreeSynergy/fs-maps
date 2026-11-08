//! The GeoJSON feature of a place, and the form made of it: reading, and putting the changes back.
//!
//! Pattern: Adapter (pure functions) — no call to the server here; what the window shows and changes of
//! a feature, and what it leaves alone.

use fs_components_workbenches::SourceError;
use fs_components_workbenches::people::{DraftField, DraftGroup, PersonDraft};
use serde_json::{Value, json};

use crate::remote::Place;
use crate::words::field;

pub(crate) const NAME: &str = "name";
pub(crate) const LAT: &str = "latitude";
pub(crate) const LON: &str = "longitude";
pub(crate) const DESCRIPTION: &str = "description";

/// `(latitude, longitude)` of a feature that is a single point.
pub(crate) fn point(feature: &Value) -> Option<(f64, f64)> {
    let geometry = feature.get("geometry")?;
    if geometry.get("type")?.as_str()? != "Point" {
        return None;
    }
    let c = geometry.get("coordinates")?.as_array()?;
    Some((c.get(1)?.as_f64()?, c.first()?.as_f64()?))
}

pub(crate) fn shape(feature: &Value) -> String {
    feature
        .get("geometry")
        .and_then(|g| g.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("none")
        .to_string()
}

pub(crate) fn description(feature: &Value) -> String {
    feature
        .get("properties")
        .and_then(|p| p.get("description"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn position(feature: &Value) -> String {
    point(feature).map_or_else(
        || shape(feature),
        |(lat, lon)| format!("{lat:.5}, {lon:.5}"),
    )
}

pub(crate) fn text(n: f64) -> String {
    format!("{n}")
}

pub(crate) fn value_of<'a>(draft: &'a PersonDraft, key: &str) -> &'a str {
    draft
        .fields()
        .find(|f| f.key == key)
        .map_or("", |f| f.value.trim())
}

pub(crate) fn number(
    draft: &PersonDraft,
    key: &str,
    low: f64,
    high: f64,
    said: &str,
) -> Result<f64, SourceError> {
    value_of(draft, key)
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && (low..=high).contains(n))
        .ok_or_else(|| SourceError::failed(said))
}

pub(crate) fn form(name: &str, point: Option<(String, String)>, description: &str) -> PersonDraft {
    let one = |key: &str, label: &str, value: &str| {
        DraftGroup::new(key, label, false, vec![DraftField::new(key, label, value)])
    };
    let mut groups = vec![one(NAME, field::NAME, name)];
    if let Some((lat, lon)) = point {
        groups.push(one(LAT, field::LATITUDE, &lat));
        groups.push(one(LON, field::LONGITUDE, &lon));
    }
    groups.push(one(DESCRIPTION, field::DESCRIPTION, description));
    PersonDraft::new(None, groups)
}

/// Put what the person changed into the feature: the name, the description, a point's coordinates.
/// A field left as it was leaves what the feature holds alone (a description that is not a plain
/// text, the altitude, members of the geometry); a moved point drops the feature's `bbox`, which would
/// no longer contain it.
pub(crate) fn apply(
    feature: &mut Value,
    draft: &PersonDraft,
    is_point: bool,
) -> Result<(), SourceError> {
    let changed = |key: &str| draft.fields().any(|f| f.key == key && f.changed());
    let name = value_of(draft, NAME);
    if name.is_empty() {
        return Err(SourceError::failed(field::NO_NAME));
    }
    let moved = is_point && (changed(LAT) || changed(LON));
    let coordinates = if moved {
        Some((
            number(draft, LAT, -90.0, 90.0, field::BAD_LATITUDE)?,
            number(draft, LON, -180.0, 180.0, field::BAD_LONGITUDE)?,
        ))
    } else {
        None
    };
    let root = feature
        .as_object_mut()
        .ok_or_else(|| SourceError::failed("the place is not a GeoJSON object"))?;
    let properties = root
        .entry("properties")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| SourceError::failed("the properties of the place are not an object"))?;
    if changed(NAME) || !properties.contains_key("name") {
        properties.insert("name".into(), json!(name));
    }
    if changed(DESCRIPTION) {
        let described = value_of(draft, DESCRIPTION);
        if described.is_empty() {
            properties.remove("description");
        } else {
            properties.insert("description".into(), json!(described));
        }
    }
    if let Some((lat, lon)) = coordinates {
        // The altitude of the point, if it has one, stays; so do the other members of the geometry.
        let geometry = root
            .entry("geometry")
            .or_insert_with(|| json!({"type": "Point", "coordinates": []}));
        let altitude = geometry
            .get("coordinates")
            .and_then(Value::as_array)
            .and_then(|c| c.get(2))
            .cloned();
        let mut c = vec![json!(lon), json!(lat)];
        c.extend(altitude);
        if let Some(g) = geometry.as_object_mut() {
            g.insert("type".into(), json!("Point"));
            g.insert("coordinates".into(), Value::Array(c));
        }
        root.remove("bbox");
    }
    Ok(())
}

/// What the draft was made from: a conflict is a place that is not the same text now.
pub(crate) fn revision_of(place: &Place) -> String {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    place.name.hash(&mut hash);
    place.geojson.hash(&mut hash);
    format!("{:x}", hash.finish())
}
pub(crate) fn feature_of(place: &Place) -> Result<Value, SourceError> {
    serde_json::from_str(&place.geojson)
        .map_err(|e| SourceError::failed(format!("the place holds no readable GeoJSON ({e})")))
}

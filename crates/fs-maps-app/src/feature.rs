//! The `GeoJSON` feature of a place and the typed values of the description `Place`, both ways.
//!
//! Pattern: Adapter (pure functions) — no call to the server and nothing drawn here. The values are those of the
//! description (`name`, `position` as a `geo:` URI, `description`, `keywords`, `link`); the feature keeps every member
//! the description does not name (a polygon's shape, `bbox`, foreign properties). Changing is partial: a field that is
//! missing from the input is unchanged, an empty list clears it, values replace.

use fs_maps_core::geouri::GeoUri;
use fs_types::{RecordIssue, RecordProblem, TypeProblem};
use serde_json::{Map, Value, json};

use crate::remote::Place;
use fs_admin_frontend::Values;

fn issue(field: &str, problem: TypeProblem) -> RecordIssue {
    RecordIssue {
        field: field.to_owned(),
        index: 0,
        problem: RecordProblem::Value(problem),
    }
}

fn property<'a>(feature: &'a Value, key: &str) -> Option<&'a Value> {
    feature.get("properties")?.get(key)
}

fn point(feature: &Value) -> Option<GeoUri> {
    let geometry = feature.get("geometry")?;
    if geometry.get("type")?.as_str()? != "Point" {
        return None;
    }
    let c = geometry.get("coordinates")?.as_array()?;
    let mut uri = GeoUri::new(c.get(1)?.as_f64()?, c.first()?.as_f64()?).ok()?;
    uri.alt = c.get(2).and_then(Value::as_f64);
    Some(uri)
}

/// Where a place is on the map as `(longitude, latitude)`: its point, or the middle of the box around its shape.
pub(crate) fn anchor(feature: &Value) -> Option<(f64, f64)> {
    fn walk(v: &Value, bounds: &mut Option<(f64, f64, f64, f64)>) {
        match v.as_array() {
            Some(a) if a.len() >= 2 && a.iter().take(2).all(Value::is_number) => {
                if let (Some(x), Some(y)) = (a[0].as_f64(), a[1].as_f64()) {
                    let b = bounds.get_or_insert((x, y, x, y));
                    *b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
                }
            }
            Some(a) => a.iter().for_each(|i| walk(i, bounds)),
            None => {}
        }
    }
    let geometry = feature.get("geometry").filter(|g| !g.is_null())?;
    let mut bounds = None;
    match geometry.get("type")?.as_str()? {
        "GeometryCollection" => geometry
            .get("geometries")?
            .as_array()?
            .iter()
            .filter_map(|g| g.get("coordinates"))
            .for_each(|c| walk(c, &mut bounds)),
        _ => walk(geometry.get("coordinates")?, &mut bounds),
    }
    let (w, s, e, n) = bounds?;
    Some((f64::midpoint(w, e), f64::midpoint(s, n)))
}

/// The values of the description for a place; fields it has nothing for are left out.
pub(crate) fn values_of(place: &Place) -> Result<(Value, Values), String> {
    let feature: Value = serde_json::from_str(&place.geojson)
        .map_err(|e| format!("the place holds no readable GeoJSON ({e})"))?;
    let mut values = Values::new();
    let text = |key: &str| {
        property(&feature, key)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    values.insert(
        "name".into(),
        vec![text("name").unwrap_or_else(|| place.name.clone())],
    );
    if let Some(p) = point(&feature) {
        values.insert("position".into(), vec![p.to_string()]);
    }
    if let Some(d) = text("description").filter(|d| !d.is_empty()) {
        values.insert("description".into(), vec![d]);
    }
    if let Some(k) = property(&feature, "keywords").and_then(Value::as_array) {
        let words: Vec<String> = k
            .iter()
            .filter_map(|w| w.as_str().map(str::to_owned))
            .collect();
        if !words.is_empty() {
            values.insert("keywords".into(), words);
        }
    }
    if let Some(l) = text("url").filter(|l| !l.is_empty()) {
        values.insert("link".into(), vec![l]);
    }
    Ok((feature, values))
}

/// A feature for a new place.
pub(crate) fn new_feature(input: &Values) -> Result<Value, Vec<RecordIssue>> {
    let mut feature = json!({"type": "Feature", "geometry": null, "properties": {}});
    apply(&mut feature, input)?;
    Ok(feature)
}

fn first<'a>(input: &'a Values, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.first()).map(String::as_str)
}

/// Put the input into the feature: only the fields that are in `input`, and so partial (see the module).
pub(crate) fn apply(feature: &mut Value, input: &Values) -> Result<(), Vec<RecordIssue>> {
    let was_point = point(feature).is_some() || feature.get("geometry").is_none_or(Value::is_null);
    let geometry = match input.get("position").map(Vec::as_slice) {
        None => None,
        Some([]) => Some(Value::Null),
        Some([uri, ..]) => {
            if !was_point {
                return Err(vec![issue("position", TypeProblem::BadPlace)]);
            }
            let p =
                GeoUri::parse(uri).map_err(|_| vec![issue("position", TypeProblem::BadPlace)])?;
            let mut c = vec![json!(p.lon), json!(p.lat)];
            c.extend(p.alt.map(|a| json!(a)));
            Some(json!({"type": "Point", "coordinates": c}))
        }
    };
    let root = feature
        .as_object_mut()
        .ok_or_else(|| vec![issue("name", TypeProblem::Empty)])?;
    if let Some(g) = geometry {
        root.insert("geometry".into(), g);
        root.remove("bbox"); // a moved point is no longer inside it
    }
    let properties = root
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()));
    if !properties.is_object() {
        *properties = Value::Object(Map::new());
    }
    let Some(properties) = properties.as_object_mut() else {
        return Ok(());
    };
    let mut set = |key: &str, field: &str, list: bool| {
        let Some(values) = input.get(field) else {
            return;
        };
        let values: Vec<&String> = values.iter().filter(|v| !v.trim().is_empty()).collect();
        if values.is_empty() {
            properties.remove(key);
        } else if list {
            properties.insert(key.into(), json!(values));
        } else {
            properties.insert(key.into(), json!(values[0]));
        }
    };
    set("name", "name", false);
    set("description", "description", false);
    set("keywords", "keywords", true);
    set("url", "link", false);
    if first(input, "name").is_some_and(|n| n.trim().is_empty()) {
        return Err(vec![issue("name", TypeProblem::Empty)]);
    }
    Ok(())
}

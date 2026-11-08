//! `PlacesAdapter`: [`PlacesApi`] as the workbench's `PeopleSource`.
//!
//! Pattern: Adapter — the list-and-page workbench of `fs-components` (made for people, neutral in what
//! it shows) reads places as rows (name, position), a sheet (position, geo URI, description) and a
//! form (name, latitude, longitude, description). A place that is not a single point (a line, an area)
//! can be renamed and described, its shape is kept as it is. The server keeps the `GeoJSON` feature; the
//! window changes only `properties.name`, `properties.description` and a point's coordinates.

use std::sync::Arc;

use fs_components_workbenches::SourceError;
use fs_components_workbenches::people::{
    DraftField, DraftGroup, PeopleCapabilities, PeopleSource, PersonDraft, PersonField, PersonRow,
    PersonSection, PersonSheet,
};
use serde_json::{Value, json};

use crate::remote::{ApiError, Place, PlacesApi};
use crate::words::field;

const NAME: &str = "name";
const LAT: &str = "latitude";
const LON: &str = "longitude";
const DESCRIPTION: &str = "description";

/// The places of a program as the workbench's source.
pub struct PlacesAdapter {
    api: Arc<dyn PlacesApi>,
}

impl PlacesAdapter {
    /// An adapter over `api`.
    pub fn new(api: Arc<dyn PlacesApi>) -> Self {
        Self { api }
    }
}

fn reason(error: ApiError) -> SourceError {
    match error {
        ApiError::NotFound(m) => SourceError::not_found(m),
        ApiError::Unsupported(m) => SourceError::unsupported(m),
        ApiError::Failed(m) => SourceError::failed(m),
    }
}

/// `(latitude, longitude)` of a feature that is a single point.
fn point(feature: &Value) -> Option<(f64, f64)> {
    let geometry = feature.get("geometry")?;
    if geometry.get("type")?.as_str()? != "Point" {
        return None;
    }
    let c = geometry.get("coordinates")?.as_array()?;
    Some((c.get(1)?.as_f64()?, c.first()?.as_f64()?))
}

fn shape(feature: &Value) -> String {
    feature
        .get("geometry")
        .and_then(|g| g.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("none")
        .to_string()
}

fn description(feature: &Value) -> String {
    feature
        .get("properties")
        .and_then(|p| p.get("description"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn position(feature: &Value) -> String {
    point(feature).map_or_else(
        || shape(feature),
        |(lat, lon)| format!("{lat:.5}, {lon:.5}"),
    )
}

fn feature_of(place: &Place) -> Result<Value, SourceError> {
    serde_json::from_str(&place.geojson)
        .map_err(|e| SourceError::failed(format!("the place holds no readable GeoJSON ({e})")))
}

fn matches(place: &Place, term: &str) -> bool {
    let term = term.trim().to_lowercase();
    term.is_empty() || place.name.to_lowercase().contains(&term)
}

fn text(n: f64) -> String {
    format!("{n}")
}

fn value_of<'a>(draft: &'a PersonDraft, key: &str) -> &'a str {
    draft
        .fields()
        .find(|f| f.key == key)
        .map_or("", |f| f.value.trim())
}

fn number(
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

fn form(name: &str, point: Option<(String, String)>, description: &str) -> PersonDraft {
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

/// Put the name and description of `draft` into the feature, and the coordinates of a point.
fn apply(feature: &mut Value, draft: &PersonDraft, is_point: bool) -> Result<(), SourceError> {
    let name = value_of(draft, NAME);
    if name.is_empty() {
        return Err(SourceError::failed(field::NO_NAME));
    }
    let coordinates = if is_point {
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
    properties.insert("name".into(), json!(name));
    let described = value_of(draft, DESCRIPTION);
    if described.is_empty() {
        properties.remove("description");
    } else {
        properties.insert("description".into(), json!(described));
    }
    if let Some((lat, lon)) = coordinates {
        // The altitude of the point, if it has one, stays.
        let altitude = root
            .get("geometry")
            .and_then(|g| g.get("coordinates"))
            .and_then(Value::as_array)
            .and_then(|c| c.get(2))
            .cloned();
        let mut c = vec![json!(lon), json!(lat)];
        c.extend(altitude);
        root.insert(
            "geometry".into(),
            json!({"type": "Point", "coordinates": c}),
        );
    }
    Ok(())
}

impl PeopleSource for PlacesAdapter {
    fn capabilities(&self) -> PeopleCapabilities {
        let mut can = PeopleCapabilities::read_only();
        can.edit = true;
        can.create = true;
        can.delete = true;
        can
    }

    fn rows(&self, term: &str, offset: usize, limit: usize) -> Result<Vec<PersonRow>, SourceError> {
        Ok(self
            .api
            .list()
            .map_err(reason)?
            .into_iter()
            .filter(|p| matches(p, term))
            .skip(offset)
            .take(limit)
            .map(|p| {
                let at = serde_json::from_str::<Value>(&p.geojson)
                    .map(|f| position(&f))
                    .unwrap_or_default();
                PersonRow::new(p.id, vec![p.name, at])
            })
            .collect())
    }

    fn read(&self, key: &str) -> Result<PersonSheet, SourceError> {
        let place = self
            .api
            .get(key)
            .map_err(reason)?
            .ok_or_else(|| SourceError::not_found("This place no longer exists."))?;
        let feature = feature_of(&place)?;
        let mut sections = Vec::new();
        if let Some((lat, lon)) = point(&feature) {
            sections.push(PersonSection::new(
                field::LOCATION,
                vec![
                    PersonField::new(field::LATITUDE, text(lat)),
                    PersonField::new(field::LONGITUDE, text(lon)),
                    PersonField::new(field::GEO, format!("geo:{lat},{lon}")),
                ],
            ));
        } else {
            sections.push(PersonSection::new(
                field::LOCATION,
                vec![
                    PersonField::new(field::SHAPE, shape(&feature)),
                    PersonField::new("", field::NOT_A_POINT),
                ],
            ));
        }
        let described = description(&feature);
        if !described.is_empty() {
            sections.push(PersonSection::new(
                field::ABOUT,
                vec![PersonField::new(field::DESCRIPTION, described)],
            ));
        }
        Ok(PersonSheet::new(place.name, sections))
    }

    fn draft(&self, key: &str) -> Result<PersonDraft, SourceError> {
        let place = self
            .api
            .get(key)
            .map_err(reason)?
            .ok_or_else(|| SourceError::not_found("This place no longer exists."))?;
        let feature = feature_of(&place)?;
        let at = point(&feature).map(|(lat, lon)| (text(lat), text(lon)));
        Ok(form(&place.name, at, &description(&feature)))
    }

    fn blank(&self) -> Result<PersonDraft, SourceError> {
        Ok(form("", Some((String::new(), String::new())), ""))
    }

    fn save(&self, key: &str, draft: &PersonDraft) -> Result<(), SourceError> {
        let place = self
            .api
            .get(key)
            .map_err(reason)?
            .ok_or_else(|| SourceError::not_found("This place no longer exists."))?;
        let mut feature = feature_of(&place)?;
        let is_point = point(&feature).is_some();
        apply(&mut feature, draft, is_point)?;
        self.api
            .update(key, &feature.to_string())
            .map(|_| ())
            .map_err(reason)
    }

    fn create(&self, draft: &PersonDraft) -> Result<String, SourceError> {
        let mut feature = json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [0.0, 0.0]},
            "properties": {},
        });
        apply(&mut feature, draft, true)?;
        self.api
            .create(&feature.to_string())
            .map(|p| p.id)
            .map_err(reason)
    }

    fn delete(&self, key: &str) -> Result<Option<String>, SourceError> {
        self.api.delete(key).map(|()| None).map_err(reason)
    }
}

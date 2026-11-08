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
    PeopleCapabilities, PeopleSource, PersonDraft, PersonField, PersonRow, PersonSection,
    PersonSheet,
};
use serde_json::{Value, json};

use crate::feature::{
    apply, description, feature_of, form, point, position, revision_of, shape, text,
};
use crate::remote::{ApiError, Place, PlacesApi};
use crate::words::field;

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

fn matches(place: &Place, term: &str) -> bool {
    let term = term.trim().to_lowercase();
    term.is_empty() || place.name.to_lowercase().contains(&term)
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
        let mut draft = form(&place.name, at, &description(&feature));
        draft.revision = Some(revision_of(&place));
        Ok(draft)
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
        if draft.revision.as_deref() != Some(revision_of(&place).as_str()) {
            return Err(SourceError::conflict(
                "Someone else changed this place since you started.",
            ));
        }
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

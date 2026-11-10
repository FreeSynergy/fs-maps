//! `PlacesBackend`: the records and commands of one data type of the description, over the maps server.
//!
//! Pattern: Repository (trait `Backend` of the admin frontend, trait first) + Observer — it reads and writes through
//! the trait [`PlacesApi`] (Cap'n Proto behind it); what the program's feature table says is not carried out
//! (`describe::Support`) is answered with `Unavailable("unsupported:<name>")` and nothing is sent. Listeners are told
//! after a write that went through (the map redraws its markers).

use std::sync::{Arc, Mutex, PoisonError};

use fs_admin_frontend::{Backend, BackendError, Outcome, Record, Values};
use fs_maps_core::describe::Support;
use fs_maps_core::geouri::GeoUri;
use fs_types::{RecordIssue, RecordProblem, TypeProblem};

use crate::feature::{apply, new_feature, values_of};
use crate::remote::{ApiError, Place, PlacesApi};

/// Told after a write that went through.
pub type Listener = Arc<dyn Fn() + Send + Sync>;

/// The records of one data type.
pub struct PlacesBackend {
    api: Arc<dyn PlacesApi>,
    data_type: String,
    listeners: Mutex<Vec<Listener>>,
}

fn error(e: ApiError) -> BackendError {
    match e {
        ApiError::NotFound(_) => BackendError::NotFound,
        ApiError::Unsupported(m) | ApiError::Failed(m) => BackendError::Unavailable(m),
    }
}

fn bad(field: &str) -> BackendError {
    BackendError::Rejected(vec![RecordIssue {
        field: field.to_owned(),
        index: 0,
        problem: RecordProblem::Value(TypeProblem::BadPlace),
    }])
}

/// `(latitude, longitude)` of a corner as typed. Not `GeoUri::parse`: that normalises the longitude of a pole and
/// -180 to 180, right for a point but wrong for the edge of a box.
fn corner(query: &Values, key: &str) -> Result<Option<(f64, f64)>, BackendError> {
    let Some(text) = query.get(key).and_then(|v| v.first()) else {
        return Ok(None);
    };
    GeoUri::parse(text).map_err(|_| bad(key))?;
    let coordinates = text
        .trim()
        .get(4..)
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default();
    let mut parts = coordinates.split(',').map(|p| p.trim().parse::<f64>());
    match (parts.next(), parts.next()) {
        (Some(Ok(lat)), Some(Ok(lon))) if lat.abs() <= 90.0 && lon.abs() <= 180.0 => {
            Ok(Some((lat, lon)))
        }
        _ => Err(bad(key)),
    }
}

impl PlacesBackend {
    /// The records of the data type `data_type` (`Place`, `MapLayer` …) over `api`.
    pub fn new(api: Arc<dyn PlacesApi>, data_type: impl Into<String>) -> Self {
        Self {
            api,
            data_type: data_type.into(),
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// Ask to be told after every write that went through.
    pub fn subscribe(&self, listener: Listener) {
        self.listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(listener);
    }

    fn told(&self) {
        let all = self
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for l in &all {
            l();
        }
    }

    fn record(place: &Place) -> Result<Record, BackendError> {
        let (_, values) = values_of(place).map_err(BackendError::Unavailable)?;
        Ok(Record {
            id: place.id.clone(),
            values,
        })
    }

    fn fetch(&self, id: &str) -> Result<Place, BackendError> {
        self.api
            .get(id)
            .map_err(error)?
            .ok_or(BackendError::NotFound)
    }

    fn id_of(input: &Values) -> Result<&str, BackendError> {
        input
            .get("id")
            .and_then(|v| v.first())
            .map(String::as_str)
            .ok_or(BackendError::NotFound)
    }

    fn write(&self, name: &str, input: &Values) -> Result<Outcome, BackendError> {
        match name {
            "create" => {
                let feature = new_feature(input).map_err(BackendError::Rejected)?;
                let made = self.api.create(&feature.to_string()).map_err(error)?;
                self.told();
                Ok(Outcome {
                    id: Some(made.id.clone()),
                    values: Values::from([("id".to_owned(), vec![made.id])]),
                })
            }
            "change" => {
                let id = Self::id_of(input)?;
                let place = self.fetch(id)?;
                let mut feature = values_of(&place).map_err(BackendError::Unavailable)?.0;
                apply(&mut feature, input).map_err(BackendError::Rejected)?;
                let saved = self.api.update(id, &feature.to_string()).map_err(error)?;
                self.told();
                Ok(Outcome {
                    id: Some(saved.id),
                    values: Values::new(),
                })
            }
            "delete" => {
                let id = Self::id_of(input)?;
                self.api.delete(id).map_err(error)?;
                self.told();
                Ok(Outcome::default())
            }
            _ => Err(BackendError::NotFound),
        }
    }
}

impl Backend for PlacesBackend {
    fn list(&self, query: &Values) -> Result<Vec<Record>, BackendError> {
        // The program has records of one type; the others are described and have none yet.
        if self.data_type != "Place" {
            return Ok(Vec::new());
        }
        let places = match (corner(query, "southwest")?, corner(query, "northeast")?) {
            (Some(sw), Some(ne)) if sw.0 > ne.0 => return Err(bad("northeast")),
            (Some(sw), Some(ne)) => self.api.within((sw.1, sw.0, ne.1, ne.0)),
            _ => match query.get("term").and_then(|v| v.first()).map(|t| t.trim()) {
                Some(term) if !term.is_empty() => self.api.find_by_name(term),
                _ => self.api.list(),
            },
        }
        .map_err(error)?;
        places.iter().map(Self::record).collect()
    }

    fn run(&self, name: &str, input: &Values) -> Result<Outcome, BackendError> {
        if !Support::is(&self.data_type, name) {
            return Err(BackendError::Unavailable(format!("unsupported:{name}")));
        }
        match name {
            "find" | "within" => Ok(Outcome::default()),
            other => self.write(other, input),
        }
    }
}

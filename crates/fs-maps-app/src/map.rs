//! What the map component needs from this program: the markers and the tiles. Nothing is drawn here.
//!
//! Pattern: Adapter — the places of the server as `MapData` for the component Karte, the tiles of the server's
//! `TileSource` as `TileBytes`. The markers are kept (a draw must be quick) and read again after a write that went
//! through ([`PlacesBackend::subscribe`]).

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use fs_components_map::{MapData, MapPosition, MapSurface, Marker, TileBytes, TileId};

use crate::feature::{anchor, values_of};
use crate::remote::PlacesApi;

/// The places as markers; a marker is chosen through the `pick` the window sets.
pub struct PlaceMarkers {
    api: Arc<dyn PlacesApi>,
    markers: Mutex<Vec<Marker>>,
    pick: OnceLock<Box<dyn Fn(&str) + Send + Sync>>,
}

impl PlaceMarkers {
    /// The markers of the places `api` has now.
    pub fn new(api: Arc<dyn PlacesApi>) -> Arc<Self> {
        let me = Arc::new(Self {
            api,
            markers: Mutex::default(),
            pick: OnceLock::new(),
        });
        me.reload();
        me
    }

    /// Read the places again. A server that does not answer leaves the markers as they are.
    pub fn reload(&self) {
        let Ok(places) = self.api.list() else { return };
        let markers = places
            .iter()
            .filter_map(|p| {
                let (feature, values) = values_of(p).ok()?;
                let (lon, lat) = anchor(&feature)?;
                let name = values
                    .get("name")
                    .and_then(|v| v.first())
                    .cloned()
                    .unwrap_or_default();
                Some(Marker::new(p.id.clone(), lon, lat).labelled(name))
            })
            .collect();
        *self.markers.lock().unwrap_or_else(PoisonError::into_inner) = markers;
    }

    /// What to do when a marker is chosen (the first call counts).
    pub fn on_pick(&self, pick: impl Fn(&str) + Send + Sync + 'static) {
        let _ = self.pick.set(Box::new(pick));
    }

    /// The view that shows all markers whole in `width` x `height`, or the world when there are none.
    pub fn start(&self, width: f64, height: f64) -> MapPosition {
        let markers = self.markers.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut w, mut s, mut e, mut n) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for m in markers.iter() {
            (w, s, e, n) = (w.min(m.lon), s.min(m.lat), e.max(m.lon), n.max(m.lat));
        }
        if markers.is_empty() {
            MapPosition::world()
        } else {
            MapPosition::fitting((w, s, e, n), width, height, 12.0)
        }
    }
}

impl MapData for PlaceMarkers {
    fn markers(&self) -> Vec<Marker> {
        self.markers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn picked(&self, key: &str) {
        if let Some(pick) = self.pick.get() {
            pick(key);
        }
    }
}

/// Redraw `surface` after every write that went through.
pub fn follow(
    markers: &Arc<PlaceMarkers>,
    surface: &MapSurface,
) -> impl Fn() + Send + Sync + 'static {
    let (markers, surface) = (Arc::clone(markers), surface.clone());
    move || {
        markers.reload();
        surface.refresh();
    }
}

/// The tiles of the server.
pub struct ServerTiles(pub Arc<dyn PlacesApi>);

impl TileBytes for ServerTiles {
    fn tile(&self, id: TileId) -> Result<Option<Vec<u8>>, String> {
        self.0.tile(id.z, id.x, id.y).map_err(|e| format!("{e:?}"))
    }
}

//! Places, boxes and pages on the wire.
//!
//! Pattern: Adapter — `StoredObject` and `Area` between Rust and `places.capnp`. The `GeoJSON` stays
//! authoritative for the content; the envelope is the routing summary. Whatever comes in malformed
//! is refused, and nothing is let in that could not come back: it would block paging forever.

use fs_interfaces::rpc::{
    DEFAULT_PAGE_SIZE, Page, check_intake, envelope_weight, invalid_request, write_envelope,
};
use fs_interfaces::{common_capnp, places_capnp};
use fs_maps_core::Area;
use fs_maps_core::places::feature_from_place;
use fs_objects::StoredObject;
use serde_json::Value;

use crate::errors::{rpc_error, stored_data};

/// The version `supportReport` names.
pub(crate) const CONTRACT_VERSION: &str = "places 0.1 (draft)";

/// Room for what a place adds to its `GeoJSON` besides its name: ids, times, IRIs, framing.
const ENVELOPE_ROOM: usize = 4096;

/// What a place weighs in a message.
pub(crate) fn weight(place: &StoredObject) -> usize {
    place.content.to_string().len() + envelope_weight(&place.envelope)
}

/// Write one place, whose size the caller checked. What [`read_feature`] let in always fits; a stored
/// place that no longer reads as one is the store's problem, not the caller's.
pub(crate) fn write_place(
    mut out: places_capnp::place::Builder<'_>,
    place: &StoredObject,
) -> capnp::Result<()> {
    let geojson = feature_from_place(place)
        .map_err(|e| stored_data(&e))?
        .to_string();
    write_envelope(out.reborrow().init_envelope(), &place.envelope);
    out.set_geojson(geojson.as_str());
    Ok(())
}

/// Read the `GeoJSON` text of a request; the core validates the feature. Measured again as it will
/// be stored and answered, because that can be more than arrived: `1E5` comes back as `100000.0`.
pub(crate) fn read_feature(text: capnp::text::Reader<'_>) -> capnp::Result<Value> {
    let text = text.to_str().map_err(|e| invalid_request(e.to_string()))?;
    check_intake(text.len())?;
    let feature: Value = serde_json::from_str(text).map_err(|e| invalid_request(e.to_string()))?;
    check_intake(feature.to_string().len() + ENVELOPE_ROOM)?;
    Ok(feature)
}

/// Read the search term. An empty one would match every place, which `list` is for.
pub(crate) fn read_term(text: capnp::text::Reader<'_>) -> capnp::Result<&str> {
    let term = text.to_str().map_err(|e| invalid_request(e.to_string()))?;
    let term = term.trim();
    if term.is_empty() {
        return Err(invalid_request("term is empty"));
    }
    Ok(term)
}

/// Read the search box. An absent box would read as a point at 0,0 and find nothing, silently.
pub(crate) fn read_area(p: places_capnp::places::within_params::Reader<'_>) -> capnp::Result<Area> {
    if !p.has_box() {
        return Err(invalid_request("box is missing"));
    }
    let b = p.get_box()?;
    Area::new(b.get_west(), b.get_south(), b.get_east(), b.get_north()).map_err(rpc_error)
}

/// Cut the page at `cursor` and write it.
pub(crate) fn write_page(
    mut out: places_capnp::place_page::Builder<'_>,
    all: Vec<StoredObject>,
    cursor: capnp::Result<common_capnp::cursor::Reader<'_>>,
) -> capnp::Result<()> {
    let opaque = cursor?.get_opaque()?;
    let page = Page::of_weighted(all, opaque, DEFAULT_PAGE_SIZE, weight)?;
    let length = u32::try_from(page.items.len())
        .map_err(|_| capnp::Error::failed("list too long".into()))?;
    let mut items = out.reborrow().init_items(length);
    for (i, place) in (0u32..).zip(&page.items) {
        write_place(items.reborrow().get(i), place)?;
    }
    out.init_next().set_opaque(&page.next);
    Ok(())
}

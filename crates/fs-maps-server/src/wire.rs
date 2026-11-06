//! Places, boxes, ids and pages on the wire.
//!
//! Pattern: Adapter — `StoredObject`, `Area` and ids between Rust and `places.capnp`. The GeoJSON
//! stays authoritative for the content; the envelope is the routing summary. Whatever comes in
//! malformed is a typed `invalid`, and nothing goes out that would not fit one message.

use fs_interfaces::rpc::{DEFAULT_PAGE_SIZE, MAX_PAGE_BYTES, Page, read_object_id, write_envelope};
use fs_interfaces::{common_capnp, places_capnp};
use fs_maps_core::Area;
use fs_maps_core::places::feature_from_place;
use fs_store::{ObjectId, StoredObject};
use serde_json::Value;

use crate::errors::{invalid, rpc_error, too_large};

/// The version `supportReport` names.
pub(crate) const CONTRACT_VERSION: &str = "places 0.1 (draft)";

/// Room for what a place adds to its GeoJSON: envelope ids, times, IRIs, framing.
const ENVELOPE_ROOM: usize = 4096;

/// What a place weighs in a message. The name comes twice: envelope, and `properties` if missing.
fn weight(place: &StoredObject) -> usize {
    place.content.to_string().len() + 2 * place.envelope.name.len() + ENVELOPE_ROOM
}

/// Write one place.
pub(crate) fn write_place(
    mut out: places_capnp::place::Builder<'_>,
    place: &StoredObject,
) -> capnp::Result<()> {
    let geojson = feature_from_place(place).map_err(rpc_error)?.to_string();
    if geojson.len() + place.envelope.name.len() + ENVELOPE_ROOM > MAX_PAGE_BYTES {
        return Err(too_large(format!(
            "place {} does not fit one message",
            place.envelope.id
        )));
    }
    write_envelope(out.reborrow().init_envelope(), &place.envelope);
    out.set_geojson(geojson.as_str());
    Ok(())
}

/// Read the GeoJSON text of a request; the core validates the feature. What could not be answered
/// once stored is refused here.
pub(crate) fn read_feature(text: capnp::text::Reader<'_>) -> capnp::Result<Value> {
    let text = text.to_str().map_err(|e| invalid(e.to_string()))?;
    if text.len() + 2 * ENVELOPE_ROOM > MAX_PAGE_BYTES {
        return Err(too_large(format!(
            "{} bytes of GeoJSON, a place may have up to {MAX_PAGE_BYTES}",
            text.len()
        )));
    }
    serde_json::from_str(text).map_err(|e| invalid(e.to_string()))
}

/// Read an id: a missing or malformed one is the request's fault.
pub(crate) fn read_id(r: common_capnp::uuid::Reader<'_>) -> capnp::Result<ObjectId> {
    read_object_id(r).map_err(|e| invalid(e.extra))
}

/// Read the search box. An absent box would read as a point at 0,0 and find nothing, silently.
pub(crate) fn read_area(p: places_capnp::places::within_params::Reader<'_>) -> capnp::Result<Area> {
    if !p.has_box() {
        return Err(invalid("box is missing"));
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
    let page = Page::of_weighted(all, opaque, DEFAULT_PAGE_SIZE, weight)
        .map_err(|e| invalid(e.extra))?;
    let length = u32::try_from(page.items.len()).map_err(|_| too_large("list too long"))?;
    let mut items = out.reborrow().init_items(length);
    for (i, place) in (0u32..).zip(&page.items) {
        write_place(items.reborrow().get(i), place)?;
    }
    out.init_next().set_opaque(&page.next);
    Ok(())
}

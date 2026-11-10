//! Reading the wire: pages, places and problems of the maps contract.
//!
//! Pattern: Adapter (pure functions) — generated Cap'n Proto readers in, the plain values of [`crate::api`] out.

use std::future::Future;

use fs_interfaces::places_capnp::{place, place_page};
use fs_interfaces::rpc::{Problem, read_envelope};
use uuid::Uuid;

use crate::api::{ApiError, Place};

/// All pages of a paged answer: `page` asks for the page at a cursor.
pub(crate) async fn collect<F, Fut>(mut page: F) -> capnp::Result<Vec<Place>>
where
    F: FnMut(&[u8]) -> Fut,
    Fut: Future<Output = capnp::Result<(Vec<Place>, Vec<u8>)>>,
{
    let (mut found, mut cursor) = (Vec::new(), Vec::<u8>::new());
    loop {
        let (items, next) = page(&cursor).await?;
        found.extend(items);
        if next.is_empty() {
            return Ok(found);
        }
        cursor = next;
    }
}

pub(crate) fn page_of(page: place_page::Reader<'_>) -> capnp::Result<(Vec<Place>, Vec<u8>)> {
    let items = page
        .get_items()?
        .iter()
        .map(read_place)
        .collect::<capnp::Result<_>>()?;
    Ok((items, page.get_next()?.get_opaque()?.to_vec()))
}

pub(crate) fn problem(error: &capnp::Error) -> ApiError {
    match Problem::from_error(error) {
        Some(p) if p.is_unsupported() => ApiError::Unsupported(p.detail),
        Some(p) if p.status == 404 => ApiError::NotFound(p.detail),
        Some(p) => ApiError::Failed(p.detail),
        None => ApiError::Failed(error.to_string()),
    }
}

pub(crate) fn parse_id(id: &str) -> Result<Uuid, ApiError> {
    id.parse()
        .map_err(|_| ApiError::NotFound(format!("\"{id}\" names no place.")))
}

pub(crate) fn read_place(p: place::Reader<'_>) -> capnp::Result<Place> {
    let envelope = read_envelope(p.get_envelope()?)?;
    Ok(Place {
        id: envelope.id.to_string(),
        name: envelope.name,
        geojson: p.get_geojson()?.to_string()?,
    })
}

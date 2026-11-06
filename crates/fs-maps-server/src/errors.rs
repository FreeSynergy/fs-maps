//! Errors of the core as RPC errors.
//!
//! Pattern: Adapter — each error the core can name becomes a typed RFC 9457 problem, so a consumer
//! can tell "not found" from "not possible" without reading a message.

use fs_interfaces::rpc::{Problem, failed, unsupported};
use fs_maps_core::Error;

const NOT_FOUND: &str = "https://freesynergy.net/problems/places/not-found";
const CONFLICT: &str = "https://freesynergy.net/problems/places/conflict";
const INVALID: &str = "https://freesynergy.net/problems/places/invalid";
const POLICY: &str = "https://freesynergy.net/problems/places/usage-policy";
const TOO_LARGE: &str = "https://freesynergy.net/problems/places/too-large";
const SOURCE_FAILED: &str = "https://freesynergy.net/problems/places/tile-source-failed";

/// A core error as an RPC error.
pub(crate) fn rpc_error(e: Error) -> capnp::Error {
    let text = e.to_string();
    match e {
        Error::Unsupported(not_possible) => unsupported(&not_possible),
        Error::NotFound(_) => not_found(text),
        Error::InvalidTile { .. }
        | Error::InvalidBBox(_)
        | Error::InvalidZoom { .. }
        | Error::GeoUri(_)
        | Error::GeoJson(_)
        | Error::Json(_) => invalid(text),
        Error::Policy(_) => Problem::new(POLICY, "Forbidden by policy", text, 403).into_error(),
        Error::TooManyTiles { .. } => too_large(text),
        Error::Pmtiles(_) | Error::Http(_) | Error::Io { .. } => {
            Problem::new(SOURCE_FAILED, "The tile source failed", text, 502).into_error()
        }
        Error::Store(e) => store_error(&e, text),
        Error::Cancelled => failed(text),
    }
}

fn store_error(e: &fs_store::Error, text: String) -> capnp::Error {
    match e {
        fs_store::Error::NotFound(_) => not_found(text),
        fs_store::Error::Invalid(_) => invalid(text),
        fs_store::Error::Conflict(_) | fs_store::Error::OriginalImmutable(_) => {
            Problem::new(CONFLICT, "Conflict", text, 409).into_error()
        }
        _ => failed(text),
    }
}

fn not_found(detail: String) -> capnp::Error {
    Problem::new(NOT_FOUND, "Place not found", detail, 404).into_error()
}

/// The request itself is not acceptable.
pub(crate) fn invalid(detail: impl Into<String>) -> capnp::Error {
    Problem::new(INVALID, "Invalid request", detail, 422).into_error()
}

/// More than fits one message: refused, because a larger message would end the whole connection.
pub(crate) fn too_large(detail: impl Into<String>) -> capnp::Error {
    Problem::new(TOO_LARGE, "Too large", detail, 413).into_error()
}

#[cfg(test)]
mod tests {
    use fs_store::ObjectId;

    use super::*;

    fn status(e: Error) -> Option<u16> {
        Problem::from_error(&rpc_error(e)).map(|p| p.status)
    }

    #[test]
    fn errors_no_request_can_reach_keep_their_type() {
        let id = ObjectId::new();
        assert_eq!(status(Error::Policy("no".into())), Some(403));
        let tiles = Error::TooManyTiles { tiles: 2, limit: 1 };
        assert_eq!(status(tiles), Some(413));
        let zoom = Error::InvalidZoom { min: 2, max: 1 };
        assert_eq!(status(zoom), Some(422));
        assert_eq!(status(fs_store::Error::NotFound(id).into()), Some(404));
        let conflict = fs_store::Error::Conflict("x".into());
        assert_eq!(status(conflict.into()), Some(409));
        assert_eq!(
            status(fs_store::Error::OriginalImmutable(id).into()),
            Some(409)
        );
        assert_eq!(
            status(fs_store::Error::Invalid("x".into()).into()),
            Some(422)
        );
        assert_eq!(status(fs_store::Error::Backend("x".into()).into()), None);
        assert_eq!(status(Error::Cancelled), None);
    }
}

//! Errors of the core as RPC errors.
//!
//! Pattern: Adapter — each error the core can name becomes a typed RFC 9457 problem, so a consumer
//! can tell "not found" from "not possible" without reading a message. What fails on data already
//! stored is not the caller's fault, so it is never a 422.

use fs_interfaces::rpc::{Problem, failed, too_large, unsupported};
use fs_maps_core::Error;
use fs_objects::Error as Store;

const NOT_FOUND: &str = "https://freesynergy.net/problems/places/not-found";
const CONFLICT: &str = "https://freesynergy.net/problems/places/conflict";
const INVALID: &str = "https://freesynergy.net/problems/places/invalid";
const POLICY: &str = "https://freesynergy.net/problems/places/usage-policy";
const SOURCE_FAILED: &str = "https://freesynergy.net/problems/places/tile-source-failed";
const STORE_FAILED: &str = "https://freesynergy.net/problems/places/store-failed";
const STORED_DATA: &str = "https://freesynergy.net/problems/places/stored-data-invalid";

/// A core error as an RPC error. The `GeoJSON` errors mean the request here: stored places are read
/// through [`stored_data`].
pub(crate) fn rpc_error(e: Error) -> capnp::Error {
    let text = e.to_string();
    match e {
        Error::Unsupported(not_possible) => unsupported(&not_possible),
        Error::NotFound(_) => not_found(text),
        Error::InvalidTile { .. }
        | Error::InvalidBBox(_)
        | Error::InvalidZoom { .. }
        | Error::GeoUri(_)
        | Error::GeoJson(_) => invalid(text),
        Error::Policy(_) => Problem::new(POLICY, "Forbidden by policy", text, 403).into_error(),
        Error::TooManyTiles { .. } | Error::TileTooLarge { .. } => too_large(text),
        Error::Pmtiles(_)
        | Error::Http(_)
        | Error::Io { .. }
        | Error::Archive(_)
        | Error::SourceUnavailable(_) => {
            Problem::new(SOURCE_FAILED, "The tile source failed", text, 502).into_error()
        }
        Error::Store(e) => store_error(&e, text),
        Error::Json(_) | Error::Cancelled => failed(text),
    }
}

fn store_error(e: &Store, text: String) -> capnp::Error {
    match e {
        Store::NotFound(_) => not_found(text),
        // What the store refuses to write comes from the request.
        Store::Invalid(_) => invalid(text),
        Store::Conflict(_) | Store::OriginalImmutable(_) => {
            Problem::new(CONFLICT, "Conflict", text, 409).into_error()
        }
        Store::Serde(_) => stored_problem(text),
        Store::EncryptionRequired(_) | Store::Key(_) | Store::Backend(_) => {
            Problem::new(STORE_FAILED, "The store failed", text, 500).into_error()
        }
    }
}

fn stored_problem(detail: String) -> capnp::Error {
    Problem::new(STORED_DATA, "A stored place no longer reads", detail, 500).into_error()
}

/// A stored place that no longer reads as a place.
pub(crate) fn stored_data(e: &Error) -> capnp::Error {
    stored_problem(e.to_string())
}

fn not_found(detail: String) -> capnp::Error {
    Problem::new(NOT_FOUND, "Place not found", detail, 404).into_error()
}

fn invalid(detail: String) -> capnp::Error {
    Problem::new(INVALID, "Invalid place", detail, 422).into_error()
}

#[cfg(test)]
mod tests {
    use fs_interfaces::rpc::TOO_LARGE;
    use fs_objects::ObjectId;

    use super::*;

    fn typed(e: Error) -> Option<Problem> {
        Problem::from_error(&rpc_error(e))
    }

    fn seen(e: Error) -> (String, u16) {
        let p = typed(e).expect("a typed problem");
        (p.problem_type, p.status)
    }

    fn is(kind: &str, status: u16) -> (String, u16) {
        (kind.to_string(), status)
    }

    #[test]
    fn errors_no_request_can_reach_keep_their_type() {
        let id = ObjectId::new();
        let serde = serde_json::from_str::<u8>("x").unwrap_err();
        assert_eq!(seen(Error::Policy("no".into())), is(POLICY, 403));
        let tiles = Error::TooManyTiles { tiles: 2, limit: 1 };
        assert_eq!(seen(tiles), is(TOO_LARGE, 413));
        let zoom = Error::InvalidZoom { min: 2, max: 1 };
        assert_eq!(seen(zoom), is(INVALID, 422));
        assert_eq!(seen(Store::NotFound(id).into()), is(NOT_FOUND, 404));
        assert_eq!(seen(Store::Conflict("x".into()).into()), is(CONFLICT, 409));
        let immutable = Store::OriginalImmutable(id);
        assert_eq!(seen(immutable.into()), is(CONFLICT, 409));
        assert_eq!(seen(Store::Invalid("x".into()).into()), is(INVALID, 422));
        assert_eq!(seen(Store::Serde(serde).into()), is(STORED_DATA, 500));
        for failing in [
            Store::Backend("x".into()),
            Store::Key("x".into()),
            Store::EncryptionRequired(id),
        ] {
            assert_eq!(seen(failing.into()), is(STORE_FAILED, 500));
        }
        assert_eq!(typed(Error::Cancelled), None);
    }
}

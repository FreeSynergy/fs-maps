//! Display name of a GeoJSON feature.
//!
//! Pattern: Strategy (fallback order) — the first usable of `name`, `name:en`, any `name:*`,
//! `title` becomes the envelope name.

use serde_json::{Map, Value};

/// Name for the envelope: `name`, then `name:en`, then any `name:*`, then `title`.
pub fn feature_name(props: &Map<String, Value>) -> Option<String> {
    let s = |k: &str| {
        props
            .get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    s("name")
        .or_else(|| s("name:en"))
        .or_else(|| {
            let mut keys: Vec<_> = props.keys().filter(|k| k.starts_with("name:")).collect();
            keys.sort();
            keys.into_iter().find_map(|k| s(k))
        })
        .or_else(|| s("title"))
}

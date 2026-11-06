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

/// Every name of the feature: `name`, every `name:*` (sorted), `title`; without duplicates. They
/// are what `findByName` searches.
pub fn feature_names(props: &Map<String, Value>) -> Vec<String> {
    let mut keys: Vec<&String> = props
        .keys()
        .filter(|k| *k == "name" || k.starts_with("name:") || *k == "title")
        .collect();
    keys.sort();
    let mut out: Vec<String> = vec![];
    for k in keys {
        if let Some(v) = props[k].as_str().map(str::trim).filter(|v| !v.is_empty())
            && !out.iter().any(|o| o == v)
        {
            out.push(v.to_owned());
        }
    }
    out
}

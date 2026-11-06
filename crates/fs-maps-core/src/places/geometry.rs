//! GeoJSON geometry validation and bounding boxes.
//!
//! Pattern: Interpreter-style recursion — walks the geometry tree by nesting depth and
//! validates every position (RFC 7946, WGS84 ranges), and the shape of lines and rings. Empty
//! `coordinates` are allowed (RFC 7946 section 3.1); an empty line or ring inside them is not.

use serde_json::Value;

use crate::error::{Error, Result};
use crate::tile::BBox;

fn gj<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::GeoJson(msg.into()))
}

fn position(v: &Value) -> Result<(f64, f64)> {
    let a = v.as_array().filter(|a| (2..=3).contains(&a.len()));
    let Some(a) = a else {
        return gj("position must be [lon, lat(, alt)]");
    };
    let n = |i: usize| a[i].as_f64().filter(|x| x.is_finite());
    let (Some(lon), Some(lat)) = (n(0), n(1)) else {
        return gj("position is not numeric");
    };
    if a.len() == 3 && n(2).is_none() {
        return gj("altitude is not numeric");
    }
    if !(-180.0..=180.0).contains(&lon) || !(-90.0..=90.0).contains(&lat) {
        return gj(format!("position {lon},{lat} outside WGS84 range"));
    }
    Ok((lon, lat))
}

fn collect(coords: &Value, depth: usize, out: &mut Vec<(f64, f64)>) -> Result<()> {
    if depth == 0 {
        out.push(position(coords)?);
        return Ok(());
    }
    let Some(arr) = coords.as_array() else {
        return gj("coordinates must be an array");
    };
    arr.iter().try_for_each(|c| collect(c, depth - 1, out))
}

fn same_position(a: &Value, b: &Value) -> bool {
    let (Some(a), Some(b)) = (a.as_array(), b.as_array()) else {
        return false;
    };
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.as_f64() == q.as_f64())
}

fn line(coords: &Value) -> Result<()> {
    match coords.as_array() {
        Some(positions) if positions.len() >= 2 => Ok(()),
        _ => gj("a LineString needs two or more positions"),
    }
}

fn ring(coords: &Value) -> Result<()> {
    let positions = coords.as_array().map_or(&[][..], Vec::as_slice);
    if positions.len() < 4 {
        return gj("a linear ring needs four or more positions");
    }
    if !same_position(&positions[0], &positions[positions.len() - 1]) {
        return gj("a linear ring must end where it begins");
    }
    Ok(())
}

fn members(coords: &Value) -> &[Value] {
    coords.as_array().map_or(&[][..], Vec::as_slice)
}

/// What RFC 7946 section 3.1 asks of the shape; the positions are valid by now.
fn check_shape(kind: &str, coords: &Value) -> Result<()> {
    match kind {
        "LineString" if members(coords).is_empty() => Ok(()),
        "LineString" => line(coords),
        "MultiLineString" => members(coords).iter().try_for_each(line),
        "Polygon" => members(coords).iter().try_for_each(ring),
        "MultiPolygon" => members(coords)
            .iter()
            .try_for_each(|polygon| members(polygon).iter().try_for_each(ring)),
        _ => Ok(()),
    }
}

/// Validates a geometry and returns all its positions.
pub(super) fn geometry_positions(g: &Value) -> Result<(String, Vec<(f64, f64)>)> {
    let Some(t) = g.get("type").and_then(Value::as_str) else {
        return gj("geometry without type");
    };
    let mut pts = Vec::new();
    if t == "GeometryCollection" {
        let Some(gs) = g.get("geometries").and_then(Value::as_array) else {
            return gj("GeometryCollection without geometries");
        };
        for sub in gs {
            pts.extend(geometry_positions(sub)?.1);
        }
        return Ok((t.to_owned(), pts));
    }
    let depth = match t {
        "Point" => 0,
        "MultiPoint" | "LineString" => 1,
        "MultiLineString" | "Polygon" => 2,
        "MultiPolygon" => 3,
        other => return gj(format!("unknown geometry type {other:?}")),
    };
    let Some(c) = g.get("coordinates") else {
        return gj("geometry without coordinates");
    };
    collect(c, depth, &mut pts)?;
    check_shape(t, c)?;
    Ok((t.to_owned(), pts))
}

/// Bounding box of a feature's geometry, `None` for a null geometry. Antimeridian-crossing
/// geometries yield a box spanning the world's longitude (RFC 7946 section 5 allows that).
pub fn feature_bbox(feature: &Value) -> Result<Option<BBox>> {
    let Some(g) = feature.get("geometry").filter(|g| !g.is_null()) else {
        return Ok(None);
    };
    let (_, pts) = geometry_positions(g)?;
    if pts.is_empty() {
        return Ok(None);
    }
    let f = |sel: fn(&(f64, f64)) -> f64, init: f64, op: fn(f64, f64) -> f64| {
        pts.iter().map(sel).fold(init, op)
    };
    Ok(Some(BBox::new(
        f(|p| p.0, f64::INFINITY, f64::min),
        f(|p| p.1, f64::INFINITY, f64::min),
        f(|p| p.0, f64::NEG_INFINITY, f64::max),
        f(|p| p.1, f64::NEG_INFINITY, f64::max),
    )?))
}

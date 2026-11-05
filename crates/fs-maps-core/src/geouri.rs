//! `geo:` URIs, RFC 5870: `geo:<lat>,<lon>[,<alt>][;crs=wgs84][;u=<uncertainty>][;<param>[=<value>]]*`.
//!
//! Only the default CRS `wgs84` is supported (RFC 5870 defines no other). Scheme and parameter
//! names are case-insensitive. At the poles (|lat| = 90) the longitude is normalised to 0, and
//! longitude -180 to 180, as the RFC prescribes for comparison.

use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct GeoUri {
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    /// Uncertainty in metres (`;u=`).
    pub uncertainty: Option<f64>,
    /// Further parameters, names lower-cased, in order.
    pub params: Vec<(String, Option<String>)>,
}

fn bad<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::GeoUri(msg.into()))
}

/// RFC 5870 number: `[-]digits[.digits]` (a leading `+` is not allowed).
fn num(s: &str, what: &str) -> Result<f64> {
    let body = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = body.split_once('.').map_or((body, None), |(a, b)| (a, Some(b)));
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) || frac.is_some_and(|f| !digits(f)) {
        return bad(format!("{what} is not a number: {s:?}"));
    }
    s.parse::<f64>().or_else(|_| bad(format!("{what} out of range: {s:?}")))
}

impl GeoUri {
    pub fn new(lat: f64, lon: f64) -> Result<Self> {
        Self::build(lat, lon, None, None, vec![])
    }

    fn build(lat: f64, lon: f64, alt: Option<f64>, u: Option<f64>, params: Vec<(String, Option<String>)>) -> Result<Self> {
        if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
            return bad(format!("latitude {lat} outside -90..90"));
        }
        if !lon.is_finite() || !(-180.0..=180.0).contains(&lon) {
            return bad(format!("longitude {lon} outside -180..180"));
        }
        if u.is_some_and(|u| !u.is_finite() || u < 0.0) {
            return bad("uncertainty must be >= 0");
        }
        let lon = if lat.abs() == 90.0 {
            0.0
        } else if lon == -180.0 {
            180.0
        } else {
            lon
        };
        Ok(Self {
            lat: lat + 0.0,
            lon: lon + 0.0,
            alt,
            uncertainty: u,
            params,
        })
    }

    pub fn parse(s: &str) -> Result<Self> {
        let Some(rest) = s.get(..4).filter(|p| p.eq_ignore_ascii_case("geo:")).map(|_| &s[4..]) else {
            return bad("missing geo: scheme");
        };
        let mut parts = rest.split(';');
        let coords: Vec<&str> = parts.next().unwrap_or("").split(',').collect();
        if !(2..=3).contains(&coords.len()) {
            return bad("expected lat,lon[,alt]");
        }
        let lat = num(coords[0], "latitude")?;
        let lon = num(coords[1], "longitude")?;
        let alt = coords.get(2).map(|a| num(a, "altitude")).transpose()?;
        let mut u = None;
        let mut params = Vec::new();
        for p in parts {
            if p.is_empty() {
                return bad("empty parameter");
            }
            let (name, val) = p.split_once('=').map_or((p, None), |(n, v)| (n, Some(v)));
            let name = name.to_ascii_lowercase();
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return bad(format!("bad parameter name {name:?}"));
            }
            match (name.as_str(), val) {
                ("crs", Some(v)) if v.eq_ignore_ascii_case("wgs84") => {}
                ("crs", _) => return bad("unsupported crs (only wgs84)"),
                ("u", Some(v)) => u = Some(num(v, "uncertainty")?),
                ("u", None) => return bad("u needs a value"),
                _ => params.push((name, val.map(str::to_owned))),
            }
        }
        Self::build(lat, lon, alt, u, params)
    }
}

impl FromStr for GeoUri {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

impl fmt::Display for GeoUri {
    /// Canonical form: lower-case scheme, default `crs` omitted, `u` before other parameters.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "geo:{},{}", self.lat, self.lon)?;
        if let Some(a) = self.alt {
            write!(f, ",{}", a + 0.0)?;
        }
        if let Some(u) = self.uncertainty {
            write!(f, ";u={u}")?;
        }
        for (n, v) in &self.params {
            match v {
                Some(v) => write!(f, ";{n}={v}")?,
                None => write!(f, ";{n}")?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn rfc_examples() {
        // RFC 5870 section 3.3 / 9.1 examples
        let g = GeoUri::parse("geo:13.4125,103.8667").unwrap();
        assert_eq!((g.lat, g.lon, g.alt), (13.4125, 103.8667, None));
        let g = GeoUri::parse("geo:48.2010,16.3695,183").unwrap();
        assert_eq!(g.alt, Some(183.0));
        let g = GeoUri::parse("geo:48.198634,16.371648;crs=wgs84;u=40").unwrap();
        assert_eq!(g.uncertainty, Some(40.0));
        assert_eq!(g.to_string(), "geo:48.198634,16.371648;u=40");
        // equality rules: poles and +-180
        assert_eq!(GeoUri::parse("geo:90,-22.43;crs=WGS84").unwrap(), GeoUri::parse("geo:90,46").unwrap());
        assert_eq!(GeoUri::parse("geo:22.3,-118").unwrap().lon, -118.0);
        assert_eq!(GeoUri::parse("GEO:0,-180").unwrap().lon, 180.0);
        // unknown parameter kept
        let g = GeoUri::parse("geo:52.52,13.405;foo=bar;Baz").unwrap();
        assert_eq!(g.to_string(), "geo:52.52,13.405;foo=bar;baz");
    }

    #[test]
    fn rejects_garbage() {
        for s in [
            "", "geo:", "geo:1", "geo:1,2,3,4", "geo:a,b", "geo:91,0", "geo:0,181", "geo:+1,2",
            "geo:1.,2", "geo:.5,2", "geo:1,2;crs=epsg:4326", "geo:1,2;u", "geo:1,2;u=-3", "geo:1,2;;",
            "http://x", "geo:1,2;=x", "geo:1,2;a b=c",
        ] {
            assert!(GeoUri::parse(s).is_err(), "{s:?} should fail");
        }
    }

    proptest! {
        #[test]
        fn roundtrip(lat in -90.0f64..=90.0, lon in -180.0f64..=180.0, alt in proptest::option::of(-500.0f64..9000.0), u in proptest::option::of(0.0f64..1000.0)) {
            let g = GeoUri::build(lat, lon, alt, u, vec![]).unwrap();
            let s = g.to_string();
            prop_assert_eq!(GeoUri::parse(&s).unwrap(), g);
        }
    }
}

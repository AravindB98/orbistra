//! TLE catalog parsing and per-object orbital metadata.

use crate::{MU_EARTH, R_EARTH_KM};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("no valid TLE sets found in input")]
    Empty,
}

/// A tracked object in the space catalog.
pub struct SpaceObject {
    pub norad_id: u64,
    pub name: String,
    pub elements: sgp4::Elements,
    /// Perigee altitude above the equatorial radius, km.
    pub perigee_km: f64,
    /// Apogee altitude above the equatorial radius, km.
    pub apogee_km: f64,
}

/// Lightweight, serializable summary of an object (for APIs / UIs).
#[derive(Debug, Clone, Serialize)]
pub struct ObjectSummary {
    pub norad_id: u64,
    pub name: String,
    pub perigee_km: f64,
    pub apogee_km: f64,
}

impl SpaceObject {
    fn from_elements(elements: sgp4::Elements) -> Self {
        // Semi-major axis from mean motion (rev/day -> rad/s).
        let n_rad_s = elements.mean_motion * 2.0 * std::f64::consts::PI / 86400.0;
        let a_km = (MU_EARTH / (n_rad_s * n_rad_s)).cbrt();
        let e = elements.eccentricity;
        let perigee_km = a_km * (1.0 - e) - R_EARTH_KM;
        let apogee_km = a_km * (1.0 + e) - R_EARTH_KM;
        let name = elements
            .object_name
            .clone()
            .unwrap_or_else(|| format!("NORAD {}", elements.norad_id));
        SpaceObject {
            norad_id: elements.norad_id,
            name,
            elements,
            perigee_km,
            apogee_km,
        }
    }

    pub fn summary(&self) -> ObjectSummary {
        ObjectSummary {
            norad_id: self.norad_id,
            name: self.name.clone(),
            perigee_km: self.perigee_km,
            apogee_km: self.apogee_km,
        }
    }
}

/// An in-memory space object catalog.
pub struct Catalog {
    pub objects: Vec<SpaceObject>,
}

impl Catalog {
    /// Parse a catalog from TLE text. Supports both 2-line and 3-line
    /// (name + 2 lines) formats, mixed freely. Malformed sets are skipped.
    pub fn from_tle_str(text: &str) -> Result<Self, CatalogError> {
        let lines: Vec<&str> = text
            .lines()
            .map(|l| l.trim_end())
            .filter(|l| !l.trim().is_empty())
            .collect();

        let mut objects = Vec::new();
        let mut pending_name: Option<String> = None;
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            if line.starts_with("1 ") && i + 1 < lines.len() && lines[i + 1].starts_with("2 ") {
                let name = pending_name.take();
                if let Ok(elements) =
                    sgp4::Elements::from_tle(name, line.as_bytes(), lines[i + 1].as_bytes())
                {
                    objects.push(SpaceObject::from_elements(elements));
                }
                i += 2;
            } else {
                // Name line (strip the optional "0 " prefix used by some sources).
                let name = line.strip_prefix("0 ").unwrap_or(line).trim().to_string();
                pending_name = Some(name);
                i += 1;
            }
        }

        if objects.is_empty() {
            return Err(CatalogError::Empty);
        }
        Ok(Catalog { objects })
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISS_TLE: &str = "ISS (ZARYA)
1 25544U 98067A   24001.50000000  .00016717  00000+0  30777-3 0  9990
2 25544  51.6400 208.9163 0006317  69.9862 290.2117 15.49815350430127";

    #[test]
    fn parses_named_tle() {
        let cat = Catalog::from_tle_str(ISS_TLE).unwrap();
        assert_eq!(cat.len(), 1);
        let obj = &cat.objects[0];
        assert_eq!(obj.norad_id, 25544);
        assert!(obj.name.contains("ISS"));
        // ISS orbits at roughly 400 km.
        assert!(obj.perigee_km > 300.0 && obj.apogee_km < 500.0);
    }

    #[test]
    fn parses_two_line_only() {
        let two_line: String = ISS_TLE.lines().skip(1).collect::<Vec<_>>().join("\n");
        let cat = Catalog::from_tle_str(&two_line).unwrap();
        assert_eq!(cat.len(), 1);
        assert_eq!(cat.objects[0].norad_id, 25544);
    }

    #[test]
    fn empty_input_errors() {
        assert!(Catalog::from_tle_str("").is_err());
    }
}

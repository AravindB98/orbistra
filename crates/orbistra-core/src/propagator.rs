//! Parallel SGP4 batch propagation over the full catalog.

use crate::catalog::Catalog;
use chrono::{DateTime, NaiveDateTime, Utc};
use rayon::prelude::*;
use serde::Serialize;

/// A propagated state vector in the TEME frame.
#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub norad_id: u64,
    pub name: String,
    /// Position, km (TEME).
    pub position_km: [f64; 3],
    /// Velocity, km/s (TEME).
    pub velocity_km_s: [f64; 3],
}

struct Entry {
    norad_id: u64,
    name: String,
    perigee_km: f64,
    apogee_km: f64,
    epoch: NaiveDateTime,
    constants: sgp4::Constants,
}

/// Precomputed SGP4 constants for every object in a catalog, ready for
/// fast repeated propagation. Objects whose elements fail SGP4
/// initialization (e.g. decayed or deep-space edge cases) are skipped.
pub struct Propagator {
    entries: Vec<Entry>,
}

impl Propagator {
    pub fn new(catalog: &Catalog) -> Self {
        let entries = catalog
            .objects
            .iter()
            .filter_map(|obj| {
                let constants = sgp4::Constants::from_elements(&obj.elements).ok()?;
                Some(Entry {
                    norad_id: obj.norad_id,
                    name: obj.name.clone(),
                    perigee_km: obj.perigee_km,
                    apogee_km: obj.apogee_km,
                    epoch: obj.elements.datetime,
                    constants,
                })
            })
            .collect();
        Propagator { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Perigee/apogee altitudes (km) per object, aligned with internal indices.
    pub fn altitude_bands(&self) -> Vec<(f64, f64)> {
        self.entries
            .iter()
            .map(|e| (e.perigee_km, e.apogee_km))
            .collect()
    }

    /// Identity (norad_id, name) per object, aligned with internal indices.
    pub fn identities(&self) -> Vec<(u64, String)> {
        self.entries
            .iter()
            .map(|e| (e.norad_id, e.name.clone()))
            .collect()
    }

    fn propagate_entry(entry: &Entry, t: DateTime<Utc>) -> Option<State> {
        let dt_min = (t.naive_utc() - entry.epoch).num_milliseconds() as f64 / 60_000.0;
        let prediction = entry
            .constants
            .propagate(sgp4::MinutesSinceEpoch(dt_min))
            .ok()?;
        let p = prediction.position;
        let v = prediction.velocity;
        if !(p[0].is_finite() && p[1].is_finite() && p[2].is_finite()) {
            return None;
        }
        Some(State {
            norad_id: entry.norad_id,
            name: entry.name.clone(),
            position_km: p,
            velocity_km_s: v,
        })
    }

    /// Lightweight parallel snapshot: `(index, position_km, velocity_km_s)`
    /// without identity strings — used by high-rate screening loops.
    pub fn snapshot(&self, t: DateTime<Utc>) -> Vec<(usize, [f64; 3], [f64; 3])> {
        self.entries
            .par_iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let dt_min = (t.naive_utc() - e.epoch).num_milliseconds() as f64 / 60_000.0;
                let p = e
                    .constants
                    .propagate(sgp4::MinutesSinceEpoch(dt_min))
                    .ok()?;
                if !(p.position[0].is_finite()
                    && p.position[1].is_finite()
                    && p.position[2].is_finite())
                {
                    return None;
                }
                Some((i, p.position, p.velocity))
            })
            .collect()
    }

    /// Propagate a single object by internal index.
    pub fn state_at(&self, index: usize, t: DateTime<Utc>) -> Option<State> {
        self.entries
            .get(index)
            .and_then(|e| Self::propagate_entry(e, t))
    }

    /// Position/velocity only (no identity allocation) — fast path for
    /// screening refinement loops.
    pub fn pos_vel_at(&self, index: usize, t: DateTime<Utc>) -> Option<([f64; 3], [f64; 3])> {
        let e = self.entries.get(index)?;
        let dt_min = (t.naive_utc() - e.epoch).num_milliseconds() as f64 / 60_000.0;
        let p = e
            .constants
            .propagate(sgp4::MinutesSinceEpoch(dt_min))
            .ok()?;
        if !(p.position[0].is_finite() && p.position[1].is_finite() && p.position[2].is_finite()) {
            return None;
        }
        Some((p.position, p.velocity))
    }

    /// Propagate the entire catalog to time `t` in parallel.
    /// Returns `(index, state)` pairs; objects that fail to propagate
    /// (decayed, numerical issues) are omitted.
    pub fn all_states_indexed(&self, t: DateTime<Utc>) -> Vec<(usize, State)> {
        self.entries
            .par_iter()
            .enumerate()
            .filter_map(|(i, e)| Self::propagate_entry(e, t).map(|s| (i, s)))
            .collect()
    }

    /// Propagate the entire catalog to time `t` in parallel.
    pub fn all_states(&self, t: DateTime<Utc>) -> Vec<State> {
        self.entries
            .par_iter()
            .filter_map(|e| Self::propagate_entry(e, t))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Catalog;
    use chrono::TimeZone;

    const ISS_TLE: &str = "ISS (ZARYA)
1 25544U 98067A   24001.50000000  .00016717  00000+0  30777-3 0  9990
2 25544  51.6400 208.9163 0006317  69.9862 290.2117 15.49815350430127";

    #[test]
    fn propagates_iss_to_plausible_state() {
        let cat = Catalog::from_tle_str(ISS_TLE).unwrap();
        let prop = Propagator::new(&cat);
        assert_eq!(prop.len(), 1);

        let t = Utc.with_ymd_and_hms(2024, 1, 1, 18, 0, 0).unwrap();
        let state = prop.state_at(0, t).expect("propagation should succeed");

        let r = (state.position_km[0].powi(2)
            + state.position_km[1].powi(2)
            + state.position_km[2].powi(2))
        .sqrt();
        let v = (state.velocity_km_s[0].powi(2)
            + state.velocity_km_s[1].powi(2)
            + state.velocity_km_s[2].powi(2))
        .sqrt();

        // LEO sanity: ~6700-6800 km radius, ~7.7 km/s speed.
        assert!((6600.0..6900.0).contains(&r), "radius = {r}");
        assert!((7.4..8.0).contains(&v), "speed = {v}");
    }
}

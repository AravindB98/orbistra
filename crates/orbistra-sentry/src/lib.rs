//! # orbistra-sentry
//!
//! Conjunction screening over the full space catalog.
//!
//! Pipeline (classic three-stage sieve):
//! 1. **Altitude-band prefilter** — pairs whose perigee/apogee bands don't
//!    overlap (with margin) can never come close; they are excluded up front.
//! 2. **Coarse sweep** — the catalog is propagated at a coarse time step and
//!    binned into a uniform spatial hash grid. Only pairs sharing a
//!    neighborhood are considered candidates. The grid capture radius
//!    accounts for the maximum relative motion within one coarse step.
//! 3. **Refinement** — for each candidate pair/time, relative distance is
//!    sampled at a fine step around the candidate time and the time of
//!    closest approach (TCA) is refined by quadratic interpolation.
//!
//! Collision probability is a simplified 2-D encounter-plane estimate with a
//! configurable combined position uncertainty (TLEs carry no covariance —
//! see `docs/ARCHITECTURE.md` for the roadmap to CDM/covariance ingestion).

use chrono::{DateTime, Duration, Utc};
use orbistra_core::Propagator;
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;

/// Maximum plausible relative speed between two Earth orbiters, km/s.
const MAX_REL_SPEED_KM_S: f64 = 16.0;

#[derive(Debug, Clone)]
pub struct ScreeningConfig {
    /// Screening window start.
    pub start: DateTime<Utc>,
    /// Screening horizon, hours.
    pub horizon_hours: f64,
    /// Coarse sweep step, seconds.
    pub coarse_step_s: f64,
    /// Fine refinement step, seconds.
    pub fine_step_s: f64,
    /// Report conjunctions with miss distance below this, km.
    pub threshold_km: f64,
    /// Combined 1-sigma position uncertainty for both objects, km.
    pub combined_sigma_km: f64,
    /// Combined hard-body radius, km (default 20 m).
    pub hard_body_radius_km: f64,
    /// Ignore pairs below this relative speed at TCA, km/s. Filters
    /// docked/attached objects (ISS modules, docked vehicles) and
    /// station-keeping formations that would otherwise flood the report.
    pub min_relative_speed_km_s: f64,
}

impl Default for ScreeningConfig {
    fn default() -> Self {
        ScreeningConfig {
            start: Utc::now(),
            horizon_hours: 24.0,
            coarse_step_s: 60.0,
            fine_step_s: 1.0,
            threshold_km: 5.0,
            combined_sigma_km: 1.0,
            hard_body_radius_km: 0.020,
            min_relative_speed_km_s: 0.01,
        }
    }
}

/// A predicted close approach between two cataloged objects.
#[derive(Debug, Clone, Serialize)]
pub struct Conjunction {
    pub norad_id_a: u64,
    pub name_a: String,
    pub norad_id_b: u64,
    pub name_b: String,
    /// Time of closest approach (UTC).
    pub tca: DateTime<Utc>,
    /// Miss distance at TCA, km.
    pub miss_distance_km: f64,
    /// Relative speed at TCA, km/s.
    pub relative_speed_km_s: f64,
    /// Simplified 2-D collision probability estimate.
    pub probability: f64,
}

/// Run the full screening pipeline. Returns conjunctions sorted by
/// miss distance, ascending.
pub fn screen(prop: &Propagator, cfg: &ScreeningConfig) -> Vec<Conjunction> {
    let bands = prop.altitude_bands();
    let ids = prop.identities();
    let n_steps = (cfg.horizon_hours * 3600.0 / cfg.coarse_step_s).ceil() as i64;
    // Anything within one coarse step of relative motion can close the gap.
    let capture_km = cfg.threshold_km + MAX_REL_SPEED_KM_S * cfg.coarse_step_s / 2.0;
    let band_margin_km = capture_km;

    // Stage 2: coarse sweep with spatial hashing, parallel over time steps.
    //
    // Memory discipline ("smart sieve"): a pair only becomes a candidate if
    // its *current* separation could shrink below the threshold within one
    // coarse step at the pair's actual relative speed:
    //     d < threshold + |v_rel| · Δt
    // This rejects the enormous population of slowly-drifting co-orbital
    // neighbors (e.g. within a Starlink shell) that a purely geometric
    // capture radius would flood the candidate list with.
    let candidate_events: Vec<(usize, usize, i64)> = (0..n_steps)
        .into_par_iter()
        .flat_map(|k| {
            let t =
                cfg.start + Duration::milliseconds((k as f64 * cfg.coarse_step_s * 1000.0) as i64);
            let states = prop.snapshot(t);
            let cell = capture_km;
            let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
            for (slot, (_, p, _)) in states.iter().enumerate() {
                let key = (
                    (p[0] / cell).floor() as i64,
                    (p[1] / cell).floor() as i64,
                    (p[2] / cell).floor() as i64,
                );
                grid.entry(key).or_default().push(slot);
            }
            let mut found = Vec::new();
            for (&(cx, cy, cz), members) in &grid {
                // Compare within this cell and against forward neighbor cells
                // (half the 26-neighborhood, to avoid double counting).
                let neighbors: [(i64, i64, i64); 13] = [
                    (1, 0, 0),
                    (0, 1, 0),
                    (0, 0, 1),
                    (1, 1, 0),
                    (1, 0, 1),
                    (0, 1, 1),
                    (1, 1, 1),
                    (1, -1, 0),
                    (1, 0, -1),
                    (0, 1, -1),
                    (1, 1, -1),
                    (1, -1, 1),
                    (1, -1, -1),
                ];
                for (mi, &sa) in members.iter().enumerate() {
                    for &sb in &members[mi + 1..] {
                        check_pair(&states, sa, sb, cfg, &bands, band_margin_km, k, &mut found);
                    }
                    for &(dx, dy, dz) in &neighbors {
                        if let Some(other) = grid.get(&(cx + dx, cy + dy, cz + dz)) {
                            for &sb in other {
                                check_pair(
                                    &states,
                                    sa,
                                    sb,
                                    cfg,
                                    &bands,
                                    band_margin_km,
                                    k,
                                    &mut found,
                                );
                            }
                        }
                    }
                }
            }
            found
        })
        .collect();

    // Group candidate events by pair.
    let mut by_pair: HashMap<(usize, usize), Vec<DateTime<Utc>>> = HashMap::new();
    for (a, b, k) in candidate_events {
        let key = if a < b { (a, b) } else { (b, a) };
        let t = cfg.start + Duration::milliseconds((k as f64 * cfg.coarse_step_s * 1000.0) as i64);
        by_pair.entry(key).or_default().push(t);
    }

    eprintln!("[sentry] coarse sweep: {} candidate pairs", by_pair.len());

    // Stage 3: refine each pair's candidate windows.
    let mut conjunctions: Vec<Conjunction> = by_pair
        .into_par_iter()
        .flat_map(|((a, b), mut times)| {
            times.sort();
            // Merge candidate times into windows separated by > 1 coarse step.
            let mut windows: Vec<(DateTime<Utc>, DateTime<Utc>)> = Vec::new();
            for t in times {
                match windows.last_mut() {
                    Some((_, end))
                        if (t - *end).num_seconds() as f64 <= cfg.coarse_step_s * 1.5 =>
                    {
                        *end = t;
                    }
                    _ => windows.push((t, t)),
                }
            }
            let pad = Duration::milliseconds((cfg.coarse_step_s * 1000.0) as i64);
            windows
                .into_iter()
                .filter_map(|(w0, w1)| refine(prop, a, b, w0 - pad, w1 + pad, cfg, &ids))
                .collect::<Vec<_>>()
        })
        .collect();

    conjunctions.sort_by(|x, y| x.miss_distance_km.total_cmp(&y.miss_distance_km));
    conjunctions
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn check_pair(
    states: &[(usize, [f64; 3], [f64; 3])],
    slot_a: usize,
    slot_b: usize,
    cfg: &ScreeningConfig,
    bands: &[(f64, f64)],
    band_margin_km: f64,
    k: i64,
    out: &mut Vec<(usize, usize, i64)>,
) {
    let (ia, pa_pos, va) = &states[slot_a];
    let (ib, pb_pos, vb) = &states[slot_b];
    if ia == ib {
        return;
    }
    // Stage 1: altitude-band prefilter.
    let (pa, aa) = bands[*ia];
    let (pb, ab) = bands[*ib];
    if pa - ab > band_margin_km || pb - aa > band_margin_km {
        return;
    }
    // Smart sieve: model relative motion as linear over one coarse step and
    // compute the minimum separation analytically:
    //   d(τ)² = |r + v·τ|²,  τ* = −(r·v)/|v|²  clamped to [0, Δt]
    // The pair is a candidate only if that minimum could dip below the
    // threshold, padded for gravity-curvature error over one step (a few km).
    let r = [
        pa_pos[0] - pb_pos[0],
        pa_pos[1] - pb_pos[1],
        pa_pos[2] - pb_pos[2],
    ];
    let v = [va[0] - vb[0], va[1] - vb[1], va[2] - vb[2]];
    let v2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    let tau = if v2 > 1e-12 {
        (-(r[0] * v[0] + r[1] * v[1] + r[2] * v[2]) / v2).clamp(0.0, cfg.coarse_step_s)
    } else {
        0.0
    };
    let dmin2 =
        (r[0] + v[0] * tau).powi(2) + (r[1] + v[1] * tau).powi(2) + (r[2] + v[2] * tau).powi(2);
    // Curvature pad: differential gravity over Δt ≈ ½·Δg·Δt² (≲ few km at 60 s).
    let reach = cfg.threshold_km + CURVATURE_PAD_KM;
    if dmin2 < reach * reach {
        out.push((*ia, *ib, k));
    }
}

/// Allowance for the error of the linear relative-motion assumption over one
/// coarse step (differential gravity), km. Sized for steps ≤ 120 s.
const CURVATURE_PAD_KM: f64 = 5.0;

#[inline]
fn dist2(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

/// Sample relative distance at the fine step over [w0, w1], find the minimum,
/// then refine TCA with a quadratic fit through the three samples around it.
fn refine(
    prop: &Propagator,
    a: usize,
    b: usize,
    w0: DateTime<Utc>,
    w1: DateTime<Utc>,
    cfg: &ScreeningConfig,
    ids: &[(u64, String)],
) -> Option<Conjunction> {
    let span_s = (w1 - w0).num_milliseconds() as f64 / 1000.0;
    let n = (span_s / cfg.fine_step_s).ceil() as i64;
    if n < 2 {
        return None;
    }

    let mut best_k = 0i64;
    let mut best_d2 = f64::INFINITY;
    let mut samples: Vec<f64> = Vec::with_capacity(n as usize + 1);
    for k in 0..=n {
        let t = w0 + chrono::Duration::milliseconds((k as f64 * cfg.fine_step_s * 1000.0) as i64);
        let ((pa, _), (pb, _)) = (prop.pos_vel_at(a, t)?, prop.pos_vel_at(b, t)?);
        let d2 = dist2(&pa, &pb);
        samples.push(d2);
        if d2 < best_d2 {
            best_d2 = d2;
            best_k = k;
        }
    }

    // Quadratic refinement around the discrete minimum.
    let mut tca_offset_s = best_k as f64 * cfg.fine_step_s;
    if best_k > 0 && best_k < n {
        let (d0, d1, d2s) = (
            samples[(best_k - 1) as usize],
            samples[best_k as usize],
            samples[(best_k + 1) as usize],
        );
        let denom = d0 - 2.0 * d1 + d2s;
        if denom.abs() > 1e-12 {
            let frac = 0.5 * (d0 - d2s) / denom;
            tca_offset_s += frac.clamp(-1.0, 1.0) * cfg.fine_step_s;
        }
    }

    let tca = w0 + chrono::Duration::milliseconds((tca_offset_s * 1000.0) as i64);
    let ((pa, va), (pb, vb)) = (prop.pos_vel_at(a, tca)?, prop.pos_vel_at(b, tca)?);
    let miss_km = dist2(&pa, &pb).sqrt();
    if miss_km > cfg.threshold_km {
        return None;
    }
    let rel_v = [va[0] - vb[0], va[1] - vb[1], va[2] - vb[2]];
    let rel_speed = (rel_v[0].powi(2) + rel_v[1].powi(2) + rel_v[2].powi(2)).sqrt();
    if rel_speed < cfg.min_relative_speed_km_s {
        // Docked / attached / tight formation — not a conjunction event.
        return None;
    }

    let (id_a, name_a) = &ids[a];
    let (id_b, name_b) = &ids[b];
    Some(Conjunction {
        norad_id_a: *id_a,
        name_a: name_a.clone(),
        norad_id_b: *id_b,
        name_b: name_b.clone(),
        tca,
        miss_distance_km: miss_km,
        relative_speed_km_s: rel_speed,
        probability: probability_2d(miss_km, cfg.combined_sigma_km, cfg.hard_body_radius_km),
    })
}

/// Simplified 2-D encounter-plane collision probability.
///
/// Assumes an isotropic combined position uncertainty `sigma` in the
/// encounter plane and a circular combined hard body of radius `hbr`.
/// For hbr << sigma this reduces to the standard Rician approximation:
/// `Pc ≈ (hbr² / (2σ²)) · exp(−d² / (2σ²))`.
pub fn probability_2d(miss_km: f64, sigma_km: f64, hbr_km: f64) -> f64 {
    if sigma_km <= 0.0 {
        return if miss_km <= hbr_km { 1.0 } else { 0.0 };
    }
    let s2 = sigma_km * sigma_km;
    ((hbr_km * hbr_km) / (2.0 * s2) * (-(miss_km * miss_km) / (2.0 * s2)).exp()).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use orbistra_core::Catalog;

    // Two objects in the same orbital plane, one trailing the other by a
    // tiny mean-anomaly offset — guaranteed repeated close approaches.
    const NEAR_PAIR: &str = "OBJECT A
1 90001U 24001A   24001.50000000  .00000000  00000+0  00000-0 0  9998
2 90001  51.6400 208.9163 0006317  69.9862 290.2117 15.49815350    18
OBJECT B
1 90002U 24001B   24001.50000000  .00000000  00000+0  00000-0 0  9999
2 90002  51.6400 208.9163 0006317  69.9862 290.2417 15.49815350    12";

    #[test]
    fn detects_engineered_conjunction() {
        let cat = Catalog::from_tle_str(NEAR_PAIR).unwrap();
        let prop = Propagator::new(&cat);
        assert_eq!(prop.len(), 2);

        let cfg = ScreeningConfig {
            start: chrono::Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap(),
            horizon_hours: 3.0,
            threshold_km: 50.0,
            // The engineered pair is co-orbital (tiny relative speed), so
            // disable the docked-object filter for this test.
            min_relative_speed_km_s: 0.0,
            ..Default::default()
        };
        let results = screen(&prop, &cfg);
        assert!(!results.is_empty(), "expected at least one conjunction");
        let c = &results[0];
        assert!(c.miss_distance_km < 50.0);
        assert!(c.probability >= 0.0 && c.probability <= 1.0);
    }

    #[test]
    fn distant_orbits_produce_nothing() {
        // ISS-like LEO vs a GEO-like orbit: altitude bands never overlap.
        let tles = "LEO OBJ
1 90003U 24001A   24001.50000000  .00000000  00000+0  00000-0 0  9990
2 90003  51.6400 208.9163 0006317  69.9862 290.2117 15.49815350    10
GEO OBJ
1 90004U 24001B   24001.50000000  .00000000  00000+0  00000-0 0  9991
2 90004   0.0500  75.0000 0002000 150.0000 210.0000  1.00270000    14";
        let cat = Catalog::from_tle_str(tles).unwrap();
        let prop = Propagator::new(&cat);
        let cfg = ScreeningConfig {
            start: chrono::Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap(),
            horizon_hours: 2.0,
            ..Default::default()
        };
        assert!(screen(&prop, &cfg).is_empty());
    }

    #[test]
    fn probability_behaves() {
        // Head-on within hard body, tiny sigma -> ~certain.
        assert!(probability_2d(0.0, 1e-6, 0.02) > 0.99);
        // Huge miss -> ~zero.
        assert!(probability_2d(100.0, 1.0, 0.02) < 1e-12);
        // Monotonically decreasing in miss distance.
        assert!(probability_2d(0.1, 1.0, 0.02) > probability_2d(1.0, 1.0, 0.02));
    }
}
